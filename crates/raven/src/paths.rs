//! Where Raven keeps its files.
//!
//! Follows [the org filesystem
//! rule](https://github.com/Project-Colony/Project-Colony-Resources/blob/main/design/filesystem.md):
//! everything under `Colony/Raven/`, with config, data and cache kept apart.
//!
//! The canonical helper is `colony_ui::paths`, and it is not used here. It lives
//! in an iced crate, so a command-line program would pull a GUI toolkit in to
//! compute a directory name. Eidos hit the same wall and wrote `eidos-paths`;
//! this is the second workaround, and the real fix - a `colony-paths` crate that
//! `colony-ui` re-exports - belongs upstream. Until then
//! `crates/raven-gui/tests/paths_parity.rs` holds the two to the same answer:
//! it fails the build if this table and `colony_ui::paths::locate` disagree.
//!
//! Raven is Linux-only, so this implements the XDG rules directly rather than
//! taking a dependency to abstract over platforms that will never be targeted.

use std::path::{Path, PathBuf};

use crate::Error;

const ORG: &str = "Colony";
const PROGRAM: &str = "Raven";

/// `~/.config/Colony/Raven/` - what the user chose and would want to keep.
pub fn config_dir() -> Result<PathBuf, Error> {
    Ok(xdg("XDG_CONFIG_HOME", ".config")?.join(ORG).join(PROGRAM))
}

/// `~/.local/share/Colony/Raven/` - what Raven produced and cannot re-derive.
pub fn data_dir() -> Result<PathBuf, Error> {
    Ok(xdg("XDG_DATA_HOME", ".local/share")?
        .join(ORG)
        .join(PROGRAM))
}

/// `~/.cache/Colony/Raven/` - deleting all of this must cost only time.
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

/// `~/.local/share/applications/` - where desktop entries go, Wine's among
/// them.
pub(crate) fn applications_dir() -> Result<PathBuf, Error> {
    Ok(xdg("XDG_DATA_HOME", ".local/share")?.join("applications"))
}

/// `~/.local/bin/` - where a shortcut goes, so it runs by its name.
///
/// The directory systemd's `file-hierarchy` gives a user's own executables,
/// and the one distributions put on `PATH` when it exists. `XDG_BIN_HOME` is
/// not in the XDG specification, but some systems set it, and a user who
/// did means this directory.
pub fn bin_dir() -> Result<PathBuf, Error> {
    xdg("XDG_BIN_HOME", ".local/bin")
}

/// The user's desktop, as `~/.config/user-dirs.dirs` names it: where Wine puts
/// the entry for a shortcut a program places on Windows' desktop.
pub(crate) fn desktop_dir() -> Result<PathBuf, Error> {
    let home = std::env::var_os("HOME").ok_or(Error::NoHome)?;
    let dirs = std::fs::read_to_string(xdg("XDG_CONFIG_HOME", ".config")?.join("user-dirs.dirs"))
        .unwrap_or_default();
    Ok(desktop_from(&dirs, Path::new(&home)))
}

/// `XDG_DESKTOP_DIR` out of a `user-dirs.dirs`, with `$HOME` expanded, or
/// `~/Desktop` when it is not set.
fn desktop_from(dirs: &str, home: &Path) -> PathBuf {
    dirs.lines()
        .find_map(|l| l.trim().strip_prefix("XDG_DESKTOP_DIR="))
        .map(|v| v.trim().trim_matches('"'))
        .map(|v| match v.strip_prefix("$HOME") {
            Some(rest) => home.join(rest.trim_start_matches('/')),
            None => PathBuf::from(v),
        })
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join("Desktop"))
}

/// An environment's runtime state: its mount point and its session record.
///
/// This lives under the runtime directory, not under data: a mount must not
/// survive a reboot, and a stale mount point or session record found in a data
/// directory after a crash is worse than finding nothing.
pub fn runtime_dir(name: &str) -> Result<PathBuf, Error> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or(Error::NoRuntimeDir)?;
    Ok(base.join("raven").join(check_name(name)?))
}

/// `$XDG_RUNTIME_DIR/raven-logs/` - what each shortcut's program printed the
/// last two times it started.
///
/// Beside `raven/` rather than in it, where every name is an environment's.
/// Under the runtime directory, which only the user can enter, and which
/// empties at logout: a log is for the start that just failed.
pub fn logs_dir() -> Result<PathBuf, Error> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or(Error::NoRuntimeDir)?;
    Ok(base.join("raven-logs"))
}

/// Where an environment's C: is mounted while it runs.
pub fn mount_point(name: &str) -> Result<PathBuf, Error> {
    Ok(runtime_dir(name)?.join("c"))
}

fn xdg(var: &str, fallback: &str) -> Result<PathBuf, Error> {
    resolve(std::env::var_os(var), std::env::var_os("HOME"), fallback)
}

/// The rule, with the environment passed in rather than read.
///
/// Reading the environment inside this would make it untestable without
/// mutating process-global state, and `std::env::set_var` races with any other
/// thread - including the ones the test harness runs tests on.
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
    fn the_desktop_is_read_from_user_dirs_as_wine_finds_it() {
        let home = Path::new("/home/u");
        let dirs = "# written by xdg-user-dirs-update\nXDG_DOWNLOAD_DIR=\"$HOME/T\u{e9}l\u{e9}chargements\"\nXDG_DESKTOP_DIR=\"$HOME/Bureau\"\n";
        assert_eq!(desktop_from(dirs, home), home.join("Bureau"));
        assert_eq!(
            desktop_from("XDG_DESKTOP_DIR=\"/data/desk\"\n", home),
            Path::new("/data/desk")
        );
        // Not set, or set to something relative: the spec's default.
        assert_eq!(desktop_from("", home), home.join("Desktop"));
        assert_eq!(
            desktop_from("XDG_DESKTOP_DIR=\"desk\"", home),
            home.join("Desktop")
        );
    }

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
