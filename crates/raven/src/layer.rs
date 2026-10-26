//! Preparing a read-only layer so it stacks cleanly over a real Windows.
//!
//! `overlayfs` merges two directories only when their paths are identical byte
//! for byte. Wine's prefix skeleton spells things `windows`, `users` and
//! `system32`; a real Windows spells them `Windows`, `Users` and `System32`.
//! Stacked as they come, the two trees do not merge at all - the mount shows
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
/// case-insensitive counterpart in `reference` are left exactly as they are -
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
/// the base provides. An opaque marker makes it replace instead - everything the
/// base has at that path becomes invisible.
///
/// This is the shadow set applied to a whole subtree rather than a single file,
/// and there is one entry in it that measurement forced:
///
/// **`Windows\WinSxS`.** A real Windows carries a populated side-by-side
/// assembly store. An installer whose manifest asks for
/// `Microsoft.Windows.Common-Controls` 6.0 gets Microsoft's `comctl32` from it -
/// which loads, and then does not work against Wine's `user32`. The symptom is
/// precise and misleading: the window and its bitmaps draw, every control is
/// created, and nothing has any text or answers a click.
///
/// `WINEDLLOVERRIDES` cannot fix it. Side-by-side resolution goes through the
/// activation context, not the loader search path the override governs - which
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
/// goes in only when a measurement says it must - see `shadow`.
///
/// `WinSxS`: unmasked, installers render without text and ignore clicks.
///
/// `Fonts` **used to be here and is not any more.** Masking it was worth 92 ms
/// of per-process spawn (227 → 135 ms) because `win32u` re-checks every font
/// file at every process start, and that was decisive when every launch paid a
/// full cold start. It bought that with an incoherence: the projected registry
/// declared 961 fonts while `C:\Windows\Fonts` held none, so the registry
/// named files that did not exist, a program enumerating the directory saw
/// nothing where a real Windows shows 338, and every face was substituted
/// through fontconfig - Arial becoming Noto Sans, Segoe UI becoming Adwaita
/// Sans. Sessions changed the price: a launch costs 0.16 s now rather than two
/// seconds, so the same 92 ms is a fraction of a much smaller number. A real
/// Windows whose fonts are hidden is not a real Windows, which is the whole
/// premise, so the fonts are back.
pub const SHADOWED: &[&str] = &["Windows/WinSxS"];

/// Subtrees that *used* to be shadowed and no longer are.
///
/// The markers are written once, when an environment is created, so dropping
/// an entry from `SHADOWED` would leave every existing environment masked for
/// ever - and the user would have to destroy and rebuild to get a fix they
/// never asked to opt into. Reconciling against this list at session start
/// heals them in place.
///
/// An entry stays here indefinitely: it costs one `removexattr` on a directory
/// that usually does not carry the marker, and removing it would silently
/// strand anyone who had not started that environment since.
const FORMERLY_SHADOWED: &[&str] = &["Windows/Fonts"];

/// Removes an opaque marker, letting the base's version of a subtree show
/// through again.
pub fn unshadow(layer: &Path, relative: &str) -> Result<(), Error> {
    let target = layer.join(relative);
    match rustix::fs::removexattr(&target, "user.overlay.opaque") {
        Ok(()) => Ok(()),
        // Absent marker or absent directory: nothing to undo, which is the
        // ordinary case and not a failure.
        Err(rustix::io::Errno::NODATA | rustix::io::Errno::NOENT) => Ok(()),
        Err(e) => Err(Error::Layer(target, e.into())),
    }
}

/// Files Wine's layer must not put in front of the base's, and why.
///
/// The reverse of `SHADOWED`: there the base's copy has to disappear, here
/// Wine's does. Same rule for getting in: one measurement per entry.
///
/// `mscoree.dll`: the loader every .NET program starts through. Wine's copy
/// hands managed code to Wine Mono, which an environment does not have, so
/// with it in front every managed program stopped at "Wine Mono is not
/// installed" while the base carried .NET Framework 4.8. With Wine's copy
/// gone, Microsoft's loads and finds `clr.dll` under `Microsoft.NET`. No DLL
/// override is needed: Wine loads a native file it finds in System32 as
/// native.
///
/// `WinMetadata`: the type descriptions of the Windows Runtime. Wine ships ten
/// `.winmd` files of its own, a few kilobytes each against Microsoft's
/// hundreds, and its `RoResolveNamespace` reads whichever it finds. A .NET
/// program looking up a type Wine's file lacks failed with "Could not find
/// Windows Runtime type", which is how the Visual Studio 2019 installer died
/// on `Windows.Networking.Connectivity.NetworkCostType`.
pub const UNCOVERED: &[&str] = &[
    "Windows/System32/mscoree.dll",
    "Windows/SysWOW64/mscoree.dll",
    "Windows/System32/WinMetadata",
    "Windows/SysWOW64/WinMetadata",
];

/// Deletes Wine's copy of `relative` from `dir`, letting the base show
/// through.
///
/// A file is removed only when it carries Wine's signature: anything else in
/// the upper layer belongs to whatever put it there, and a .NET update does
/// write its own `mscoree.dll`. Wine's `.winmd` files carry no signature, so
/// for a directory every file the base also has is taken as Wine's. Nothing
/// but Wine's wineboot writes the OS's own namespaces into `WinMetadata`.
pub fn uncover(dir: &Path, base: &Path, relative: &str) -> Result<(), Error> {
    let target = dir.join(relative);
    let theirs = base.join(relative);
    if !theirs.is_dir() {
        if !is_wine_builtin(&target) {
            return Ok(());
        }
        return fs::remove_file(&target).map_err(|e| Error::Layer(target, e));
    }
    let Ok(entries) = fs::read_dir(&target) else {
        return Ok(());
    };
    for entry in entries.filter_map(Result::ok) {
        if entry.file_type().is_ok_and(|t| t.is_file()) && theirs.join(entry.file_name()).is_file()
        {
            fs::remove_file(entry.path()).map_err(|e| Error::Layer(entry.path(), e))?;
        }
    }
    Ok(())
}

/// Whether `path` is one of Wine's own PE files. Wine signs them right after
/// the 64-byte DOS header: "Wine builtin DLL" on the libraries current Wine
/// installs, "Wine placeholder DLL" on the stubs older Wine wrote instead.
fn is_wine_builtin(path: &Path) -> bool {
    use std::io::Read as _;
    let mut head = [0u8; 0x40 + 20];
    fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok_and(|()| {
            let sig = &head[0x40..];
            sig.starts_with(b"Wine builtin DLL") || sig.starts_with(b"Wine placeholder DLL")
        })
}

/// Brings a layer's markers in line with the current shadow set, and takes
/// Wine's copies of the uncovered files out of both layers.
///
/// Idempotent and cheap, so it can run before every mount rather than needing
/// a migration the user has to know about.
pub fn reconcile(layer: &Path, upper: &Path, base: &Path) -> Result<(), Error> {
    for rel in FORMERLY_SHADOWED {
        unshadow(layer, rel)?;
    }
    for rel in SHADOWED {
        if layer.join(rel).is_dir() {
            shadow(layer, rel)?;
        }
    }
    // The upper layer too: a Wine upgrade reruns wineboot inside the mount.
    // It copies Wine's .winmd files over the base's every time, and in an
    // environment whose layer still held Wine's mscoree.dll it wrote a fresh
    // one. Either hides the base from the upper layer just the same.
    for rel in UNCOVERED {
        uncover(layer, base, rel)?;
        uncover(upper, base, rel)?;
    }
    Ok(())
}

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
    fn reconciling_removes_a_marker_the_shadow_set_no_longer_wants() {
        let tmp = Tmp::new("reconcile");
        let layer = &tmp.0;
        // An environment created before Fonts left the shadow set.
        fs::create_dir_all(layer.join("Windows/Fonts")).unwrap();
        fs::create_dir_all(layer.join("Windows/WinSxS")).unwrap();
        shadow(layer, "Windows/Fonts").unwrap();
        shadow(layer, "Windows/WinSxS").unwrap();

        reconcile(layer, &tmp.0.join("upper"), &tmp.0.join("base")).unwrap();

        assert!(
            rustix::fs::getxattr(
                layer.join("Windows/Fonts"),
                "user.overlay.opaque",
                &mut [0u8; 8]
            )
            .is_err(),
            "the base's fonts must show through again"
        );
        assert!(
            rustix::fs::getxattr(
                layer.join("Windows/WinSxS"),
                "user.overlay.opaque",
                &mut [0u8; 8]
            )
            .is_ok(),
            "WinSxS is still shadowed and must stay so"
        );
        // Running it twice must not fail on the marker that is already gone.
        reconcile(layer, &tmp.0.join("upper"), &tmp.0.join("base")).unwrap();
    }

    /// The start of a PE file signed the way Wine signs its own, padded
    /// because a real one never ends at its signature.
    fn signed(sig: &str) -> String {
        format!("MZ{}{sig}{}", "\0".repeat(0x40 - 2), "\0".repeat(64))
    }

    #[test]
    fn reconciling_takes_wines_net_and_winrt_files_out_of_both_layers() {
        // Measured on a Windows 11 base: with Wine's mscoree.dll in front of
        // Microsoft's, every .NET program died on "Wine Mono is not
        // installed", and with Wine's .winmd files in front, a WinRT type
        // lookup found nothing. A Wine upgrade's wineboot writes both into
        // the upper layer.
        let tmp = Tmp::new("uncover");
        tmp.file(
            "layer/Windows/System32/mscoree.dll",
            &signed("Wine builtin DLL"),
        )
        .file(
            "layer/Windows/SysWOW64/mscoree.dll",
            &signed("Wine builtin DLL"),
        )
        .file(
            "upper/Windows/System32/mscoree.dll",
            &signed("Wine placeholder DLL"),
        )
        // Copied in by hand from the base: not Wine's, so not Raven's to
        // delete.
        .file("upper/Windows/SysWOW64/mscoree.dll", &signed("Microsoft"))
        .file(
            "layer/Windows/System32/kernel32.dll",
            &signed("Wine builtin DLL"),
        )
        .file(
            "base/Windows/System32/WinMetadata/Windows.Networking.winmd",
            "ms",
        )
        .file(
            "layer/Windows/System32/WinMetadata/Windows.Networking.winmd",
            "wine",
        )
        .file(
            "upper/Windows/SysWOW64/WinMetadata/Windows.Networking.winmd",
            "wine",
        )
        .file(
            "base/Windows/SysWOW64/WinMetadata/Windows.Networking.winmd",
            "ms",
        )
        // A namespace the base does not describe stays Wine's to provide.
        .file(
            "layer/Windows/System32/WinMetadata/Windows.Wine.winmd",
            "wine",
        );
        let (layer, upper, base) = (tmp.0.join("layer"), tmp.0.join("upper"), tmp.0.join("base"));

        reconcile(&layer, &upper, &base).unwrap();

        assert!(!layer.join("Windows/System32/mscoree.dll").exists());
        assert!(!layer.join("Windows/SysWOW64/mscoree.dll").exists());
        assert!(!upper.join("Windows/System32/mscoree.dll").exists());
        assert!(
            upper.join("Windows/SysWOW64/mscoree.dll").exists(),
            "a native copy in the upper layer must survive"
        );
        assert!(
            layer.join("Windows/System32/kernel32.dll").exists(),
            "only the uncovered files go"
        );
        assert!(
            !layer
                .join("Windows/System32/WinMetadata/Windows.Networking.winmd")
                .exists()
        );
        assert!(
            !upper
                .join("Windows/SysWOW64/WinMetadata/Windows.Networking.winmd")
                .exists()
        );
        assert!(
            layer
                .join("Windows/System32/WinMetadata/Windows.Wine.winmd")
                .exists()
        );
        assert!(
            base.join("Windows/System32/WinMetadata/Windows.Networking.winmd")
                .exists(),
            "the base is read, never written"
        );
        // Nothing left to remove the second time round.
        reconcile(&layer, &upper, &base).unwrap();
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
