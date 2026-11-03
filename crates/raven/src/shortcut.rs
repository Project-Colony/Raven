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
//!   chose. It runs the program through `raven run`, in the program's own
//!   directory when Raven finds it, unless told otherwise. Started from the
//!   menu, or anywhere with no terminal, it writes what the program prints to
//!   `~/.cache/Colony/Raven/logs/<command>.log`, keeping the previous start's
//!   as `.log.1`: the reason a program gives for a crash would otherwise be
//!   lost;
//! - the menu entry, `raven-shortcut-<command>.desktop`, which runs the
//!   command;
//! - the program's icon, read out of its `.exe` and kept in Raven's data
//!   directory.
//!
//! The command and the menu entry each say they are Raven's in a line of
//! their own, and a file without that line is never replaced or deleted. The
//! icon is in Raven's own directory, where nothing else writes.

use std::ffi::OsStr;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use crate::env::Environment;
use crate::i18n::{self, Lang};
use crate::{Error, base::Base, icon, layer, paths};

/// The second line of a command Raven wrote, before the environment's name.
const MARKER: &str = "# raven env shortcut: ";
/// The third line, before the command line it runs.
const RUNS: &str = "# Runs: ";
/// The line of a menu entry Raven wrote, before the command's name. Written
/// near the top, so a long title cannot push it out of what is read.
const ENTRY_KEY: &str = "X-Raven-Shortcut=";
/// Raven's own commands, and Wine's that Raven runs by name: a shortcut by
/// one of these would stand in for them, inside the environment too, where
/// `raven run` would then start the shortcut instead of Wine.
const RESERVED: &[&str] = &[
    "raven",
    "rvn",
    "raven-gui",
    "raven-linux",
    "wine",
    "wineserver",
];
/// Longer than any command anyone types, and short enough that the menu
/// entry's file name stays within what a filesystem allows.
const MAX_NAME: usize = 64;
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
    /// Which icon the menu entry shows.
    pub icon: IconFrom,
    /// Whether typing the name runs it: its directory is on `PATH`.
    pub on_path: bool,
}

/// Where a menu entry's icon comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IconFrom {
    /// The program's own.
    Program,
    /// Raven's: the program has none that could be read.
    Unreadable,
    /// Raven's: the program the command line names is not in the environment,
    /// so it starts where `raven run` starts it, not in its own directory.
    NotFound(String),
    /// Raven's: the command line names no `.exe`.
    NoProgram,
}

/// A shortcut just removed.
pub struct Removed {
    pub shortcut: Shortcut,
    /// The menu entry of that name was someone else's, and was left alone.
    pub entry_kept: Option<PathBuf>,
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
    cwd: Option<&'a str>,
    title: &'a str,
    /// The Raven binary the command runs.
    raven: &'a str,
    /// The program's icon, as PNG.
    icon: Option<&'a [u8]>,
    /// The Windows program, when the command line names one.
    program: Option<&'a str>,
    /// Where the command writes the program's output.
    logs: &'a str,
}

/// Makes the shortcut `name`, which runs `argv` in `env` as `raven run` does,
/// or replaces the one of that name Raven made for `env` before.
///
/// The menu shows `title`, or else the program's file name without `.exe`.
/// Without `cwd`, the program starts in its own directory when it is found in
/// the environment, as Windows starts one from a shortcut.
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
    if loops(argv, name) {
        return Err(Error::ShortcutConflict {
            command: name.to_owned(),
            other: None,
        });
    }
    let places = Places::user()?;
    // The menu entry names both, and a desktop entry is text.
    utf8(&places.command(name))?;
    utf8(&places.icon(name))?;
    // Everything that can refuse is asked before anything is read or written.
    let replaced = claim(&places, &env.name, name)?;
    if let Some(other) = elsewhere_on_path(
        &places.command(name),
        name,
        std::env::var_os("PATH").as_deref(),
    ) {
        return Err(Error::ShortcutConflict {
            command: name.to_owned(),
            other: Some(other),
        });
    }
    let raven = crate::session::helper()?;
    let raven = utf8(&raven)?;
    let logs = paths::logs_dir()?;
    let logs = utf8(&logs)?;

    let program = program(argv);
    let found = program.and_then(|p| locate(&roots(env), p));
    let icon = found
        .as_ref()
        .and_then(|(_, file)| read_program(file))
        .and_then(|bytes| icon::from_exe(&bytes));
    let cwd = match cwd {
        Some(dir) => Some(utf8(dir)?.to_owned()),
        None => found
            .as_ref()
            .and_then(|(root, file)| own_directory(env, root.as_deref(), file)),
    };
    let title = match title {
        Some(t) => t.to_owned(),
        None => program.map_or(name, stem).to_owned(),
    };
    write(
        &places,
        &Spec {
            env: &env.name,
            name,
            argv,
            cwd: cwd.as_deref(),
            title: &title,
            raven,
            icon: icon.as_deref(),
            program,
            logs,
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
        icon: match (program, &found, &icon) {
            (_, _, Some(_)) => IconFrom::Program,
            (_, Some(_), None) => IconFrom::Unreadable,
            (Some(program), None, None) => IconFrom::NotFound(program.to_owned()),
            (None, None, None) => IconFrom::NoProgram,
        },
    })
}

/// Whether the command line `argv` would start the shortcut `name` itself:
/// `raven run` execs its first word by `PATH`, and a path is not looked up.
fn loops(argv: &[String], name: &str) -> bool {
    argv.first().is_some_and(|a| !a.contains('/') && a == name)
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
pub fn remove(env: &str, name: &str) -> Result<Removed, Error> {
    remove_in(&Places::user()?, env, name)
}

/// The file the command `name` writes the program's output to.
pub fn log_file(name: &str) -> Result<PathBuf, Error> {
    Ok(paths::logs_dir()?.join(format!("{name}.log")))
}

/// A name that is a plain command: letters, digits, `-`, `_`, `.` and `+`,
/// not starting with `-` or `.`, at most 64 characters, and not one of
/// Raven's or Wine's.
fn check(name: &str) -> Result<(), Error> {
    let plain = !name.is_empty()
        && name.len() <= MAX_NAME
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

/// Whether the files of `name` may be written for `env`: `Ok(true)` when they
/// are a shortcut Raven made for `env` before, `Ok(false)` when there are
/// none.
fn claim(places: &Places, env: &str, name: &str) -> Result<bool, Error> {
    let command = places.command(name);
    let replaced = match owner(&command, is_command) {
        Owner::Nobody => false,
        Owner::Raven => {
            // Another environment's shortcut is not this one's to take over.
            let theirs = head(&command).and_then(|h| environment_of(&h).map(str::to_owned));
            match theirs {
                Some(theirs) if theirs != env => {
                    return Err(Error::ShortcutElsewhere {
                        command: name.to_owned(),
                        env: theirs,
                    });
                }
                _ => true,
            }
        }
        Owner::Other => return Err(Error::ShortcutTaken(command)),
    };
    let entry = places.entry(name);
    if owner(&entry, is_entry) == Owner::Other {
        return Err(Error::ShortcutTaken(entry));
    }
    Ok(replaced)
}

/// Writes the three files, the menu entry last: until it exists the
/// shortcut is not in the menu, and once it does, what it names is there.
fn write(places: &Places, s: &Spec) -> Result<(), Error> {
    let command = places.command(s.name);
    write_file(&command, script(s).as_bytes(), 0o755)?;
    let icon = places.icon(s.name);
    let icon = match s.icon {
        Some(png) => {
            write_file(&icon, png, 0o644)?;
            Some(icon)
        }
        None => {
            // A program whose icon could not be read this time does not keep
            // the last one's.
            let _ = std::fs::remove_file(&icon);
            None
        }
    };
    write_file(
        &places.entry(s.name),
        desktop_entry(s, &command, icon.as_deref()).as_bytes(),
        0o644,
    )
}

fn list_in(places: &Places, env: &str) -> Vec<Shortcut> {
    let Ok(dir) = std::fs::read_dir(&places.bin) else {
        return Vec::new();
    };
    let mut out: Vec<Shortcut> = dir
        .flatten()
        .filter_map(|entry| {
            // A temporary file left by a write that died is not a shortcut.
            let name = entry.file_name().into_string().ok()?;
            check(&name).ok()?;
            let command = entry.path();
            let head = head(&command)?;
            (environment_of(&head)? == env).then(|| Shortcut {
                name,
                runs: runs_of(&head).to_owned(),
                command,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Removes the menu entry first, then the icon, then the command: the command
/// is what says the shortcut is `env`'s, so a removal that fails half way can
/// be run again.
fn remove_in(places: &Places, env: &str, name: &str) -> Result<Removed, Error> {
    let missing = || Error::NoSuchShortcut {
        env: env.to_owned(),
        command: name.to_owned(),
    };
    check(name).map_err(|_| missing())?;
    let command = places.command(name);
    let head = head(&command)
        .filter(|h| environment_of(h) == Some(env))
        .ok_or_else(missing)?;
    let gone = |file: &Path| match std::fs::remove_file(file) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(Error::ShortcutFile(file.to_path_buf(), e))
        }
        _ => Ok(()),
    };
    let entry = places.entry(name);
    let entry_kept = match owner(&entry, is_entry) {
        Owner::Raven => {
            gone(&entry)?;
            None
        }
        Owner::Nobody => None,
        Owner::Other => Some(entry),
    };
    gone(&places.icon(name))?;
    gone(&command)?;
    Ok(Removed {
        shortcut: Shortcut {
            name: name.to_owned(),
            runs: runs_of(&head).to_owned(),
            command,
        },
        entry_kept,
    })
}

/// The command: run the program through `raven run`, its output to a log
/// when nobody is watching it. A terminal on any stream means someone is,
/// and their own redirections are kept: a menu gives a program `/dev/null`
/// for input and a pipe for output. The directory is private, since what a
/// launcher prints can carry an account's address or a sign-in link.
///
/// The Raven binary is named by its path, and `raven` on `PATH` stands in for
/// it once it has moved. The log keeps one start before the last, so a crash
/// is still on record after the start that follows it.
fn script(s: &Spec) -> String {
    let mut run = format!("exec \"$raven\" run {}", sh(s.env));
    if let Some(cwd) = s.cwd {
        run.push_str(&format!(" --cwd {}", sh(cwd)));
    }
    run.push_str(" --");
    for arg in s.argv {
        run.push(' ');
        run.push_str(&sh(arg));
    }
    run.push_str(" \"$@\"");
    let name = s.name;
    // Two starts at the same moment can rotate each other's log; the second
    // one's is then the one kept, which is the start the user is looking at.
    format!(
        "#!/bin/sh\n\
         {MARKER}{env}\n\
         {RUNS}{runs}\n\
         # Made by Raven. Remove it with: raven env shortcut {env_arg} {name} --remove\n\
         raven={raven}\n\
         [ -x \"$raven\" ] || raven=raven\n\
         if ! [ -t 0 ] && ! [ -t 1 ] && ! [ -t 2 ] && mkdir -p -m 700 {logs}; then\n\
         \x20   log={logs}/{name}.log\n\
         \x20   [ -e \"$log\" ] && mv -f \"$log\" \"$log.1\"\n\
         \x20   exec >\"$log\" 2>&1\n\
         fi\n\
         {run}\n",
        env = s.env,
        runs = one_line(&runs(s.argv)),
        env_arg = sh(s.env),
        raven = sh(s.raven),
        logs = sh(s.logs),
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
    out.push_str(&format!("{ENTRY_KEY}{}\n", s.name));
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
    out
}

/// The first argument after the command that names a `.exe`.
fn program(argv: &[String]) -> Option<&str> {
    argv.iter()
        .skip(1)
        .map(String::as_str)
        .find(|a| a.to_ascii_lowercase().ends_with(".exe"))
}

/// The layers of `env`'s C:, the one whose file wins first. Read from the
/// environment's directories, so no session or runtime directory is needed.
fn roots(env: &Environment) -> Vec<PathBuf> {
    let mut roots = vec![env.upper(), env.layer()];
    roots.extend(Base::find(&env.manifest.base).map(|b| b.path));
    roots
}

/// Where `program` is, without the environment mounted: a `C:` path in the
/// first of `roots` that has it, matching each name without case as Wine does,
/// or an absolute Linux path as it is. Also the layer it was found in, for a
/// `C:` path.
fn locate(roots: &[PathBuf], program: &str) -> Option<(Option<PathBuf>, PathBuf)> {
    if let Some(rest) = program
        .strip_prefix(['C', 'c'])
        .and_then(|r| r.strip_prefix(':'))
    {
        let relative = rest.replace('\\', "/");
        // `..` would climb out of the layer and read the host's files.
        if relative.split('/').any(|part| part == "..") {
            return None;
        }
        return roots.iter().find_map(|root| {
            let file = layer::locate(root, &relative);
            file.is_file().then(|| (Some(root.clone()), file))
        });
    }
    let path = Path::new(program);
    (path.is_absolute() && path.is_file()).then(|| (None, path.to_path_buf()))
}

/// The directory `file` is in, as the program sees it once the environment is
/// mounted: under the mount point for a file in one of C:'s layers, as it is
/// for a Linux path.
fn own_directory(env: &Environment, root: Option<&Path>, file: &Path) -> Option<String> {
    let dir = file.parent()?;
    let dir = match root {
        Some(root) => paths::mount_point(&env.name)
            .ok()?
            .join(dir.strip_prefix(root).ok()?),
        None => dir.to_path_buf(),
    };
    dir.to_str().map(str::to_owned)
}

/// The program's bytes, at most `MAX_PROGRAM` of them. Its size is checked
/// first, so a large installer is not read at all, and the read is bounded
/// too: a file under `/proc` says 0 and never ends.
fn read_program(file: &Path) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(file).ok()?;
    if !meta.is_file() || meta.len() > MAX_PROGRAM {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(file)
        .ok()?
        .take(MAX_PROGRAM + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_PROGRAM).then_some(bytes)
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

/// `path` as text, which the command's script and the menu entry need.
fn utf8(path: &Path) -> Result<&str, Error> {
    path.to_str()
        .ok_or_else(|| Error::NotUtf8(path.to_path_buf()))
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

/// The executable files named `name` in `PATH`'s directories.
fn runnable_on_path<'a>(name: &'a str, path: Option<&OsStr>) -> impl Iterator<Item = PathBuf> + 'a {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(path.unwrap_or_default())
        .collect::<Vec<_>>()
        .into_iter()
        // An empty or relative entry means a directory that depends on where
        // the shell is, which no answer here would hold for.
        .filter(|dir| dir.is_absolute())
        .map(move |dir| dir.join(name))
        .filter(|candidate| {
            std::fs::metadata(candidate)
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

/// Another program named `name` anywhere on `PATH`: before the command it
/// would hide it, after it the command would hide that program, from the user
/// and from whatever runs it by name.
fn elsewhere_on_path(command: &Path, name: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    let ours = std::fs::canonicalize(command).ok();
    runnable_on_path(name, path)
        .find(|candidate| ours.is_none() || std::fs::canonicalize(candidate).ok() != ours)
}

/// Whether typing `name` runs `command`, with `PATH` as given.
fn on_path(command: &Path, name: &str, path: Option<&OsStr>) -> bool {
    let ours = std::fs::canonicalize(command).ok();
    runnable_on_path(name, path)
        .next()
        .is_some_and(|first| ours.is_some() && std::fs::canonicalize(first).ok() == ours)
}

/// Writes `file` whole, or leaves what was there.
fn write_file(file: &Path, contents: &[u8], mode: u32) -> Result<(), Error> {
    crate::registry::text::replace(file, contents, mode)
        .map_err(|e| Error::ShortcutFile(file.to_path_buf(), e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

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

    fn logs(dir: &Path) -> String {
        dir.join("logs").to_str().unwrap().to_owned()
    }

    /// A stand-in for Raven that prints each argument it gets on a line of
    /// its own, and what it printed to stderr goes to the log too.
    fn fake_raven(dir: &Path) -> String {
        let raven = dir.join("fake raven");
        std::fs::write(
            &raven,
            "#!/bin/sh\nfor a in \"$@\"; do printf '[%s]\\n' \"$a\"; done\necho complaint >&2\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&raven, std::fs::Permissions::from_mode(0o755)).unwrap();
        raven.to_str().unwrap().to_owned()
    }

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    fn spec<'a>(
        env: &'a str,
        name: &'a str,
        argv: &'a [String],
        raven: &'a str,
        logs: &'a str,
    ) -> Spec<'a> {
        Spec {
            env,
            name,
            argv,
            cwd: None,
            title: "MyGame",
            raven,
            icon: None,
            program: program(argv),
            logs,
        }
    }

    /// Runs `cmd` with no terminal on any stream and says whether it
    /// succeeded. A script written a moment ago can be busy for as long as
    /// another test's fork holds it open, so that is waited out.
    fn succeeds(cmd: &mut Command) -> bool {
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for _ in 0..100 {
            match cmd.status() {
                Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                other => return other.unwrap().success(),
            }
        }
        panic!("still busy");
    }

    /// Runs the command `name` in `places` with `args`, and returns its log.
    fn run(places: &Places, name: &str, logs: &str, args: &[&str]) -> String {
        assert!(succeeds(Command::new(places.command(name)).args(args)));
        std::fs::read_to_string(Path::new(logs).join(format!("{name}.log"))).unwrap()
    }

    #[test]
    fn the_command_runs_raven_with_every_argument_intact() {
        let dir = scratch("args");
        let places = places(&dir);
        let (raven, logs) = (fake_raven(&dir), logs(&dir));
        let args = argv(&["wine", r"C:\Games\It's $HOME\game.exe", "-x y", "*"]);
        let mut s = spec("my games", "game", &args, &raven, &logs);
        s.cwd = Some("/run/user/1000/raven/my games/c/Games");
        write(&places, &s).unwrap();

        let log = run(&places, "game", &logs, &["--server=10.0.0.1:27015"]);
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
        let (raven, logs) = (fake_raven(&dir), logs(&dir));
        let args = argv(&["wine", "game.exe"]);
        write(&places, &spec("games", "game", &args, &raven, &logs)).unwrap();
        run(&places, "game", &logs, &["first"]);
        run(&places, "game", &logs, &["second"]);
        let logs = Path::new(&logs);
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
    fn started_from_a_terminal_the_program_keeps_it() {
        let dir = scratch("tty");
        let places = places(&dir);
        let (raven, logs) = (fake_raven(&dir), logs(&dir));
        let args = argv(&["wine", "game.exe"]);
        write(&places, &spec("games", "game", &args, &raven, &logs)).unwrap();
        let script = std::fs::read_to_string(places.command("game")).unwrap();
        // Every stream is tested: `game > out.txt` typed in a terminal keeps
        // its input there, and its output goes where it was sent.
        assert!(
            script.contains("if ! [ -t 0 ] && ! [ -t 1 ] && ! [ -t 2 ] && mkdir -p -m 700 "),
            "{script}"
        );
        // Without a terminal, as above, there is a log; nothing was written
        // before the first start.
        assert!(!Path::new(&logs).exists());
    }

    #[test]
    fn a_raven_that_moved_is_found_on_path() {
        let dir = scratch("moved");
        let places = places(&dir);
        let logs = logs(&dir);
        let args = argv(&["wine", "game.exe"]);
        let gone = dir.join("gone/raven");
        write(
            &places,
            &spec("games", "game", &args, gone.to_str().unwrap(), &logs),
        )
        .unwrap();
        let path_dir = dir.join("path");
        std::fs::create_dir_all(&path_dir).unwrap();
        std::fs::rename(fake_raven(&dir), path_dir.join("raven")).unwrap();
        assert!(succeeds(
            Command::new(places.command("game"))
                .env("PATH", format!("{}:/usr/bin:/bin", path_dir.display()))
        ));
        let log = std::fs::read_to_string(Path::new(&logs).join("game.log")).unwrap();
        assert!(log.starts_with("[run]\n[games]\n"), "{log}");
    }

    #[test]
    fn the_menu_entry_runs_the_command_with_the_programs_icon() {
        let dir = scratch("entry");
        let places = places(&dir);
        let logs = logs(&dir);
        let args = argv(&["wine", r"C:\Games\MyGame\MyGame.exe"]);
        let image = crate::icon::png(2, 2, &[9; 16]);
        let mut s = spec("games", "mygame", &args, "/usr/bin/raven", &logs);
        s.icon = Some(&image);
        write(&places, &s).unwrap();

        let entry = std::fs::read_to_string(places.entry("mygame")).unwrap();
        let icon = places.icon("mygame");
        assert!(
            entry.starts_with("[Desktop Entry]\nType=Application\nX-Raven-Shortcut=mygame\n"),
            "Raven's mark comes before anything as long as a title: {entry}"
        );
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
        assert_eq!(std::fs::read(icon).unwrap(), image);

        // Made again without an icon, it shows Raven's, and the old one goes.
        s.icon = None;
        assert!(
            claim(&places, "games", "mygame").unwrap(),
            "now a replacement"
        );
        write(&places, &s).unwrap();
        let entry = std::fs::read_to_string(places.entry("mygame")).unwrap();
        assert!(entry.contains("\nIcon=raven\n"), "{entry}");
        assert!(!places.icon("mygame").exists());
    }

    #[test]
    fn a_long_title_does_not_hide_ravens_mark() {
        let dir = scratch("title");
        let places = places(&dir);
        let logs = logs(&dir);
        let args = argv(&["wine", "game.exe"]);
        let title = "x".repeat(10_000);
        let mut s = spec("games", "game", &args, "/usr/bin/raven", &logs);
        s.title = &title;
        write(&places, &s).unwrap();
        assert_eq!(owner(&places.entry("game"), is_entry), Owner::Raven);
    }

    #[test]
    fn a_file_raven_did_not_write_is_left_alone() {
        let dir = scratch("taken");
        let places = places(&dir);
        std::fs::create_dir_all(&places.bin).unwrap();
        std::fs::write(places.command("game"), "#!/bin/sh\necho mine\n").unwrap();
        assert!(matches!(
            claim(&places, "games", "game"),
            Err(Error::ShortcutTaken(p)) if p == places.command("game")
        ));
        assert!(remove_in(&places, "games", "game").is_err());
        assert_eq!(
            std::fs::read_to_string(places.command("game")).unwrap(),
            "#!/bin/sh\necho mine\n",
            "not replaced, not removed"
        );

        // A menu entry of that name that is not Raven's stops it too.
        std::fs::create_dir_all(&places.applications).unwrap();
        std::fs::write(places.entry("other"), "[Desktop Entry]\nName=Mine\n").unwrap();
        assert!(matches!(
            claim(&places, "games", "other"),
            Err(Error::ShortcutTaken(p)) if p == places.entry("other")
        ));
    }

    #[test]
    fn another_environments_shortcut_is_not_taken_over() {
        let dir = scratch("elsewhere");
        let places = places(&dir);
        let logs = logs(&dir);
        let args = argv(&["wine", "game.exe"]);
        write(
            &places,
            &spec("work", "game", &args, "/usr/bin/raven", &logs),
        )
        .unwrap();
        assert!(matches!(
            claim(&places, "games", "game"),
            Err(Error::ShortcutElsewhere { env, .. }) if env == "work"
        ));
        assert!(claim(&places, "work", "game").unwrap());
    }

    #[test]
    fn removing_keeps_a_menu_entry_that_is_not_ravens_and_says_so() {
        let dir = scratch("remove");
        let places = places(&dir);
        let logs = logs(&dir);
        let args = argv(&["wine", "game.exe"]);
        let image = crate::icon::png(1, 1, &[0; 4]);
        let mut s = spec("games", "game", &args, "/usr/bin/raven", &logs);
        s.icon = Some(&image);
        write(&places, &s).unwrap();
        // The user replaced the menu entry with one of their own.
        std::fs::write(places.entry("game"), "[Desktop Entry]\nName=Mine\n").unwrap();

        let removed = remove_in(&places, "games", "game").unwrap();
        assert_eq!(removed.entry_kept, Some(places.entry("game")));
        assert!(places.entry("game").exists());
        assert!(!places.command("game").exists());
        assert!(!places.icon("game").exists());
    }

    #[test]
    fn shortcuts_are_listed_and_removed_by_environment() {
        let dir = scratch("list");
        let places = places(&dir);
        let logs = logs(&dir);
        let raven = "/usr/bin/raven";
        let a = argv(&["wine", r"C:\A\a.exe"]);
        let b = argv(&["winecfg"]);
        write(&places, &spec("games", "zeta", &a, raven, &logs)).unwrap();
        write(&places, &spec("games", "alpha", &b, raven, &logs)).unwrap();
        write(&places, &spec("work", "office", &b, raven, &logs)).unwrap();
        std::fs::write(places.bin.join("unrelated"), "#!/bin/sh\n").unwrap();
        // What a write that died half way leaves.
        std::fs::copy(places.command("zeta"), places.bin.join(".zeta.raven-tmp")).unwrap();

        let games = list_in(&places, "games");
        let names: Vec<&str> = games.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["alpha", "zeta"]);
        assert_eq!(games[1].runs, r"wine 'C:\A\a.exe'");

        assert!(matches!(
            remove_in(&places, "games", "office"),
            Err(Error::NoSuchShortcut { .. })
        ));
        let gone = remove_in(&places, "games", "zeta").unwrap();
        assert_eq!(gone.shortcut.name, "zeta");
        assert_eq!(gone.entry_kept, None);
        assert!(!places.command("zeta").exists());
        assert!(!places.entry("zeta").exists());
        assert_eq!(list_in(&places, "games").len(), 1);
        assert_eq!(list_in(&places, "work").len(), 1);
    }

    #[test]
    fn a_name_must_be_a_plain_command() {
        let long = "a".repeat(MAX_NAME + 1);
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
            "wine",
            "wineserver",
            "é",
            &long,
        ] {
            assert!(check(bad).is_err(), "{bad:?}");
        }
        for ok in [
            "mygame",
            "gta5",
            "my-game_2.0",
            "c++",
            &"a".repeat(MAX_NAME),
        ] {
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
        std::fs::write(dir.join("outside.exe"), "host").unwrap();
        let roots = [upper.clone(), base.clone()];
        assert_eq!(
            locate(&roots, r"C:\mygame\MYGAME.EXE"),
            Some((Some(upper.clone()), upper.join("MyGame/MyGame.exe")))
        );
        assert_eq!(
            locate(&roots, r"c:\Program Files\Game\Game.exe"),
            Some((Some(base.clone()), base.join("Program Files/Game/Game.exe")))
        );
        assert_eq!(locate(&roots, r"C:\nowhere.exe"), None);
        assert_eq!(
            locate(&roots, r"D:\Game.exe"),
            None,
            "another drive is not C:'s layers"
        );
        assert_eq!(
            locate(&roots, r"C:\..\outside.exe"),
            None,
            "C: does not reach the host's files"
        );
    }

    #[test]
    fn a_program_starts_in_its_own_directory_inside_the_environment() {
        let dir = scratch("cwd");
        let env = Environment {
            name: "games".into(),
            manifest: crate::env::Manifest {
                base: "none".into(),
            },
            root: dir.clone(),
        };
        let upper = env.upper();
        let file = upper.join("Games/My Game/game.exe");
        let Ok(mount) = paths::mount_point("games") else {
            return; // No runtime directory here, so no mount point either.
        };
        assert_eq!(
            own_directory(&env, Some(&upper), &file).as_deref(),
            mount.join("Games/My Game").to_str()
        );
        // A Linux path is the same inside as outside.
        assert_eq!(
            own_directory(&env, None, Path::new("/opt/game/game.exe")).as_deref(),
            Some("/opt/game")
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
    fn a_command_line_starting_with_the_shortcuts_name_loops() {
        assert!(loops(&argv(&["mygame", "-x"]), "mygame"));
        assert!(
            !loops(&argv(&["/opt/mygame/mygame"]), "mygame"),
            "a path is not looked up"
        );
        assert!(!loops(&argv(&["wine", "mygame.exe"]), "mygame"));
    }

    #[test]
    fn a_program_too_large_or_endless_is_not_read() {
        let dir = scratch("read");
        let small = dir.join("small.exe");
        std::fs::write(&small, b"MZ").unwrap();
        assert_eq!(read_program(&small).as_deref(), Some(&b"MZ"[..]));
        // Sparse, so the test writes nothing; refused by its size alone.
        let large = std::fs::File::create(dir.join("large.exe")).unwrap();
        large.set_len(MAX_PROGRAM + 1).unwrap();
        assert_eq!(read_program(&dir.join("large.exe")), None);
        assert_eq!(read_program(&dir), None, "a directory is not a program");
        assert_eq!(
            read_program(Path::new("/dev/zero")),
            None,
            "not a regular file"
        );
    }

    #[test]
    fn another_program_of_the_name_anywhere_on_path_is_found() {
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
        // Before the command or after it: either way one hides the other.
        for dirs in [[mine.as_path(), &other], [other.as_path(), &mine]] {
            assert_eq!(
                elsewhere_on_path(&command, "game", Some(&path(&dirs))),
                Some(other.join("game"))
            );
        }
        assert_eq!(
            elsewhere_on_path(&command, "game", Some(&path(&[&mine]))),
            None
        );
        // A command not written yet is in conflict with anything of its name.
        assert_eq!(
            elsewhere_on_path(&dir.join("none/game"), "game", Some(&path(&[&mine]))),
            Some(command.clone())
        );

        assert!(on_path(&command, "game", Some(&path(&[&mine, &other]))));
        assert!(!on_path(&command, "game", Some(&path(&[&other, &mine]))));
        assert!(!on_path(&command, "game", Some(&path(&[&dir]))));
        assert!(!on_path(&command, "game", None));
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
