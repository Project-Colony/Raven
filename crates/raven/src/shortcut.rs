//! Shortcuts: a command and an entry in the applications menu that start one
//! program in an environment, as an installer would leave them.
//!
//! `raven run games -- wine 'C:\Games\MyGame\MyGame.exe'` is a lot to type
//! for a program started every day. A program installed by copying a folder,
//! such as a portable build or a game's own launcher, also leaves no Start
//! menu shortcut for Wine to turn into a menu entry. A shortcut is three
//! files:
//!
//! - the command, a shell script in `~/.local/bin` under the name the user
//!   chose. It runs the program through `raven run`, and writes what the
//!   program prints to `$XDG_RUNTIME_DIR/raven-logs/<command>.log`, keeping
//!   the start before as `.log.1`: started from the menu, a program has no
//!   terminal, and the reason it gives for a crash would otherwise be lost;
//! - the menu entry, `raven-shortcut-<command>.desktop`, which runs the
//!   command;
//! - the program's icon, read out of its `.exe` and kept in Raven's data
//!   directory.
//!
//! Each file says it is Raven's in a line of its own, and a file without that
//! line is never replaced or deleted.

use std::ffi::OsStr;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use crate::env::Environment;
use crate::i18n::{self, Lang};
use crate::icon::{self, Icon};
use crate::{Error, layer, paths};

/// The second line of a command Raven wrote, before the environment's name.
const MARKER: &str = "# raven env shortcut: ";
/// The third line, before the command line it runs.
const RUNS: &str = "# Runs: ";
/// The line of a menu entry Raven wrote, before the command's name.
const ENTRY_KEY: &str = "X-Raven-Shortcut=";
/// Raven's own commands: a shortcut by one of these names would stand in for
/// Raven, and the command falls back to `raven` on `PATH`.
const RESERVED: &[&str] = &["raven", "rvn", "raven-gui", "raven-linux"];
/// Larger than any program whose icon is worth reading the whole file for.
const MAX_PROGRAM: u64 = 512 << 20;
/// How much of a file is read to tell whether it is Raven's.
const HEAD: u64 = 4096;

/// One shortcut.
pub struct Shortcut {
    /// The command's name.
    pub name: String,
    /// The command line it runs, quoted as a shell would need it.
    pub runs: String,
    /// The command's file.
    pub command: PathBuf,
}

/// A shortcut just made.
pub struct Made {
    pub shortcut: Shortcut,
    /// A shortcut of the same name was there before, and this replaced it.
    pub replaced: bool,
    /// The name the menu shows.
    pub title: String,
    /// Whether the program's own icon was read. Without it, the entry shows
    /// Raven's.
    pub icon: bool,
    /// What typing the name runs.
    pub on_path: OnPath,
}

/// What typing a shortcut's name in a shell runs.
#[derive(Debug, PartialEq, Eq)]
pub enum OnPath {
    /// The shortcut.
    Found,
    /// Nothing: its directory is not on `PATH`.
    NotOnPath,
    /// Another program of the same name, earlier on `PATH`.
    Shadowed(PathBuf),
}

/// Where the three files go.
struct Places {
    bin: PathBuf,
    applications: PathBuf,
    icons: PathBuf,
}

impl Places {
    fn user() -> Result<Self, Error> {
        Ok(Places {
            bin: paths::bin_dir()?,
            applications: paths::applications_dir()?,
            icons: paths::data_dir()?.join("icons"),
        })
    }

    fn command(&self, name: &str) -> PathBuf {
        self.bin.join(name)
    }

    fn entry(&self, name: &str) -> PathBuf {
        self.applications
            .join(format!("raven-shortcut-{name}.desktop"))
    }

    fn icon(&self, name: &str) -> PathBuf {
        self.icons.join(format!("{name}.png"))
    }
}

/// What a shortcut is made of.
struct Spec<'a> {
    env: &'a str,
    name: &'a str,
    argv: &'a [String],
    cwd: Option<&'a Path>,
    title: &'a str,
    /// The Raven binary the command runs.
    raven: &'a Path,
    icon: Option<&'a Icon>,
    /// The Windows program, when the command line names one.
    program: Option<&'a str>,
}

/// Makes the shortcut `name`, which runs `argv` in `env` as `raven run` does,
/// or replaces the one of that name Raven made before.
///
/// The menu shows `title`, or else the program's file name without `.exe`.
pub fn make(
    env: &Environment,
    name: &str,
    argv: &[String],
    cwd: Option<&Path>,
    title: Option<&str>,
) -> Result<Made, Error> {
    check(name)?;
    // The name is written into a comment line of the command, which a
    // line break would end.
    if env.name.chars().any(char::is_control) {
        return Err(Error::BadName(env.name.clone()));
    }
    let program = program(argv);
    let icon = program
        .and_then(|p| locate(&roots(env), p))
        .and_then(|file| read_program(&file))
        .and_then(|bytes| icon::from_exe(&bytes));
    let title = match title {
        Some(t) => t.to_owned(),
        None => program.map_or(name, stem).to_owned(),
    };
    let raven = crate::session::helper()?;
    let places = Places::user()?;
    let replaced = write(
        &places,
        &Spec {
            env: &env.name,
            name,
            argv,
            cwd,
            title: &title,
            raven: &raven,
            icon: icon.as_ref(),
            program,
        },
    )?;
    let command = places.command(name);
    Ok(Made {
        on_path: on_path(&command, name, std::env::var_os("PATH").as_deref()),
        shortcut: Shortcut {
            name: name.to_owned(),
            runs: runs(argv),
            command,
        },
        replaced,
        title,
        icon: icon.is_some(),
    })
}

/// The shortcuts Raven made for `env`, by name.
///
/// The environment need not exist any more: shortcuts outlive a destroyed
/// one, and this is how they are found to remove.
pub fn list(env: &str) -> Result<Vec<Shortcut>, Error> {
    Ok(list_in(&Places::user()?, env))
}

/// Removes the shortcut `name` of `env`: the command, its menu entry and its
/// icon.
pub fn remove(env: &str, name: &str) -> Result<Shortcut, Error> {
    remove_in(&Places::user()?, env, name)
}

/// The file the command `name` writes the program's output to.
pub fn log_file(name: &str) -> Result<PathBuf, Error> {
    Ok(paths::logs_dir()?.join(format!("{name}.log")))
}

/// A name that is a plain command: letters, digits, `-`, `_`, `.` and `+`,
/// not starting with `-` or `.`, and not one of Raven's.
fn check(name: &str) -> Result<(), Error> {
    let plain = !name.is_empty()
        && !name.starts_with(['-', '.'])
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'));
    if plain && !RESERVED.contains(&name) {
        Ok(())
    } else {
        Err(Error::BadShortcutName(name.to_owned()))
    }
}

fn write(places: &Places, s: &Spec) -> Result<bool, Error> {
    let command = places.command(s.name);
    let entry = places.entry(s.name);
    let replaced = match owner(&command, is_command) {
        Owner::Nobody => false,
        Owner::Raven => true,
        Owner::Other => return Err(Error::ShortcutTaken(command)),
    };
    if owner(&entry, is_entry) == Owner::Other {
        return Err(Error::ShortcutTaken(entry));
    }
    let icon = places.icon(s.name);
    let icon = match s.icon {
        Some(image) => {
            write_file(&icon, &image.png, 0o644)?;
            Some(icon)
        }
        None => {
            // A program whose icon could not be read this time does not keep
            // the last one's.
            let _ = std::fs::remove_file(&icon);
            None
        }
    };
    write_file(&command, script(s).as_bytes(), 0o755)?;
    write_file(
        &entry,
        desktop_entry(s, &command, icon.as_deref()).as_bytes(),
        0o644,
    )?;
    Ok(replaced)
}

fn list_in(places: &Places, env: &str) -> Vec<Shortcut> {
    let Ok(dir) = std::fs::read_dir(&places.bin) else {
        return Vec::new();
    };
    let mut out: Vec<Shortcut> = dir
        .flatten()
        .filter_map(|entry| {
            let command = entry.path();
            let head = head(&command)?;
            (environment_of(&head)? == env).then(|| Shortcut {
                name: entry.file_name().to_string_lossy().into_owned(),
                runs: runs_of(&head).to_owned(),
                command,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn remove_in(places: &Places, env: &str, name: &str) -> Result<Shortcut, Error> {
    let missing = || Error::NoSuchShortcut {
        env: env.to_owned(),
        name: name.to_owned(),
    };
    check(name).map_err(|_| missing())?;
    let command = places.command(name);
    let head = head(&command)
        .filter(|h| environment_of(h) == Some(env))
        .ok_or_else(missing)?;
    std::fs::remove_file(&command).map_err(|e| Error::ShortcutFile(command.clone(), e))?;
    let entry = places.entry(name);
    if owner(&entry, is_entry) == Owner::Raven {
        let _ = std::fs::remove_file(&entry);
    }
    let _ = std::fs::remove_file(places.icon(name));
    Ok(Shortcut {
        name: name.to_owned(),
        runs: runs_of(&head).to_owned(),
        command,
    })
}

/// The command: run the program through `raven run`, its output to a log.
///
/// The Raven binary is named by its path, and `raven` on `PATH` stands in for
/// it once it has moved. The log keeps one start before the last, so a crash
/// is still on record after the start that follows it.
fn script(s: &Spec) -> String {
    let mut run = format!("exec \"$raven\" run {}", sh(s.env));
    if let Some(cwd) = s.cwd {
        run.push_str(&format!(" --cwd {}", sh(&cwd.to_string_lossy())));
    }
    run.push_str(" --");
    for arg in s.argv {
        run.push(' ');
        run.push_str(&sh(arg));
    }
    run.push_str(" \"$@\"");
    let name = s.name;
    format!(
        "#!/bin/sh\n\
         {MARKER}{env}\n\
         {RUNS}{runs}\n\
         # Made by Raven. Remove it with: raven env shortcut {env_arg} {name} --remove\n\
         raven={raven}\n\
         [ -x \"$raven\" ] || raven=raven\n\
         if [ -n \"$XDG_RUNTIME_DIR\" ] && mkdir -p \"$XDG_RUNTIME_DIR/raven-logs\"; then\n\
         \x20   log=\"$XDG_RUNTIME_DIR/raven-logs/{name}.log\"\n\
         \x20   [ -e \"$log\" ] && mv -f \"$log\" \"$log.1\"\n\
         \x20   [ -t 2 ] && echo \"{name}: output goes to $log\" >&2\n\
         \x20   exec >\"$log\" 2>&1\n\
         fi\n\
         {run}\n",
        env = s.env,
        runs = one_line(&runs(s.argv)),
        env_arg = sh(s.env),
        raven = sh(&s.raven.to_string_lossy()),
    )
}

/// The menu entry: run the command, show the program's icon.
fn desktop_entry(s: &Spec, command: &Path, icon: Option<&Path>) -> String {
    let comment = |lang| {
        i18n::fill(
            i18n::get_in(lang, "shortcut_entry_comment").unwrap_or_default(),
            &[("name", &s.env)],
        )
    };
    let mut out = String::from("[Desktop Entry]\nType=Application\n");
    out.push_str(&format!("Name={}\n", value(s.title)));
    out.push_str(&format!("Comment={}\n", value(&comment(Lang::En))));
    out.push_str(&format!("Comment[fr]={}\n", value(&comment(Lang::Fr))));
    out.push_str(&format!(
        "Exec={}\n",
        crate::desktop::quoted(&command.to_string_lossy())
    ));
    match icon {
        Some(icon) => out.push_str(&format!("Icon={}\n", value(&icon.to_string_lossy()))),
        None => out.push_str("Icon=raven\n"),
    }
    out.push_str("Terminal=false\nStartupNotify=true\n");
    // Wine names a program's windows after its file, in lower case, and a
    // panel matches them to the entry by it.
    if let Some(program) = s.program {
        out.push_str(&format!(
            "StartupWMClass={}\n",
            value(&file_name(program).to_lowercase())
        ));
    }
    out.push_str(&format!("{ENTRY_KEY}{}\n", s.name));
    out
}

/// The first argument after the command that names a `.exe`.
fn program(argv: &[String]) -> Option<&str> {
    argv.iter()
        .skip(1)
        .map(String::as_str)
        .find(|a| a.to_ascii_lowercase().ends_with(".exe"))
}

/// The layers of `env`'s C:, the one whose file wins first.
fn roots(env: &Environment) -> Vec<PathBuf> {
    env.spec()
        .map(|spec| std::iter::once(spec.upper).chain(spec.lower).collect())
        .unwrap_or_default()
}

/// Where `program` is, without the environment mounted: a `C:` path in the
/// first of `roots` that has it, matching each name without case as Wine does,
/// or an absolute Linux path as it is.
fn locate(roots: &[PathBuf], program: &str) -> Option<PathBuf> {
    if let Some(rest) = program
        .strip_prefix(['C', 'c'])
        .and_then(|r| r.strip_prefix(':'))
    {
        let relative = rest.replace('\\', "/");
        return roots
            .iter()
            .map(|root| layer::locate(root, &relative))
            .find(|p| p.is_file());
    }
    let path = Path::new(program);
    (path.is_absolute() && path.is_file()).then(|| path.to_path_buf())
}

fn read_program(file: &Path) -> Option<Vec<u8>> {
    let size = std::fs::metadata(file).ok()?.len();
    (size <= MAX_PROGRAM)
        .then(|| std::fs::read(file).ok())
        .flatten()
}

/// The last component of a Windows or Linux path.
fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// The file name without its extension.
fn stem(path: &str) -> &str {
    let name = file_name(path);
    match name.rfind('.') {
        Some(dot) if dot > 0 => &name[..dot],
        _ => name,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Owner {
    Nobody,
    Raven,
    Other,
}

/// Whose `file` is, by `ours` on its first bytes. Anything not a regular
/// file, or not readable, is someone else's.
fn owner(file: &Path, ours: fn(&str) -> bool) -> Owner {
    match std::fs::symlink_metadata(file) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Owner::Nobody,
        Ok(_) if head(file).is_some_and(|h| ours(&h)) => Owner::Raven,
        _ => Owner::Other,
    }
}

fn is_command(head: &str) -> bool {
    environment_of(head).is_some()
}

fn is_entry(head: &str) -> bool {
    head.lines().any(|l| l.starts_with(ENTRY_KEY))
}

/// The start of `file`, if it is a regular file and not a link to one.
fn head(file: &Path) -> Option<String> {
    if !std::fs::symlink_metadata(file).ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(file)
        .ok()?
        .take(HEAD)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The environment a command Raven wrote runs in.
fn environment_of(head: &str) -> Option<&str> {
    let mut lines = head.lines();
    (lines.next()? == "#!/bin/sh")
        .then(|| lines.next()?.strip_prefix(MARKER))
        .flatten()
}

fn runs_of(head: &str) -> &str {
    head.lines()
        .nth(2)
        .and_then(|l| l.strip_prefix(RUNS))
        .unwrap_or_default()
}

/// `argv` as a shell command line.
fn runs(argv: &[String]) -> String {
    argv.iter().map(|a| sh(a)).collect::<Vec<_>>().join(" ")
}

/// `arg` as one shell word: as it is when nothing in it is special, single
/// quoted otherwise.
fn sh(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@%+,".contains(c));
    if plain {
        arg.to_owned()
    } else {
        format!("'{}'", arg.replace('\'', r"'\''"))
    }
}

/// `text` with no line breaks, for a comment line.
fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

/// `text` as a desktop entry value: backslash and the escapes it has, and
/// nothing else that would end the line.
fn value(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push('?'),
            c => out.push(c),
        }
    }
    out
}

/// What typing `name` runs, with `PATH` as given.
fn on_path(command: &Path, name: &str, path: Option<&OsStr>) -> OnPath {
    use std::os::unix::fs::PermissionsExt;
    let ours = std::fs::canonicalize(command).ok();
    for dir in std::env::split_paths(path.unwrap_or_default()) {
        // An empty or relative entry means a directory that depends on where
        // the shell is, which no answer here would hold for.
        if !dir.is_absolute() {
            continue;
        }
        let candidate = dir.join(name);
        let runnable = std::fs::metadata(&candidate)
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
        if runnable {
            return if std::fs::canonicalize(&candidate).ok() == ours {
                OnPath::Found
            } else {
                OnPath::Shadowed(candidate)
            };
        }
    }
    OnPath::NotOnPath
}

/// Writes `file` whole, or leaves what was there: a temporary file beside it,
/// renamed over it.
fn write_file(file: &Path, contents: &[u8], mode: u32) -> Result<(), Error> {
    use std::os::unix::fs::OpenOptionsExt;
    let fail = |e| Error::ShortcutFile(file.to_path_buf(), e);
    let dir = file.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(fail)?;
    let mut name = std::ffi::OsString::from(".");
    name.push(file.file_name().unwrap_or_default());
    name.push(".raven-tmp");
    let tmp = dir.join(name);
    let _ = std::fs::remove_file(&tmp);
    let result = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&tmp)?;
        f.write_all(contents)?;
        f.sync_all()?;
        std::fs::rename(&tmp, file)
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(fail(e));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// A directory of its own for one test, emptied first.
    fn scratch(test: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("raven-shortcut-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn places(dir: &Path) -> Places {
        Places {
            bin: dir.join("bin"),
            applications: dir.join("applications"),
            icons: dir.join("icons"),
        }
    }

    /// A stand-in for Raven that prints each argument it gets on a line of
    /// its own, and what it printed to stderr goes to the log too.
    fn fake_raven(dir: &Path) -> PathBuf {
        let raven = dir.join("fake raven");
        std::fs::write(
            &raven,
            "#!/bin/sh\nfor a in \"$@\"; do printf '[%s]\\n' \"$a\"; done\necho complaint >&2\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&raven, std::fs::Permissions::from_mode(0o755)).unwrap();
        raven
    }

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    fn spec<'a>(env: &'a str, name: &'a str, argv: &'a [String], raven: &'a Path) -> Spec<'a> {
        Spec {
            env,
            name,
            argv,
            cwd: None,
            title: "MyGame",
            raven,
            icon: None,
            program: program(argv),
        }
    }

    /// Runs the command `name` in `places` with `args`, with `runtime` as
    /// `XDG_RUNTIME_DIR`, and returns its log.
    fn run(places: &Places, name: &str, runtime: &Path, args: &[&str]) -> String {
        let status = Command::new(places.command(name))
            .args(args)
            .env("XDG_RUNTIME_DIR", runtime)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::read_to_string(runtime.join("raven-logs").join(format!("{name}.log"))).unwrap()
    }

    #[test]
    fn the_command_runs_raven_with_every_argument_intact() {
        let dir = scratch("args");
        let places = places(&dir);
        let raven = fake_raven(&dir);
        let args = argv(&["wine", r"C:\Games\It's $HOME\game.exe", "-x y", "*"]);
        let mut s = spec("my games", "game", &args, &raven);
        s.cwd = Some(Path::new("/run/user/1000/raven/my games/c/Games"));
        write(&places, &s).unwrap();

        let log = run(&places, "game", &dir, &["--server=10.0.0.1:27015"]);
        assert_eq!(
            log,
            "[run]\n[my games]\n[--cwd]\n[/run/user/1000/raven/my games/c/Games]\n[--]\n\
             [wine]\n[C:\\Games\\It's $HOME\\game.exe]\n[-x y]\n[*]\n\
             [--server=10.0.0.1:27015]\ncomplaint\n",
            "stdout and stderr both reach the log"
        );
    }

    #[test]
    fn each_start_keeps_the_one_before() {
        let dir = scratch("log");
        let places = places(&dir);
        let raven = fake_raven(&dir);
        let args = argv(&["wine", "game.exe"]);
        write(&places, &spec("games", "game", &args, &raven)).unwrap();
        run(&places, "game", &dir, &["first"]);
        run(&places, "game", &dir, &["second"]);
        let logs = dir.join("raven-logs");
        assert!(
            std::fs::read_to_string(logs.join("game.log"))
                .unwrap()
                .contains("[second]")
        );
        assert!(
            std::fs::read_to_string(logs.join("game.log.1"))
                .unwrap()
                .contains("[first]")
        );
    }

    #[test]
    fn a_raven_that_moved_is_found_on_path() {
        let dir = scratch("moved");
        let places = places(&dir);
        let args = argv(&["wine", "game.exe"]);
        write(
            &places,
            &spec("games", "game", &args, &dir.join("gone/raven")),
        )
        .unwrap();
        let path_dir = dir.join("path");
        std::fs::create_dir_all(&path_dir).unwrap();
        std::fs::rename(fake_raven(&dir), path_dir.join("raven")).unwrap();
        let status = Command::new(places.command("game"))
            .env("XDG_RUNTIME_DIR", &dir)
            .env("PATH", format!("{}:/usr/bin:/bin", path_dir.display()))
            .status()
            .unwrap();
        assert!(status.success());
        let log = std::fs::read_to_string(dir.join("raven-logs/game.log")).unwrap();
        assert!(log.starts_with("[run]\n[games]\n"), "{log}");
    }

    #[test]
    fn the_menu_entry_runs_the_command_with_the_programs_icon() {
        let dir = scratch("entry");
        let places = places(&dir);
        let args = argv(&["wine", r"C:\Games\MyGame\MyGame.exe"]);
        let image = crate::icon::from_exe(&crate::icon::tests::exe_with_icon(&[(
            0,
            &crate::icon::png(2, 2, &[9; 16]),
        )]))
        .unwrap();
        let raven = dir.join("raven");
        let mut s = spec("games", "mygame", &args, &raven);
        s.icon = Some(&image);
        write(&places, &s).unwrap();

        let entry = std::fs::read_to_string(places.entry("mygame")).unwrap();
        let icon = places.icon("mygame");
        assert!(entry.contains("\nName=MyGame\n"), "{entry}");
        assert!(
            entry.contains(&format!(
                "\nExec=\"{}\"\n",
                places.command("mygame").display()
            )),
            "{entry}"
        );
        assert!(
            entry.contains(&format!("\nIcon={}\n", icon.display())),
            "{entry}"
        );
        assert!(entry.contains("\nStartupWMClass=mygame.exe\n"), "{entry}");
        assert!(
            entry.contains("\nComment=Runs in the Raven environment games\n"),
            "{entry}"
        );
        assert_eq!(std::fs::read(icon).unwrap(), image.png);

        // Made again without an icon, it shows Raven's, and the old one goes.
        s.icon = None;
        assert!(write(&places, &s).unwrap(), "the second time replaces");
        let entry = std::fs::read_to_string(places.entry("mygame")).unwrap();
        assert!(entry.contains("\nIcon=raven\n"), "{entry}");
        assert!(!places.icon("mygame").exists());
    }

    #[test]
    fn a_file_raven_did_not_write_is_left_alone() {
        let dir = scratch("taken");
        let places = places(&dir);
        let args = argv(&["wine", "game.exe"]);
        let raven = dir.join("raven");
        std::fs::create_dir_all(&places.bin).unwrap();
        std::fs::write(places.command("game"), "#!/bin/sh\necho mine\n").unwrap();
        assert!(matches!(
            write(&places, &spec("games", "game", &args, &raven)),
            Err(Error::ShortcutTaken(p)) if p == places.command("game")
        ));
        assert_eq!(
            std::fs::read_to_string(places.command("game")).unwrap(),
            "#!/bin/sh\necho mine\n"
        );
        assert!(remove_in(&places, "games", "game").is_err());
        assert!(places.command("game").exists(), "not removed either");

        // A menu entry of that name that is not Raven's stops it too.
        std::fs::create_dir_all(&places.applications).unwrap();
        std::fs::write(places.entry("other"), "[Desktop Entry]\nName=Mine\n").unwrap();
        assert!(matches!(
            write(&places, &spec("games", "other", &args, &raven)),
            Err(Error::ShortcutTaken(p)) if p == places.entry("other")
        ));
        assert!(!places.command("other").exists(), "nothing half made");
    }

    #[test]
    fn shortcuts_are_listed_and_removed_by_environment() {
        let dir = scratch("list");
        let places = places(&dir);
        let raven = dir.join("raven");
        let a = argv(&["wine", r"C:\A\a.exe"]);
        let b = argv(&["winecfg"]);
        write(&places, &spec("games", "zeta", &a, &raven)).unwrap();
        write(&places, &spec("games", "alpha", &b, &raven)).unwrap();
        write(&places, &spec("work", "office", &b, &raven)).unwrap();
        std::fs::write(places.bin.join("unrelated"), "#!/bin/sh\n").unwrap();

        let games = list_in(&places, "games");
        let names: Vec<&str> = games.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["alpha", "zeta"]);
        assert_eq!(games[1].runs, r"wine 'C:\A\a.exe'");

        assert!(matches!(
            remove_in(&places, "games", "office"),
            Err(Error::NoSuchShortcut { .. })
        ));
        let gone = remove_in(&places, "games", "zeta").unwrap();
        assert_eq!(gone.name, "zeta");
        assert!(!places.command("zeta").exists());
        assert!(!places.entry("zeta").exists());
        assert_eq!(list_in(&places, "games").len(), 1);
        assert_eq!(list_in(&places, "work").len(), 1);
    }

    #[test]
    fn a_name_must_be_a_plain_command() {
        for bad in [
            "",
            "-x",
            ".hidden",
            "a/b",
            "../x",
            "with space",
            "raven",
            "rvn",
            "raven-gui",
            "raven-linux",
            "é",
        ] {
            assert!(check(bad).is_err(), "{bad:?}");
        }
        for ok in ["mygame", "gta5", "my-game_2.0", "c++"] {
            assert!(check(ok).is_ok(), "{ok:?}");
        }
    }

    #[test]
    fn the_program_is_found_through_the_layers_without_case() {
        let dir = scratch("locate");
        let (upper, base) = (dir.join("upper"), dir.join("base"));
        std::fs::create_dir_all(upper.join("MyGame")).unwrap();
        std::fs::create_dir_all(base.join("Program Files/Game")).unwrap();
        std::fs::write(upper.join("MyGame/MyGame.exe"), "upper").unwrap();
        std::fs::write(base.join("Program Files/Game/Game.exe"), "base").unwrap();
        let roots = [upper.clone(), base.clone()];
        assert_eq!(
            locate(&roots, r"C:\mygame\MYGAME.EXE"),
            Some(upper.join("MyGame/MyGame.exe"))
        );
        assert_eq!(
            locate(&roots, r"c:\Program Files\Game\Game.exe"),
            Some(base.join("Program Files/Game/Game.exe"))
        );
        assert_eq!(locate(&roots, r"C:\nowhere.exe"), None);
        assert_eq!(
            locate(&roots, r"D:\Game.exe"),
            None,
            "another drive is not C:'s layers"
        );
    }

    #[test]
    fn the_title_and_class_come_from_the_programs_file_name() {
        let args = argv(&["wine", "start", r"C:\Games\MyGame\MyGame.exe", "-arg"]);
        assert_eq!(program(&args), Some(r"C:\Games\MyGame\MyGame.exe"));
        assert_eq!(stem(program(&args).unwrap()), "MyGame");
        assert_eq!(stem("/games/Some.Game.EXE"), "Some.Game");
        assert_eq!(program(&argv(&["winecfg"])), None);
        assert_eq!(
            program(&argv(&["setup.exe"])),
            None,
            "the command itself is not its program"
        );
    }

    #[test]
    fn what_typing_the_name_runs_is_reported() {
        let dir = scratch("path");
        let (mine, other) = (dir.join("mine"), dir.join("other"));
        for d in [&mine, &other] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join("game"), "#!/bin/sh\n").unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(d.join("game"), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        let command = mine.join("game");
        let path = |dirs: &[&Path]| std::env::join_paths(dirs).unwrap();
        assert_eq!(
            on_path(&command, "game", Some(&path(&[&mine, &other]))),
            OnPath::Found
        );
        assert_eq!(
            on_path(&command, "game", Some(&path(&[&other, &mine]))),
            OnPath::Shadowed(other.join("game"))
        );
        assert_eq!(
            on_path(&command, "game", Some(&path(&[&dir]))),
            OnPath::NotOnPath
        );
        assert_eq!(on_path(&command, "game", None), OnPath::NotOnPath);
    }

    #[test]
    fn desktop_values_cannot_break_their_line() {
        assert_eq!(value("a\\b\nc\td"), "a\\\\b\\nc\\td");
        assert_eq!(one_line("a\nb"), "a?b");
        assert_eq!(sh("plain-word_1.0"), "plain-word_1.0");
        assert_eq!(sh("it's"), r"'it'\''s'");
        assert_eq!(sh(""), "''");
    }
}
