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
//! So Raven puts itself first in `winebrowser`'s list, and passes the link to
//! the desktop portal (`org.freedesktop.portal.OpenURI`). The portal runs in
//! the user's session and starts their own browser there, as them. D-Bus
//! authenticates by the kernel's view of the caller's uid, which a user
//! namespace does not change, so the call works from inside one.
//!
//! A `file:` link stays with `xdg-open` inside the environment: its path may be
//! under C:, and C: exists only in the environment's mount namespace, so a
//! program outside could not open it.

use std::path::Path;
use std::process::{Command, Stdio};

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
/// goes to `xdg-open` as it did before Raven took it.
pub fn open_uri(uri: &str) -> Result<(), Error> {
    if for_the_desktop(uri) && (portal_via_busctl(uri) || portal_via_gdbus(uri)) {
        return Ok(());
    }
    let status = Command::new("xdg-open")
        .arg(uri)
        .status()
        .map_err(|e| Error::Tool("xdg-open", e))?;
    if !status.success() {
        return Err(Error::ToolFailed(
            "xdg-open",
            crate::i18n::tf("tool_exited", &[("status", &status)]),
        ));
    }
    Ok(())
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
    let Some(helper) = helper.to_str().filter(|h| !h.contains('"')) else {
        // Not representable as one quoted argument in the registry's text;
        // Wine's own default stays.
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

/// `text` with `Browsers` and `Mailers` naming `helper`, wherever they are
/// absent or already Raven's.
fn point_at(text: &str, helper: &str) -> String {
    // A command line, as `winebrowser` splits it: the path is quoted so a space
    // in it survives, then escaped for the registry file.
    let ours = crate::registry::emit::escape(&format!("\"{helper}\" {VERB}"));
    let current = text::values(text, SECTION);
    let mut out = text.to_string();
    for key in ["Browsers", "Mailers"] {
        let set = current.iter().find(|(name, _)| name == key);
        let raven_s = set.is_none_or(|(_, value)| value.ends_with(&format!(" {VERB}")));
        if raven_s && set.is_none_or(|(_, value)| *value != ours) {
            out = text::set_value(&out, SECTION, key, Some(&ours));
        }
    }
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
}
