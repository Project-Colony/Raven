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

    #[test]
    fn the_interpreter_is_held_open_by_the_kernel() {
        // Without F the kernel resolves the interpreter path at exec time, in
        // the caller's mount namespace - which for Raven is one where /usr/bin
        // may not hold what it did.
        assert!(binfmt_line(Path::new("/usr/bin/raven")).ends_with(":F"));
    }
}
