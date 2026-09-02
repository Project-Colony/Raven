//! Making `./program.exe` run like any other executable.
//!
//! The kernel already knows how to do this. `binfmt_misc` maps a file's magic
//! bytes to an interpreter, and a PE executable starts with `MZ` — the initials
//! of Mark Zbikowski, who designed the format in 1983 and whose name has been at
//! the front of every DOS and Windows binary since.
//!
//! Wine has shipped such a registration for years. Raven's differs in one way:
//! it points at a resolver rather than at `wine` directly, because a program has
//! to be run *in an environment*, and which one is a question the kernel cannot
//! answer.

use std::path::{Path, PathBuf};

use crate::{Error, env::Environment, paths};

/// The name Raven registers under, used to install and to remove it.
pub const BINFMT_NAME: &str = "raven-pe";

/// The line `/etc/binfmt.d/raven.conf` contains.
///
/// The fields are name, type (Magic), offset, magic, mask, interpreter, flags.
/// The `F` flag makes the kernel open the interpreter at registration time and
/// hold it open, so the registration keeps working inside a mount namespace
/// where `/usr/bin` may not be what it was — which is precisely the situation
/// Raven creates for every program it runs.
pub fn binfmt_line(interpreter: &Path) -> String {
    format!(":{BINFMT_NAME}:M::MZ::{}:F", interpreter.display())
}

/// Whether Raven's registration is currently active.
pub fn registered() -> bool {
    Path::new("/proc/sys/fs/binfmt_misc")
        .join(BINFMT_NAME)
        .exists()
}

/// One `binfmt_misc` registration that would claim a Windows executable.
///
/// Wine ships one of these too, and when both are present the kernel picks
/// one silently. The evening that cost an hour, every `.exe` ran against
/// `~/.wine` and the failure looked like Raven losing its prefix — so
/// `raven doctor` reports every claimant, not just Raven's.
#[derive(Debug, PartialEq, Eq)]
pub struct ExeHandler {
    pub name: String,
    pub enabled: bool,
    pub interpreter: PathBuf,
    /// The `F` flag. The kernel holds the interpreter open from registration
    /// on, so deleting the binary leaves the registration working — until the
    /// next boot registers from the conf file again and finds nothing.
    pub held_open: bool,
}

/// Every registration that would claim a `.exe`, in the order the kernel
/// tries them. The first *enabled* one wins.
///
/// The kernel scans newest-registration-first, and the directory lists in
/// that same order. Verified by experiment rather than read from
/// documentation: two entries registered for `MZ` in a sandboxed
/// `binfmt_misc` mount, and the one registered second both listed first
/// (unsorted readdir order) and ran.
pub fn exe_handlers() -> Vec<ExeHandler> {
    exe_handlers_in(Path::new("/proc/sys/fs/binfmt_misc"))
}

fn exe_handlers_in(dir: &Path) -> Vec<ExeHandler> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "register" || name == "status" {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(entry.path()) {
            found.extend(parse_handler(&name, &text));
        }
    }
    found
}

/// Reads one `/proc/sys/fs/binfmt_misc/<name>` entry, keeping it only when it
/// would claim a Windows executable: magic bytes overlapping `MZ` at offset
/// zero, or the `.exe` extension.
fn parse_handler(name: &str, text: &str) -> Option<ExeHandler> {
    let mut enabled = false;
    let mut interpreter = None;
    let mut held_open = false;
    let mut offset = 0u64;
    let mut magic = None;
    let mut extension = None;
    for line in text.lines() {
        if line == "enabled" {
            enabled = true;
        } else if let Some(v) = line.strip_prefix("interpreter ") {
            interpreter = Some(PathBuf::from(v));
        } else if let Some(v) = line.strip_prefix("flags:") {
            held_open = v.contains('F');
        } else if let Some(v) = line.strip_prefix("offset ") {
            offset = v.parse().ok()?;
        } else if let Some(v) = line.strip_prefix("magic ") {
            magic = Some(v.to_owned());
        } else if let Some(v) = line.strip_prefix("extension ") {
            extension = Some(v.to_owned());
        }
    }
    let claims_exe = match (&magic, &extension) {
        // A magic shorter than `MZ` claims a superset of PEs; a longer one
        // starting with it still claims almost every PE. A mask could in
        // principle bend a different magic onto `MZ`; nothing registers such
        // a thing, and this reads registrations rather than re-implementing
        // the kernel's matcher.
        (Some(m), _) => offset == 0 && (m.starts_with("4d5a") || "4d5a".starts_with(m.as_str())),
        (None, Some(e)) => e == ".exe",
        _ => false,
    };
    if !claims_exe {
        return None;
    }
    Some(ExeHandler {
        name: name.to_owned(),
        enabled,
        interpreter: interpreter?,
        held_open,
    })
}

/// Whether a path is a Windows executable, by reading its first two bytes.
///
/// `binfmt_misc` invokes its interpreter as `interpreter <file> <args…>`, with
/// no way to pass a subcommand — so `raven` is handed a path where it expects a
/// verb. Checking the magic is what makes that unambiguous: a subcommand name is
/// never a readable file starting with `MZ`, so nothing a user types can be
/// mistaken for a program, and no program can be mistaken for a verb.
pub fn looks_like_pe(path: &Path) -> bool {
    use std::io::Read as _;
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 2];
    f.read_exact(&mut magic).is_ok() && &magic == b"MZ"
}

/// Which environment a program should run in.
///
/// A path inside an environment answers for itself. Anything else falls back to
/// the configured default, and having neither is an error worth stating plainly
/// rather than a guess worth making.
pub fn resolve(exe: &Path) -> Result<Environment, Error> {
    let exe = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());

    for env in Environment::list()? {
        if exe.starts_with(&env.root) {
            return Ok(env);
        }
        // A program launched from inside a live mount is under the runtime
        // mount point, not under the environment's data directory.
        if let Ok(mount) = paths::mount_point(&env.name) {
            if exe.starts_with(&mount) {
                return Ok(env);
            }
        }
    }

    match default_environment()? {
        Some(name) => Environment::open(&name),
        None => Err(Error::NoDefaultEnvironment),
    }
}

/// The environment used for programs that are not inside one.
pub fn default_environment() -> Result<Option<String>, Error> {
    let file = paths::config_dir()?.join("default-environment");
    Ok(std::fs::read_to_string(file)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty()))
}

pub fn set_default_environment(name: &str) -> Result<(), Error> {
    // Opening it first means a typo is refused here rather than at the next
    // double-click, when nothing will explain why.
    Environment::open(name)?;
    let dir = paths::config_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| Error::Layer(dir.clone(), e))?;
    let file = dir.join("default-environment");
    std::fs::write(&file, name).map_err(|e| Error::Layer(file, e))
}

/// Where the packaged registration file belongs.
pub fn conf_path() -> PathBuf {
    PathBuf::from("/etc/binfmt.d/raven.conf")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pe_is_recognised_by_its_magic_and_nothing_else_is() {
        let dir = std::env::temp_dir().join(format!("raven-pe-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let exe = dir.join("prog.exe");
        std::fs::write(&exe, b"MZ\x90\x00").unwrap();
        let text = dir.join("notes.txt");
        std::fs::write(&text, b"hello").unwrap();

        assert!(looks_like_pe(&exe));
        assert!(!looks_like_pe(&text), "content decides, not the extension");
        assert!(
            !looks_like_pe(Path::new("doctor")),
            "a subcommand is not a file"
        );
        assert!(!looks_like_pe(&dir), "a directory is not a program");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_too_short_to_have_magic_is_not_a_pe() {
        let f = std::env::temp_dir().join(format!("raven-short-{}", std::process::id()));
        std::fs::write(&f, b"M").unwrap();
        assert!(!looks_like_pe(&f));
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn the_registration_line_matches_what_binfmt_misc_parses() {
        let line = binfmt_line(Path::new("/usr/bin/raven"));
        assert_eq!(line, ":raven-pe:M::MZ::/usr/bin/raven:F");
        // Seven colon-separated fields plus the leading empty one.
        assert_eq!(line.split(':').count(), 8);
    }

    #[test]
    fn the_magic_is_the_two_bytes_a_pe_actually_starts_with() {
        let line = binfmt_line(Path::new("/usr/bin/raven"));
        let magic = line.split(':').nth(4).unwrap();
        assert_eq!(magic, "MZ");
    }

    // The fixture texts below are byte-for-byte what the kernel prints,
    // captured from a sandboxed binfmt_misc mount rather than written from
    // memory of the format.

    #[test]
    fn wines_registration_is_recognised_as_a_rival() {
        let h = parse_handler(
            "DOSWin",
            "enabled\ninterpreter /usr/bin/wine\nflags: \noffset 0\nmagic 4d5a\n",
        )
        .expect("Wine claims MZ and must be reported");
        assert!(h.enabled);
        assert_eq!(h.interpreter, PathBuf::from("/usr/bin/wine"));
        assert!(!h.held_open, "Wine registers without F");
    }

    #[test]
    fn a_disabled_entry_is_kept_but_marked() {
        // Disabled entries never match, but doctor must still list them: a
        // user who disabled one by hand deserves to see it.
        let h = parse_handler(
            "DOSWin",
            "disabled\ninterpreter /usr/bin/wine\nflags: \noffset 0\nmagic 4d5a\n",
        )
        .unwrap();
        assert!(!h.enabled);
    }

    #[test]
    fn an_extension_entry_claims_a_double_clicked_exe() {
        let h = parse_handler(
            "byext",
            "enabled\ninterpreter /usr/bin/wine\nflags: \nextension .exe\n",
        )
        .expect(".exe by extension is a claim too");
        assert!(h.enabled);
    }

    #[test]
    fn unrelated_registrations_are_not_reported() {
        // ELF magic, a different extension, and MZ at a non-zero offset: none
        // of these would claim a PE.
        for (name, text) in [
            (
                "elf",
                "enabled\ninterpreter /bin/true\nflags: \noffset 0\nmagic 7f454c46\n",
            ),
            (
                "com",
                "enabled\ninterpreter /usr/bin/wine\nflags: \nextension .com\n",
            ),
            (
                "shifted",
                "enabled\ninterpreter /bin/true\nflags: POCF\noffset 2\nmagic 4d5a\n",
            ),
        ] {
            assert!(
                parse_handler(name, text).is_none(),
                "{name} should not match"
            );
        }
    }

    #[test]
    fn the_f_flag_is_what_keeps_a_deleted_interpreter_alive() {
        let h = parse_handler(
            "raven-pe",
            "enabled\ninterpreter /usr/bin/raven\nflags: F\noffset 0\nmagic 4d5a\n",
        )
        .unwrap();
        assert!(h.held_open);
    }

    #[test]
    fn scanning_a_directory_skips_the_control_files() {
        let dir = std::env::temp_dir().join(format!("raven-bfm-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(
            dir.join("raven-pe"),
            "enabled\ninterpreter /usr/bin/raven\nflags: F\noffset 0\nmagic 4d5a\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("DOSWin"),
            "enabled\ninterpreter /usr/bin/wine\nflags: \noffset 0\nmagic 4d5a\n",
        )
        .unwrap();
        // The two pseudo-files every binfmt_misc mount carries.
        std::fs::write(dir.join("register"), "").unwrap();
        std::fs::write(dir.join("status"), "enabled\n").unwrap();

        let mut names: Vec<String> = exe_handlers_in(&dir).into_iter().map(|h| h.name).collect();
        names.sort();
        assert_eq!(names, ["DOSWin", "raven-pe"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_interpreter_is_held_open_by_the_kernel() {
        // Without F the kernel resolves the interpreter path at exec time, in
        // the caller's mount namespace - which for Raven is one where /usr/bin
        // may not hold what it did.
        assert!(binfmt_line(Path::new("/usr/bin/raven")).ends_with(":F"));
    }
}
