//! Handing links to the user's desktop session.
//!
//! A program inside an environment opens a link through Wine's `winebrowser`,
//! which starts a Linux program to show it - `xdg-open` by default. Started
//! from there, that program runs *inside* the environment: in its user
//! namespace, where the user is uid 0, and in its mount namespace, where it
//! holds the session open. Chromium-based browsers refuse to start as root
//! ("Running as root without --no-sandbox is not supported"), so a sign-in
//! link did nothing at all; and a browser that did start would have been
//! killed by `env stop` along with the program that opened the link.
//!
//! So Raven puts itself in place of `winebrowser`'s list, and passes the link
//! to the desktop portal (`org.freedesktop.portal.OpenURI`). The portal runs in
//! the user's session and starts their own browser there, as them. The call
//! works from inside the namespace because `busctl` and `gdbus` send no uid
//! when they authenticate: the bus then takes the caller's uid from the
//! kernel, as seen outside, which is the user's. A client that sends its uid,
//! as libdbus's `dbus-send` does, sends 0 from in here and is refused.
//!
//! A `file:` link stays with `xdg-open` inside the environment: its path may be
//! under C:, and C: exists only in the environment's mount namespace, so a
//! program outside could not open it.
//!
//! The other direction goes through desktop entries. Wine's `winemenubuilder`
//! writes one for each shortcut a program creates and for each file type and
//! link scheme it registers, and their command runs Wine outside the
//! environment. [`reroute`] makes them run through Raven instead.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

use crate::{Error, registry::text};

/// Where `winebrowser` reads the programs it may start, in `user.reg`.
const SECTION: &str = "Software\\\\Wine\\\\WineBrowser";

/// The verb `winebrowser` runs Raven with, and how a value Raven wrote is told
/// from one the user wrote.
const VERB: &str = "open-uri";

/// Opens `uri` in the user's session, or inside the environment when that is
/// the only place it can be opened.
///
/// The portal is tried through `busctl`, then `gdbus`: either is enough, and
/// a desktop without one of them usually has the other. When neither reaches
/// a portal - no desktop session, or one without the portal service - the link
/// goes to the programs `winebrowser` would have tried had Raven not replaced
/// its list, inside the environment, as before.
pub fn open_uri(uri: &str) -> Result<(), Error> {
    if for_the_desktop(uri) && (portal_via_busctl(uri) || portal_via_gdbus(uri)) {
        return Ok(());
    }
    // As winebrowser does: the first program that starts is the answer,
    // whatever it then says.
    let programs = fallbacks(uri);
    for &program in programs {
        // Any failure to start moves on, as it does in winebrowser.
        let Ok(status) = Command::new(program).arg(uri).status() else {
            continue;
        };
        if !status.success() {
            return Err(Error::ToolFailed(
                program,
                crate::i18n::tf("tool_exited", &[("status", &status)]),
            ));
        }
        return Ok(());
    }
    Err(Error::Tool(
        programs[programs.len() - 1],
        std::io::Error::from(std::io::ErrorKind::NotFound),
    ))
}

/// `winebrowser`'s own lists, in its order, from Wine 11's
/// `programs/winebrowser`: what a link went to before Raven's value replaced
/// them. `mailto:` links go to the mail programs.
fn fallbacks(uri: &str) -> &'static [&'static str] {
    const BROWSERS: &[&str] = &[
        "xdg-open",
        "/usr/bin/open",
        "firefox",
        "konqueror",
        "mozilla",
        "opera",
        "dillo",
    ];
    const MAILERS: &[&str] = &[
        "/usr/bin/open",
        "xdg-email",
        "mozilla-thunderbird",
        "thunderbird",
        "evolution",
    ];
    if uri
        .get(..7)
        .is_some_and(|s| s.eq_ignore_ascii_case("mailto:"))
    {
        MAILERS
    } else {
        BROWSERS
    }
}

/// Whether `uri` names something a program outside the environment can open:
/// it has a scheme, and the scheme is not `file`.
///
/// A one-letter scheme is a drive letter, not a scheme.
fn for_the_desktop(uri: &str) -> bool {
    let Some((scheme, _)) = uri.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    let well_formed = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    well_formed && scheme.len() > 1 && !scheme.eq_ignore_ascii_case("file")
}

fn portal_via_busctl(uri: &str) -> bool {
    quietly(Command::new("busctl").args([
        "--user",
        "call",
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.OpenURI",
        "OpenURI",
        "ssa{sv}",
        "",
        uri,
        "0",
    ]))
}

fn portal_via_gdbus(uri: &str) -> bool {
    quietly(Command::new("gdbus").args([
        "call",
        "--session",
        "--dest",
        "org.freedesktop.portal.Desktop",
        "--object-path",
        "/org/freedesktop/portal/desktop",
        "--method",
        "org.freedesktop.portal.OpenURI.OpenURI",
        "",
        uri,
        "{}",
    ]))
}

/// Runs `cmd` with its output discarded, and says whether it succeeded. A tool
/// that is missing is a failure like any other: the next one is tried.
fn quietly(cmd: &mut Command) -> bool {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Makes `winebrowser` in `prefix` hand links to `helper`, the Raven binary.
///
/// Run at every session start, while no wineserver holds the registry, so an
/// environment created before this existed receives it, and a Raven that moved
/// is followed. A value the user set themselves is left alone; only an absent
/// one, or one Raven wrote, is (re)written.
pub fn reconcile(prefix: &Path, helper: &Path) -> Result<(), Error> {
    if is_the_window(helper) {
        return Ok(());
    }
    // A quote cannot be one quoted argument in the registry's text, and
    // winebrowser splits its list at every comma, quoted or not. Wine's own
    // default stays.
    let Some(helper) = helper.to_str().filter(|h| !h.contains(['"', ','])) else {
        return Ok(());
    };
    let reg = prefix.join("user.reg");
    let old = std::fs::read_to_string(&reg).map_err(|e| Error::Layer(reg.clone(), e))?;
    let new = point_at(&old, helper);
    if new != old {
        text::write_atomic(&reg, &new)?;
    }
    Ok(())
}

/// Whether `helper` is the window rather than the CLI.
///
/// The window answers neither `open-uri` nor `run`. One that had to run its
/// own anchor, with no CLI beside it, would name itself, refuse the verb, and
/// the link would go nowhere. The CLI may have any name: Colony keeps the
/// release asset as `raven-linux`.
fn is_the_window(helper: &Path) -> bool {
    helper
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("raven-gui"))
}

/// `text` with `Browsers` and `Mailers` naming `helper`, wherever they are
/// absent or already Raven's.
fn point_at(text: &str, helper: &str) -> String {
    // A command line, as `winebrowser` splits it: the path is quoted so a space
    // in it survives, then escaped for the registry file.
    let ours = crate::registry::emit::escape(&format!("\"{helper}\" {VERB}"));
    let mut out = text.to_string();
    for key in ["Browsers", "Mailers"] {
        let lines = lines_setting(text, key);
        let ours_line = format!("\"{key}\"=\"{ours}\"");
        // Raven's is one plain string ending in the verb. Anything else - a
        // list the user typed, a multi-string, a second spelling of the key -
        // is the user's, and Wine would honour it.
        let raven_s = match lines.as_slice() {
            [] => true,
            [line] => {
                line.starts_with(&format!("\"{key}\"=\""))
                    && line.trim_end().ends_with(&format!(" {VERB}\""))
            }
            _ => false,
        };
        if raven_s && lines.as_slice() != [ours_line.as_str()] {
            out = text::set_value(&out, SECTION, key, Some(&ours));
        }
    }
    out
}

/// Every line under `SECTION` that sets `key`, whatever the value's type,
/// matching the section and the name without case, as Wine does when it loads
/// them.
fn lines_setting<'a>(text: &'a str, key: &str) -> Vec<&'a str> {
    let header = format!("[{SECTION}]").to_ascii_lowercase();
    let name = format!("\"{key}\"=").to_ascii_lowercase();
    let mut inside = false;
    text.lines()
        .filter(|line| {
            if line.starts_with('[') {
                inside = line.to_ascii_lowercase().starts_with(&header);
                return false;
            }
            inside && line.to_ascii_lowercase().starts_with(&name)
        })
        .collect()
}

/// Makes the desktop entries Wine wrote for the environment `name`, whose
/// prefix is `prefix`, start through `helper`, the Raven binary. Returns how
/// many it changed.
///
/// `winemenubuilder` gives each entry `env "WINEPREFIX=<prefix>" wine …` as
/// its command. Started from the desktop, that runs outside the environment:
/// C: there is the empty mount point, and Wine, finding no session it can
/// join, starts a second wineserver on the same prefix. A sign-in link coming
/// back from the browser through a program's own scheme went that way.
/// Rewritten, the entry runs `raven run <name>`, and the working directory
/// Wine gave it, which exists only inside, moves to `--cwd`.
///
/// With `seen`, an entry is read once it has stayed the same for a whole
/// pass and not again until it changes, so a session can call this every few
/// seconds: Wine may still be writing an entry it has just created, and it
/// marks one on the desktop executable a moment after closing it. Without,
/// every entry is read now. Best effort: an entry that cannot be read or
/// written is left as it was.
pub fn reroute(prefix: &Path, name: &str, helper: &Path, seen: Option<&mut Seen>) -> usize {
    let (Some(prefix), Some(cli)) = (prefix.to_str(), helper.to_str()) else {
        return 0;
    };
    if is_the_window(helper) {
        return 0;
    }
    let Ok(applications) = crate::paths::applications_dir() else {
        return 0;
    };
    let desktop = crate::paths::desktop_dir().ok();
    rewrite(
        wine_entries(&applications, desktop.as_deref()),
        prefix,
        name,
        cli,
        seen,
    )
}

/// What [`reroute`] last saw of each entry, and whether it has dealt with it
/// as it was then.
pub type Seen = std::collections::HashMap<PathBuf, (Stamp, bool)>;

/// An entry's modification time, size and mode. Compared for equality only,
/// never against the clock, so a home on a network filesystem whose server
/// runs behind, or a desktop on vfat, is read when it should be.
type Stamp = (Option<SystemTime>, u64, u32);

fn stamp(file: &Path) -> Option<Stamp> {
    use std::os::unix::fs::PermissionsExt;
    let m = std::fs::metadata(file).ok()?;
    Some((m.modified().ok(), m.len(), m.permissions().mode()))
}

/// [`reroute`] over `files`.
fn rewrite(
    files: Vec<PathBuf>,
    prefix: &str,
    name: &str,
    cli: &str,
    mut seen: Option<&mut Seen>,
) -> usize {
    let mut changed = 0;
    for file in files {
        let Some(before) = stamp(&file) else {
            continue;
        };
        if let Some(seen) = seen.as_mut() {
            match seen.get(&file) {
                Some((s, true)) if *s == before => continue,
                Some((s, false)) if *s == before => {}
                _ => {
                    seen.insert(file, (before, false));
                    continue;
                }
            }
        }
        let outcome = rewrite_one(&file, &before, prefix, name, cli);
        if outcome == Some(true) {
            changed += 1;
        }
        if let (Some(seen), Some(after)) = (seen.as_mut(), stamp(&file)) {
            // An entry that changed while it was read is looked at again.
            seen.insert(file, (after, outcome.is_some()));
        }
    }
    changed
}

/// Rewrites one entry. `Some(changed)`, or `None` when it changed while it
/// was being read.
fn rewrite_one(file: &Path, before: &Stamp, prefix: &str, name: &str, cli: &str) -> Option<bool> {
    use std::os::unix::fs::PermissionsExt;
    let Ok(text) = std::fs::read_to_string(file) else {
        return Some(false);
    };
    let Some(new) = rerouted(&text, prefix, name, cli) else {
        return Some(false);
    };
    // Wine writes an entry in place. Had it written this one again since it
    // was read, replacing the file would throw its version away.
    if stamp(file).as_ref() != Some(before) {
        return None;
    }
    if text::write_atomic(file, &new).is_err() {
        return Some(false);
    }
    // A launcher on the desktop has to stay executable to be trusted.
    let _ = std::fs::set_permissions(file, std::fs::Permissions::from_mode(before.2));
    Some(true)
}

/// Every desktop entry Wine may have written: `wine-*.desktop` and the
/// `wine/` menu tree in `applications`, and the entries on the desktop.
fn wine_entries(applications: &Path, desktop: Option<&Path>) -> Vec<PathBuf> {
    fn entries(dir: &Path, deep: bool, keep: &dyn Fn(&str) -> bool, out: &mut Vec<PathBuf>) {
        let Ok(list) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in list.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() && deep {
                entries(&path, deep, keep, out);
            } else if kind.is_file()
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(".desktop") && keep(n))
            {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    entries(applications, false, &|n| n.starts_with("wine-"), &mut out);
    entries(&applications.join("wine"), true, &|_| true, &mut out);
    if let Some(desktop) = desktop {
        entries(desktop, false, &|_| true, &mut out);
    }
    out
}

/// `text` with its command going through `cli`, if it is an entry Wine wrote
/// for `prefix`, or one Raven rewrote for `name` naming another binary.
fn rerouted(text: &str, prefix: &str, name: &str, cli: &str) -> Option<String> {
    let wine = format!("Exec=env \"WINEPREFIX={prefix}\" wine ");
    let ours = format!(" run {} ", quoted(name));
    let (old, exec, from_wine) = text.lines().find_map(|line| {
        if let Some(command) = line.strip_prefix(&wine) {
            let mut exec = format!("Exec={} run {}", quoted(cli), quoted(name));
            if let Some(cwd) = text.lines().find_map(|l| l.strip_prefix("Path=")) {
                exec.push_str(&format!(" --cwd {}", quoted(cwd)));
            }
            exec.push_str(&format!(" -- wine {command}"));
            return Some((line, exec, true));
        }
        // Rewritten by a Raven that has since moved, or by a build that was
        // only being tried: the binary it names may be gone.
        let program = line.strip_prefix("Exec=\"")?;
        let at = program.find(&ours)?;
        if !program[..at].ends_with('"') {
            return None;
        }
        let exec = format!("Exec={}{}", quoted(cli), &program[at..]);
        (exec != line).then_some((line, exec, false))
    })?;
    let mut out = String::with_capacity(text.len() + exec.len());
    let mut replaced = false;
    for line in text.lines() {
        if !replaced && line == old {
            out.push_str(&exec);
            replaced = true;
        } else if from_wine && line.starts_with("Path=") {
            // The launcher would change into it before starting Raven, and
            // outside the environment it does not exist.
            continue;
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    Some(out)
}

/// `arg` as one quoted argument of a desktop entry's `Exec`.
///
/// The value is unescaped as a string first and split into arguments second,
/// so a backslash takes four, and `"`, `` ` `` and `$` take two. `%` would
/// start a field code.
fn quoted(arg: &str) -> String {
    let mut out = String::from("\"");
    for c in arg.chars() {
        match c {
            '"' | '`' | '$' => {
                out.push_str("\\\\");
                out.push(c);
            }
            '\\' => out.push_str("\\\\\\\\"),
            '%' => out.push_str("%%"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const REG: &str = "WINE REGISTRY Version 2\n\n[Software\\\\Fluff] 100\n\"x\"=\"y\"\n";

    #[test]
    fn web_and_mail_links_go_to_the_desktop() {
        for uri in [
            "https://cfx.re/sign-in?x=1",
            "http://example.com",
            "mailto:someone@example.com",
            "steam://run/271590",
        ] {
            assert!(for_the_desktop(uri), "{uri}");
        }
    }

    #[test]
    fn files_and_paths_stay_inside_the_environment() {
        for uri in [
            "file:///run/user/1000/raven/games/c/readme.txt",
            "FILE:///tmp/x",
            "/home/user/readme.txt",
            "C:\\readme.txt",
            "",
            "1http://x",
        ] {
            assert!(!for_the_desktop(uri), "{uri}");
        }
    }

    #[test]
    fn an_absent_setting_is_written() {
        let out = point_at(REG, "/usr/bin/raven");
        assert!(
            out.contains("\"Browsers\"=\"\\\"/usr/bin/raven\\\" open-uri\""),
            "got:\n{out}"
        );
        assert!(out.contains("\"Mailers\"=\"\\\"/usr/bin/raven\\\" open-uri\""));
        assert!(out.contains("\"x\"=\"y\""), "the rest is untouched");
    }

    #[test]
    fn a_raven_that_moved_is_followed() {
        let before = point_at(REG, "/home/user/.local/bin/raven");
        let after = point_at(&before, "/usr/bin/raven");
        assert!(after.contains("\\\"/usr/bin/raven\\\" open-uri"));
        assert!(!after.contains(".local/bin"), "got:\n{after}");
    }

    #[test]
    fn the_users_own_choice_is_kept() {
        let mine = format!("{REG}\n[{SECTION}] 200\n\"Browsers\"=\"firefox\"\n");
        let out = point_at(&mine, "/usr/bin/raven");
        assert!(out.contains("\"Browsers\"=\"firefox\""), "got:\n{out}");
        assert!(
            out.contains("\"Mailers\"=\"\\\"/usr/bin/raven\\\" open-uri\""),
            "the other value is still Raven's to set"
        );
    }

    #[test]
    fn a_value_of_the_users_in_another_form_is_kept() {
        // A multi-string, which winebrowser reads as a list, and the key or
        // the section spelled in another case, which Wine merges at load.
        for mine in [
            format!("{REG}\n[{SECTION}] 200\n\"Browsers\"=str(7):\"firefox\\0chromium\\0\"\n"),
            format!("{REG}\n[{SECTION}] 200\n\"Browsers\"=hex(7):66,00,00,00,\\\n  00,00\n"),
            format!("{REG}\n[{SECTION}] 200\n\"browsers\"=\"firefox\"\n"),
            format!("{REG}\n[Software\\\\Wine\\\\winebrowser] 200\n\"Browsers\"=\"firefox\"\n"),
        ] {
            let out = point_at(&mine, "/usr/bin/raven");
            assert!(
                !out.contains("\"Browsers\"=\"\\\"/usr/bin/raven"),
                "overwrote:\n{mine}\ngot:\n{out}"
            );
            assert!(out.contains("\"Mailers\"=\"\\\"/usr/bin/raven\\\" open-uri\""));
        }
    }

    #[test]
    fn a_path_wine_cannot_hold_or_a_window_is_left_to_wine() {
        let dir = std::env::temp_dir().join(format!("raven-desktop-c-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("user.reg"), REG).unwrap();
        for helper in ["/opt/a,b/raven", "/usr/bin/raven-gui", "/x/raven-gui-linux"] {
            reconcile(&dir, Path::new(helper)).unwrap();
        }
        let after = std::fs::read_to_string(dir.join("user.reg")).unwrap();
        // The CLI under the name Colony gives the release asset still counts.
        reconcile(&dir, Path::new("/apps/Raven/raven-linux")).unwrap();
        let colony = std::fs::read_to_string(dir.join("user.reg")).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(after, REG);
        assert!(
            colony.contains("raven-linux\\\" open-uri"),
            "got:\n{colony}"
        );
    }

    #[test]
    fn without_a_portal_a_link_goes_where_winebrowser_sent_it() {
        assert_eq!(fallbacks("https://example.com")[0], "xdg-open");
        assert_eq!(fallbacks("MAILTO:a@b.c")[1], "xdg-email");
    }

    #[test]
    fn reconciling_twice_changes_nothing() {
        let once = point_at(REG, "/usr/bin/raven");
        assert_eq!(point_at(&once, "/usr/bin/raven"), once);
    }

    #[test]
    fn a_backslash_in_the_path_is_escaped() {
        let out = point_at(REG, "/opt/we\\ird/raven");
        assert!(
            out.contains("\"Browsers\"=\"\\\"/opt/we\\\\ird/raven\\\" open-uri\""),
            "got:\n{out}"
        );
    }

    #[test]
    fn a_path_with_a_quote_is_left_to_wine() {
        let dir = std::env::temp_dir().join(format!("raven-desktop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("user.reg"), REG).unwrap();
        reconcile(&dir, Path::new("/opt/we\"ird/raven")).unwrap();
        let after = std::fs::read_to_string(dir.join("user.reg")).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(after, REG);
    }

    const PREFIX: &str = "/home/u/.local/share/Colony/Raven/environments/unreal/prefix";

    /// A menu entry as winemenubuilder writes it for a Raven environment.
    fn menu_entry() -> String {
        format!(
            "[Desktop Entry]\nName=Epic Games Launcher\nExec=env \"WINEPREFIX={PREFIX}\" wine \"C:\\\\\\\\users\\\\\\\\u\\\\\\\\Epic Games Launcher.lnk\"\nType=Application\nPath=/run/user/1000/raven/unreal/c/Program Files/Epic Games/\nIcon=F2A9_EpicGamesLauncher.0\n"
        )
    }

    #[test]
    fn a_menu_entry_starts_through_raven_in_its_own_directory() {
        let out = rerouted(&menu_entry(), PREFIX, "unreal", "/usr/bin/raven").unwrap();
        assert_eq!(
            out,
            "[Desktop Entry]\nName=Epic Games Launcher\nExec=\"/usr/bin/raven\" run \"unreal\" --cwd \"/run/user/1000/raven/unreal/c/Program Files/Epic Games/\" -- wine \"C:\\\\\\\\users\\\\\\\\u\\\\\\\\Epic Games Launcher.lnk\"\nType=Application\nIcon=F2A9_EpicGamesLauncher.0\n"
        );
    }

    #[test]
    fn a_link_scheme_handler_keeps_its_field_code() {
        let entry = format!(
            "[Desktop Entry]\nType=Application\nMimeType=x-scheme-handler/com.epicgames.launcher;\nExec=env \"WINEPREFIX={PREFIX}\" wine start %u\nNoDisplay=true\n"
        );
        let out = rerouted(&entry, PREFIX, "unreal", "/usr/bin/raven").unwrap();
        assert!(
            out.contains("\nExec=\"/usr/bin/raven\" run \"unreal\" -- wine start %u\n"),
            "{out}"
        );
        assert!(out.contains("MimeType=x-scheme-handler/com.epicgames.launcher;\n"));
    }

    #[test]
    fn entries_for_any_other_prefix_and_rerouted_ones_are_left_alone() {
        let other = menu_entry().replace("environments/unreal", "environments/games");
        assert_eq!(rerouted(&other, PREFIX, "unreal", "/usr/bin/raven"), None);
        // A prefix whose path merely starts like this one is another one.
        let longer = menu_entry().replace("/prefix\"", "/prefix2\"");
        assert_eq!(rerouted(&longer, PREFIX, "unreal", "/usr/bin/raven"), None);
        let done = rerouted(&menu_entry(), PREFIX, "unreal", "/usr/bin/raven").unwrap();
        assert_eq!(rerouted(&done, PREFIX, "unreal", "/usr/bin/raven"), None);
        assert_eq!(
            rerouted("[Desktop Entry]\nExec=wine x\n", PREFIX, "unreal", "/r"),
            None
        );
    }

    #[test]
    fn an_argument_survives_the_entrys_two_rounds_of_unescaping() {
        assert_eq!(quoted("/opt/My Raven/raven"), "\"/opt/My Raven/raven\"");
        assert_eq!(quoted("a\"b"), "\"a\\\\\"b\"");
        assert_eq!(quoted("$HOME`x`"), "\"\\\\$HOME\\\\`x\\\\`\"");
        assert_eq!(quoted("C:\\x"), "\"C:\\\\\\\\x\"");
        assert_eq!(quoted("100%"), "\"100%%\"");
    }

    #[test]
    fn only_what_wine_writes_is_looked_at() {
        let root = std::env::temp_dir().join(format!("raven-entries-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let apps = root.join("applications");
        let desk = root.join("Bureau");
        for d in [apps.join("wine/Programs/Epic Games"), desk.clone()] {
            std::fs::create_dir_all(d).unwrap();
        }
        for f in [
            apps.join("wine-protocol-x.desktop"),
            apps.join("firefox.desktop"),
            apps.join("wine-protocol-x.desktop.raven-tmp"),
            apps.join("wine/Programs/Epic Games/Launcher.desktop"),
            desk.join("Launcher.desktop"),
            desk.join("notes.txt"),
        ] {
            std::fs::write(f, "").unwrap();
        }
        let mut found = wine_entries(&apps, Some(&desk));
        found.sort();
        let mut want = vec![
            apps.join("wine-protocol-x.desktop"),
            apps.join("wine/Programs/Epic Games/Launcher.desktop"),
            desk.join("Launcher.desktop"),
        ];
        want.sort();
        assert_eq!(found, want);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_launcher_on_the_desktop_stays_executable_once_rewritten() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!("raven-reroute-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let launcher = root.join("Launcher.desktop");
        std::fs::write(&launcher, menu_entry()).unwrap();
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755)).unwrap();

        let cli = "/usr/bin/raven";
        assert_eq!(
            rewrite(vec![launcher.clone()], PREFIX, "unreal", cli, None),
            1
        );
        let text = std::fs::read_to_string(&launcher).unwrap();
        assert!(
            text.contains("Exec=\"/usr/bin/raven\" run \"unreal\" --cwd"),
            "{text}"
        );
        let mode = std::fs::metadata(&launcher).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        assert_eq!(rewrite(vec![launcher], PREFIX, "unreal", cli, None), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn during_a_session_an_entry_is_read_once_it_has_settled() {
        let root = std::env::temp_dir().join(format!("raven-settle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let entry = root.join("wine-protocol-x.desktop");
        std::fs::write(&entry, menu_entry()).unwrap();
        let cli = "/usr/bin/raven";
        let mut seen = Seen::new();

        // Just written: Wine may not be done with it.
        assert_eq!(
            rewrite(vec![entry.clone()], PREFIX, "unreal", cli, Some(&mut seen)),
            0
        );
        // The same a pass later: now it is rewritten.
        assert_eq!(
            rewrite(vec![entry.clone()], PREFIX, "unreal", cli, Some(&mut seen)),
            1
        );
        // And not read again until it changes.
        assert!(seen.get(&entry).is_some_and(|(_, done)| *done));
        assert_eq!(
            rewrite(vec![entry.clone()], PREFIX, "unreal", cli, Some(&mut seen)),
            0
        );

        // Wine writes it again: looked at again once it has settled.
        std::fs::write(&entry, menu_entry() + "Comment=again\n").unwrap();
        assert_eq!(
            rewrite(vec![entry.clone()], PREFIX, "unreal", cli, Some(&mut seen)),
            0
        );
        assert_eq!(
            rewrite(vec![entry], PREFIX, "unreal", cli, Some(&mut seen)),
            1
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_entry_naming_a_raven_that_moved_follows_it() {
        let tried = rerouted(
            &menu_entry(),
            PREFIX,
            "unreal",
            "/tmp/build/target/debug/raven",
        )
        .unwrap();
        let moved = rerouted(&tried, PREFIX, "unreal", "/usr/bin/raven").unwrap();
        assert_eq!(
            moved,
            rerouted(&menu_entry(), PREFIX, "unreal", "/usr/bin/raven").unwrap()
        );
        // Another environment's is not this one's to fix.
        assert_eq!(rerouted(&tried, PREFIX, "games", "/usr/bin/raven"), None);
    }
}
