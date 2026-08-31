//! Where Raven keeps its files.
//!
//! Follows [the org filesystem
//! rule](https://github.com/Project-Colony/Project-Colony-Resources/blob/main/design/filesystem.md):
//! everything under `Colony/Raven/`, with config, data and cache kept apart.
//!
//! The canonical helper is `colony_ui::paths`, and it is not used here. It lives
//! in an iced crate, so a command-line program would pull a GUI toolkit in to
//! compute a directory name. Eidos hit the same wall and wrote `eidos-paths`;
//! this is the second workaround, and the real fix — a `colony-paths` crate that
//! `colony-ui` re-exports — belongs upstream.
//!
//! Raven is Linux-only, so this implements the XDG rules directly rather than
//! taking a dependency to abstract over platforms that will never be targeted.

use std::path::PathBuf;

use crate::Error;

const ORG: &str = "Colony";
const PROGRAM: &str = "Raven";

/// `~/.config/Colony/Raven/` — what the user chose and would want to keep.
pub fn config_dir() -> Result<PathBuf, Error> {
    Ok(xdg("XDG_CONFIG_HOME", ".config")?.join(ORG).join(PROGRAM))
}

/// `~/.local/share/Colony/Raven/` — what Raven produced and cannot re-derive.
pub fn data_dir() -> Result<PathBuf, Error> {
    Ok(xdg("XDG_DATA_HOME", ".local/share")?
        .join(ORG)
        .join(PROGRAM))
}

/// `~/.cache/Colony/Raven/` — deleting all of this must cost only time.
pub fn cache_dir() -> Result<PathBuf, Error> {
    Ok(xdg("XDG_CACHE_HOME", ".cache")?.join(ORG).join(PROGRAM))
}

/// Where deployed Windows bases live.
///
/// A base is data rather than cache despite being reproducible from an ISO:
/// reproducing it needs an ISO the user may no longer have, and Microsoft's
/// download links expire.
pub fn bases_dir() -> Result<PathBuf, Error> {
    Ok(data_dir()?.join("bases"))
}

/// The three layers and the Wine prefix belonging to one environment.
pub fn environment_dir(name: &str) -> Result<PathBuf, Error> {
    Ok(data_dir()?.join("environments").join(check_name(name)?))
}

/// Where an environment's C: is mounted while it runs.
///
/// This is runtime state and lives under the runtime directory, not under data:
/// a mount must not survive a reboot, and a stale mount point found in a data
/// directory after a crash is worse than finding nothing.
pub fn mount_point(name: &str) -> Result<PathBuf, Error> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or(Error::NoRuntimeDir)?;
    Ok(base.join("raven").join(check_name(name)?).join("c"))
}

fn xdg(var: &str, fallback: &str) -> Result<PathBuf, Error> {
    resolve(std::env::var_os(var), std::env::var_os("HOME"), fallback)
}

/// The rule, with the environment passed in rather than read.
///
/// Reading the environment inside this would make it untestable without
/// mutating process-global state, and `std::env::set_var` races with any other
/// thread — including the ones the test harness runs tests on.
fn resolve(
    value: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
    fallback: &str,
) -> Result<PathBuf, Error> {
    if let Some(v) = value {
        let p = PathBuf::from(v);
        // The XDG spec says a relative value must be ignored, not resolved.
        if p.is_absolute() {
            return Ok(p);
        }
    }
    Ok(PathBuf::from(home.ok_or(Error::NoHome)?).join(fallback))
}

/// An environment name is joined into a path that later gets written to and
/// removed, so it may not climb out of the directory it belongs in.
pub(crate) fn check_name(name: &str) -> Result<&str, Error> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\0') {
        return Err(Error::BadName(name.to_owned()));
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_that_escape_their_directory_are_refused() {
        for bad in ["", ".", "..", "../../etc", "a/b", "a\0b"] {
            assert!(
                check_name(bad).is_err(),
                "{bad:?} should be refused - it is joined into a path that gets removed"
            );
        }
    }

    #[test]
    fn ordinary_names_are_accepted() {
        for ok in ["skyrim", "Photoshop 2024", "a.b", "..hidden"] {
            assert!(check_name(ok).is_ok(), "{ok:?} should be accepted");
        }
    }

    fn os(s: &str) -> Option<std::ffi::OsString> {
        Some(std::ffi::OsString::from(s))
    }

    #[test]
    fn a_relative_xdg_value_is_ignored_per_spec() {
        let p = resolve(os("relative/path"), os("/home/someone"), ".config").unwrap();
        assert_eq!(p, PathBuf::from("/home/someone/.config"));
    }

    #[test]
    fn an_absolute_xdg_value_wins_over_home() {
        let p = resolve(os("/somewhere/else"), os("/home/someone"), ".config").unwrap();
        assert_eq!(p, PathBuf::from("/somewhere/else"));
    }

    #[test]
    fn without_home_or_xdg_there_is_no_guess() {
        assert!(matches!(resolve(None, None, ".config"), Err(Error::NoHome)));
    }

    #[test]
    fn the_three_roots_are_distinct() {
        let home = os("/home/someone");
        let c = resolve(None, home.clone(), ".config")
            .unwrap()
            .join(ORG)
            .join(PROGRAM);
        let d = resolve(None, home.clone(), ".local/share")
            .unwrap()
            .join(ORG)
            .join(PROGRAM);
        let k = resolve(None, home, ".cache")
            .unwrap()
            .join(ORG)
            .join(PROGRAM);
        assert_ne!(c, d);
        assert_ne!(d, k);
        assert_ne!(c, k);
        // The invariant the org doc names: clearing the cache must not take
        // config or data with it.
        assert!(!c.starts_with(&k) && !d.starts_with(&k));
    }
}
