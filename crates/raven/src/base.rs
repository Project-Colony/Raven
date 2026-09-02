//! Deploying a Windows base from an installation image.
//!
//! An official Microsoft ISO carries `sources/install.wim`, and `wimlib` writes
//! an image out of it to an ordinary directory from Linux — no hypervisor, no
//! installer, no first boot. Not booting is the point: booting is what would
//! bind the installation to hardware that is not there.
//!
//! A base is immutable once deployed. Nothing here ever writes to an existing
//! one, and nothing anywhere else is allowed to either.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{Error, paths};

/// One edition inside an installation image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edition {
    pub index: u32,
    pub name: String,
    pub build: Option<String>,
}

/// A deployed Windows, identified by the directory name it lives under.
#[derive(Debug, Clone)]
pub struct Base {
    pub id: String,
    pub path: PathBuf,
}

impl Base {
    /// The bases already deployed, in a stable order.
    pub fn list() -> Result<Vec<Base>, Error> {
        let dir = paths::bases_dir()?;
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut out: Vec<Base> = std::fs::read_dir(&dir)
            .map_err(|e| Error::Layer(dir.clone(), e))?
            .filter_map(Result::ok)
            .filter(|e| e.path().is_dir())
            .map(|e| Base {
                id: e.file_name().to_string_lossy().into_owned(),
                path: e.path(),
            })
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    pub fn find(id: &str) -> Result<Base, Error> {
        let path = paths::bases_dir()?.join(paths::check_name(id)?);
        if !path.is_dir() {
            return Err(Error::NoSuchBase(id.to_owned()));
        }
        Ok(Base {
            id: id.to_owned(),
            path,
        })
    }

    /// Whether this looks like a Windows rather than an arbitrary directory.
    ///
    /// Checked before a base is used, because pointing an environment at the
    /// wrong directory produces failures that look like Wine bugs.
    pub fn looks_like_windows(&self) -> bool {
        self.path.join("Windows/System32/config/SOFTWARE").is_file()
    }
}

/// Lists the editions inside a `.wim`, by asking `wimlib-imagex`.
pub fn editions(image: &Path) -> Result<Vec<Edition>, Error> {
    let out = Command::new("wimlib-imagex")
        .arg("info")
        .arg(image)
        .output()
        .map_err(|e| Error::Tool("wimlib-imagex", e))?;
    if !out.status.success() {
        return Err(Error::ToolFailed(
            "wimlib-imagex info",
            String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        ));
    }
    Ok(parse_editions(&String::from_utf8_lossy(&out.stdout)))
}

/// Pulls the index/name/build triples out of `wimlib-imagex info` output.
///
/// Kept separate from the command so it can be tested without wimlib installed,
/// and so a change in wimlib's output shape fails a test rather than silently
/// returning an empty list.
fn parse_editions(text: &str) -> Vec<Edition> {
    let mut out = Vec::new();
    let (mut index, mut name, mut build) = (None, None, None);
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "Index" => {
                // A new index closes the previous record.
                if let (Some(i), Some(n)) = (index.take(), name.take()) {
                    out.push(Edition {
                        index: i,
                        name: n,
                        build: build.take(),
                    });
                }
                index = value.parse().ok();
            }
            "Name" => name = Some(value.to_owned()),
            "Build" => build = Some(value.to_owned()),
            _ => {}
        }
    }
    if let (Some(i), Some(n)) = (index, name) {
        out.push(Edition {
            index: i,
            name: n,
            build,
        });
    }
    out
}

/// Applies one edition of an image into a new base directory.
///
/// Refuses to touch a base that already exists: bases are immutable, and
/// "deploy over the top" is how an immutable thing quietly stops being one.
pub fn deploy(image: &Path, index: u32, id: &str) -> Result<Base, Error> {
    let dir = paths::bases_dir()?.join(paths::check_name(id)?);
    if dir.exists() {
        return Err(Error::BaseExists(id.to_owned()));
    }
    std::fs::create_dir_all(&dir).map_err(|e| Error::Layer(dir.clone(), e))?;

    let status = Command::new("wimlib-imagex")
        .arg("apply")
        .arg(image)
        .arg(index.to_string())
        .arg(&dir)
        .status()
        .map_err(|e| Error::Tool("wimlib-imagex", e))?;

    if !status.success() {
        // A half-applied base is worse than none: it looks deployable.
        let _ = std::fs::remove_dir_all(&dir);
        return Err(Error::ToolFailed(
            "wimlib-imagex apply",
            format!("exited with {status}"),
        ));
    }

    let base = Base {
        id: id.to_owned(),
        path: dir,
    };
    repoint_absolute_symlinks(&base.path)?;
    Ok(base)
}

/// Rewrites the reparse points a WIM leaves behind as absolute symlinks.
///
/// Applying a WIM turns Windows junctions into symlinks with absolute targets —
/// `Users\All Users` becomes a link to `/ProgramData`, which resolves against
/// the *Linux* root and points at nothing. Only a couple exist in a stock
/// Windows, and they are made relative so they resolve inside the base.
fn repoint_absolute_symlinks(root: &Path) -> Result<usize, Error> {
    fn walk(dir: &Path, root: &Path, fixed: &mut usize) -> Result<(), Error> {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return Ok(()), // an unreadable corner is not fatal here
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_symlink() {
                let Ok(target) = std::fs::read_link(&path) else {
                    continue;
                };
                if !target.is_absolute() {
                    continue;
                }
                // Interpret the absolute target as being rooted at the base.
                let inside = root.join(target.strip_prefix("/").unwrap_or(&target));
                let Some(parent) = path.parent() else {
                    continue;
                };
                if let Some(rel) = relative_to(&inside, parent) {
                    let _ = std::fs::remove_file(&path);
                    if std::os::unix::fs::symlink(&rel, &path).is_ok() {
                        *fixed += 1;
                    }
                }
            } else if path.is_dir() {
                walk(&path, root, fixed)?;
            }
        }
        Ok(())
    }
    let mut fixed = 0;
    walk(root, root, &mut fixed)?;
    Ok(fixed)
}

/// Expresses `target` as a path relative to `from`.
fn relative_to(target: &Path, from: &Path) -> Option<PathBuf> {
    let (t, f): (Vec<_>, Vec<_>) = (target.components().collect(), from.components().collect());
    let shared = t.iter().zip(&f).take_while(|(a, b)| a == b).count();
    let mut rel = PathBuf::new();
    for _ in shared..f.len() {
        rel.push("..");
    }
    for c in &t[shared..] {
        rel.push(c);
    }
    (!rel.as_os_str().is_empty()).then_some(rel)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIMLIB_INFO: &str = "\
Index:                  1
Name:                   Windows 11 Home
Description:            Windows 11 Home
Architecture:           x86_64
Build:                  26200

Index:                  6
Name:                   Windows 11 Pro
Description:            Windows 11 Pro
Architecture:           x86_64
Build:                  26200
";

    #[test]
    fn editions_are_parsed_from_wimlib_output() {
        let e = parse_editions(WIMLIB_INFO);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].index, 1);
        assert_eq!(e[1].index, 6);
        assert_eq!(e[1].name, "Windows 11 Pro");
        assert_eq!(e[1].build.as_deref(), Some("26200"));
    }

    #[test]
    fn a_trailing_record_is_not_dropped() {
        // The last edition has no following "Index:" to close it, which is
        // exactly the off-by-one a naive parser loses.
        assert_eq!(parse_editions(WIMLIB_INFO).last().unwrap().index, 6);
    }

    #[test]
    fn unrelated_output_yields_no_editions() {
        assert!(parse_editions("ERROR: cannot open file\n").is_empty());
    }

    #[test]
    fn absolute_targets_become_relative_within_the_base() {
        assert_eq!(
            relative_to(Path::new("/base/ProgramData"), Path::new("/base/Users")).unwrap(),
            PathBuf::from("../ProgramData")
        );
        assert_eq!(
            relative_to(Path::new("/base/Users/Default"), Path::new("/base/Users")).unwrap(),
            PathBuf::from("Default")
        );
    }
}
