//! Preparing a read-only layer so it stacks cleanly over a real Windows.
//!
//! `overlayfs` merges two directories only when their paths are identical byte
//! for byte. Wine's prefix skeleton spells things `windows`, `users` and
//! `system32`; a real Windows spells them `Windows`, `Users` and `System32`.
//! Stacked as they come, the two trees do not merge at all — the mount shows
//! both, and Wine's files shadow nothing.
//!
//! Wine's own case-insensitive path resolution cannot help: it operates on
//! Windows paths, one layer above the filesystem, long after `overlayfs` has
//! decided which directories are the same directory.
//!
//! So the layer is renamed to match. Measured against a real Windows 11 base,
//! this is 338 renames, after which the trees merge and `System32` holds the
//! union of both.

use std::fs;
use std::path::Path;

use crate::Error;

/// Renames entries in `layer` to match `reference`'s spelling wherever the two
/// differ only by case. Returns how many paths were renamed.
///
/// Only `layer` is modified; `reference` is read. Entries with no
/// case-insensitive counterpart in `reference` are left exactly as they are —
/// Wine ships files a real Windows does not have, and those must survive.
pub fn normalise_case(layer: &Path, reference: &Path) -> Result<usize, Error> {
    let mut renamed = 0;
    walk(layer, reference, &mut renamed)?;
    Ok(renamed)
}

/// Descends one directory, renaming before recursing.
///
/// The order matters: renaming a directory changes the path its children are
/// reached by, so a pass that recursed first would descend into paths it was
/// about to invalidate.
fn walk(layer: &Path, reference: &Path, renamed: &mut usize) -> Result<(), Error> {
    if !reference.is_dir() {
        return Ok(());
    }

    let canonical: std::collections::HashMap<String, String> = fs::read_dir(reference)
        .map_err(|e| Error::Layer(reference.to_path_buf(), e))?
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .map(|n| (n.to_lowercase(), n))
        .collect();

    let entries: Vec<_> = fs::read_dir(layer)
        .map_err(|e| Error::Layer(layer.to_path_buf(), e))?
        .filter_map(Result::ok)
        .collect();

    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let name = match canonical.get(&name.to_lowercase()) {
            Some(canon) if *canon != name => {
                fs::rename(layer.join(&name), layer.join(canon))
                    .map_err(|e| Error::Layer(layer.join(&name), e))?;
                *renamed += 1;
                canon.clone()
            }
            _ => name,
        };

        // Symlinks are not followed: descending through one would rename files
        // outside the layer, and a Windows tree does contain them.
        let child = layer.join(&name);
        if child.is_dir() && !child.is_symlink() {
            walk(&child, &reference.join(&name), renamed)?;
        }
    }
    Ok(())
}

/// Hides a directory of the base behind an empty one in this layer.
///
/// `overlayfs` normally *merges* directories, so a layer can only add to what
/// the base provides. An opaque marker makes it replace instead — everything the
/// base has at that path becomes invisible.
///
/// This is the shadow set applied to a whole subtree rather than a single file,
/// and there is one entry in it that measurement forced:
///
/// **`Windows\WinSxS`.** A real Windows carries a populated side-by-side
/// assembly store. An installer whose manifest asks for
/// `Microsoft.Windows.Common-Controls` 6.0 gets Microsoft's `comctl32` from it —
/// which loads, and then does not work against Wine's `user32`. The symptom is
/// precise and misleading: the window and its bitmaps draw, every control is
/// created, and nothing has any text or answers a click.
///
/// `WINEDLLOVERRIDES` cannot fix it. Side-by-side resolution goes through the
/// activation context, not the loader search path the override governs — which
/// is why forcing `comctl32=b` changes nothing and hiding the store changes
/// everything.
///
/// The mask sits in the read-only layer, so the writable overlay above it is
/// unaffected: an installer that registers its own assemblies into the
/// environment still works.
pub fn shadow(layer: &Path, relative: &str) -> Result<(), Error> {
    let target = layer.join(relative);
    std::fs::create_dir_all(&target).map_err(|e| Error::Layer(target.clone(), e))?;
    // `user.` rather than `trusted.`: Raven mounts with `userxattr` because it
    // is unprivileged, and an unprivileged process cannot set trusted xattrs.
    rustix::fs::setxattr(
        &target,
        "user.overlay.opaque",
        b"y",
        rustix::fs::XattrFlags::empty(),
    )
    .map_err(|e| Error::Layer(target, e.into()))
}

/// Subtrees of the base that a layer hides, and why.
///
/// Deliberately short. Each entry costs the environment something real, so one
/// goes in only when a measurement says it must — see `shadow`.
pub const SHADOWED: &[&str] = &["Windows/WinSxS"];

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(std::path::PathBuf);
    impl Tmp {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir().join(format!("raven-layer-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn file(&self, rel: &str, body: &str) -> &Self {
            let p = self.0.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, body).unwrap();
            self
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_shadowed_directory_is_marked_opaque() {
        let layer = Tmp::new("shadow");
        shadow(&layer.0, "Windows/WinSxS").unwrap();
        let mut buf = [0u8; 8];
        let n = rustix::fs::getxattr(
            layer.0.join("Windows/WinSxS"),
            "user.overlay.opaque",
            &mut buf,
        )
        .expect("the marker must be readable back, or overlayfs will not honour it");
        assert_eq!(&buf[..n], b"y");
    }

    #[test]
    fn shadowing_creates_the_directory_if_the_layer_lacks_it() {
        let layer = Tmp::new("shadow2");
        // Wine's skeleton has no WinSxS at all, so the common case is creating it.
        assert!(!layer.0.join("Windows/WinSxS").exists());
        shadow(&layer.0, "Windows/WinSxS").unwrap();
        assert!(layer.0.join("Windows/WinSxS").is_dir());
    }

    #[test]
    fn a_path_differing_only_by_case_is_renamed_to_match() {
        let layer = Tmp::new("l1");
        let reference = Tmp::new("r1");
        layer.file("windows/system32/ntdll.dll", "wine");
        reference.file("Windows/System32/ntdll.dll", "microsoft");

        assert_eq!(normalise_case(&layer.0, &reference.0).unwrap(), 2);
        assert!(layer.0.join("Windows/System32/ntdll.dll").exists());
        assert!(!layer.0.join("windows").exists());
    }

    #[test]
    fn a_path_the_reference_does_not_have_is_left_alone() {
        let layer = Tmp::new("l2");
        let reference = Tmp::new("r2");
        // Wine ships files a real Windows has never had; they must survive.
        layer.file("windows/winebus.sys", "wine only");
        reference.file("Windows/ntoskrnl.exe", "microsoft");

        assert_eq!(normalise_case(&layer.0, &reference.0).unwrap(), 1);
        assert!(layer.0.join("Windows/winebus.sys").exists());
    }

    #[test]
    fn identical_spelling_is_not_counted_as_a_rename() {
        let layer = Tmp::new("l3");
        let reference = Tmp::new("r3");
        layer.file("Windows/System32/a.dll", "x");
        reference.file("Windows/System32/a.dll", "y");

        assert_eq!(normalise_case(&layer.0, &reference.0).unwrap(), 0);
        assert_eq!(
            fs::read_to_string(layer.0.join("Windows/System32/a.dll")).unwrap(),
            "x",
            "the layer's own content must never be replaced by the reference's"
        );
    }

    #[test]
    fn renaming_a_directory_does_not_lose_its_children() {
        let layer = Tmp::new("l4");
        let reference = Tmp::new("r4");
        layer
            .file("windows/system32/drivers/etc/hosts", "wine")
            .file("windows/system32/kernel32.dll", "wine");
        reference
            .file("Windows/System32/drivers/etc/hosts", "ms")
            .file("Windows/System32/kernel32.dll", "ms");

        normalise_case(&layer.0, &reference.0).unwrap();
        assert!(layer.0.join("Windows/System32/drivers/etc/hosts").exists());
        assert!(layer.0.join("Windows/System32/kernel32.dll").exists());
    }

    #[test]
    fn a_symlink_is_renamed_but_not_descended_into() {
        let layer = Tmp::new("l5");
        let reference = Tmp::new("r5");
        let outside = Tmp::new("l5-outside");
        outside.file("secret.txt", "must not be touched");
        layer.file("windows/placeholder", "x");
        std::os::unix::fs::symlink(&outside.0, layer.0.join("windows/link")).unwrap();
        reference.file("Windows/placeholder", "y");
        fs::create_dir_all(reference.0.join("Windows/Link")).unwrap();
        reference.file("Windows/Link/SECRET.TXT", "y");

        normalise_case(&layer.0, &reference.0).unwrap();
        assert!(
            outside.0.join("secret.txt").exists(),
            "the walk followed a symlink and renamed a file outside the layer"
        );
    }
}
