//! Installing a Direct3D-on-Vulkan runtime into an environment.
//!
//! Two of them exist and they do not overlap: **DXVK** reimplements Direct3D 8
//! through 11, and **vkd3d-proton** reimplements Direct3D 12. Neither is a
//! patch or a fork of anything - each is a set of DLLs, so installing one is
//! two operations and no more: put the DLLs where the loader looks, and tell
//! Wine to prefer them over its own builtins.
//!
//! They install side by side. A game wanting D3D11 and a game wanting D3D12
//! are different games, and an environment can serve both.
//!
//! Raven's twist is the whole reason this module exists. C: is a **real**
//! Windows, so `Windows/System32` already holds Microsoft's own `d3d11.dll` and
//! `dxgi.dll`. The DXVK copies go into the environment's writable upper layer,
//! where `overlayfs` makes them shadow Microsoft's - the base finishes
//! byte-identical, and `remove` puts the real Windows back by deleting files
//! rather than by restoring them. Nothing is ever written into a base.
//!
//! The overrides go in `user.reg`, **not** `system.reg`: Wine reads
//! `Software\Wine\DllOverrides` from HKCU. That was verified against the
//! installed `ntdll`, which builds a `\Registry\User\S-…` path for them and
//! carries no `\Registry\Machine` path at all - the opposite of the drive
//! configuration [`crate::attach`] writes. Do not assume the next setting
//! follows either of them.
//!
//! Raven downloads nothing. Point it at a build you already have - the upstream
//! release, the one inside a Proton, a distribution's package - the same way
//! `base deploy` takes an ISO you already have. There is no bundled version to
//! fall behind, and no opinion about whose build is right.
//!
//! That last part is not neutrality for its own sake. Both CachyOS's Proton and
//! Valve's carry these two projects as **unpatched upstream submodules**;
//! what a Proton distribution actually forks is Wine. There is no "the CachyOS
//! DXVK" to prefer, so preferring one in code would be inventing a distinction
//! that does not exist.

use std::path::{Path, PathBuf};

use crate::{Error, env::Environment, paths, registry::text};

/// The section Wine reads DLL overrides from, as it appears in `user.reg`.
const OVERRIDES: &str = "Software\\\\Wine\\\\DllOverrides";

/// One of the two runtimes, and everything that differs between them.
pub struct Runtime {
    /// What the CLI calls it, and the stem of its manifest file.
    pub key: &'static str,
    /// The modules it provides. Which of these a given build ships varies by
    /// version - DXVK dropped `d3d10.dll` and `d3d10_1.dll` upstream - so an
    /// install copies what it finds and reports it, rather than demanding a
    /// fixed set.
    pub dlls: &'static [&'static str],
    /// The directories inside a release, and where each lands on a real
    /// Windows. The 32-bit one is **not** named the same by both projects:
    /// DXVK ships `x32`, vkd3d-proton ships `x86`.
    pub arches: &'static [(&'static str, &'static str)],
}

/// Direct3D 8 through 11.
pub const DXVK: Runtime = Runtime {
    key: "dxvk",
    dlls: &[
        "d3d8",
        "d3d9",
        "d3d10core",
        "d3d11",
        "dxgi",
        "d3d10",
        "d3d10_1",
    ],
    arches: &[("x64", "Windows/System32"), ("x32", "Windows/SysWOW64")],
};

/// Direct3D 12, which DXVK does not implement at all.
pub const VKD3D: Runtime = Runtime {
    key: "vkd3d",
    dlls: &["d3d12", "d3d12core"],
    arches: &[("x64", "Windows/System32"), ("x86", "Windows/SysWOW64")],
};

/// One installed DLL: which module, and the Windows directory it shadows in.
#[derive(Debug, PartialEq, Eq)]
pub struct Shadow {
    pub dll: String,
    pub arch: &'static str,
    pub path: PathBuf,
}

impl Environment {
    /// Copies a DXVK build into the environment and points Wine at it.
    ///
    /// `source` is either an extracted DXVK release (a directory holding `x64/`
    /// and `x32/`) or a `.tar.gz` of one. Refuses while the environment runs:
    /// the overlay is mounted and `wineserver` would overwrite the registry
    /// edit on exit.
    pub fn install_d3d(&self, rt: &Runtime, source: &Path) -> Result<Vec<Shadow>, Error> {
        self.ensure_not_running()?;
        let (dir, keep) = unpack(source)?;
        let root = build_root(&dir, rt.key)?;

        // Plan every copy before performing any of it. Checking as we went
        // meant a refusal left the copies already made behind, and those then
        // blocked the next attempt - the failure mode taught us the rule:
        // decide first, write second.
        let ours = self.d3d_manifest(rt);
        let mut plan: Vec<(PathBuf, PathBuf, String, &str, &'static str)> = Vec::new();
        for (arch, windir) in rt.arches {
            let from_dir = root.join(arch);
            if !from_dir.is_dir() {
                // A 64-bit-only build is legitimate; a missing x64 is not, and
                // build_root has already refused that case.
                continue;
            }
            for dll in rt.dlls {
                let from = from_dir.join(format!("{dll}.dll"));
                if !from.is_file() {
                    continue;
                }
                // A link inside an archive points wherever its author chose,
                // and the copy below would follow it, carrying a file from
                // elsewhere on this machine into a layer every Windows program
                // in the environment can read. A directory the user names is
                // theirs to arrange; an archive is not.
                if keep.is_some() && through_link(&dir, &from) {
                    return Err(Error::D3dLink(from));
                }
                let rel = format!("{windir}/{dll}.dll");
                let to = self.upper().join(&rel);
                // A file already in the upper layer that Raven did not install
                // belongs to whatever wrote it. Overwriting it would be silent
                // damage, because removal would then delete it for good.
                if to.exists() && !ours.contains(&rel) {
                    return Err(Error::D3dWouldOverwrite(to));
                }
                plan.push((from, to, rel, dll, arch));
            }
        }
        if plan.is_empty() {
            return Err(Error::NotAD3dBuild(rt.key, root));
        }

        let mut written: Vec<String> = Vec::new();
        let mut created: Vec<String> = Vec::new();
        let mut done = Vec::new();
        for (from, to, rel, dll, arch) in &plan {
            // Whether this call is the reason the file is there. An upgrade
            // copies over the previous install's libraries, and undoing that
            // by deleting them would turn a failed upgrade into an uninstall
            // of the build that was working.
            let is_new = !to.exists();
            let copy = (|| {
                if let Some(parent) = to.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(from, to).map(|_| ())
            })();
            if let Err(e) = copy {
                // Undo the half-install rather than leave the environment in a
                // state neither `dxvk` nor `--remove` can describe.
                for r in &created {
                    let _ = std::fs::remove_file(self.upper().join(r));
                }
                // An upgrade has by now overwritten some of the previous
                // build's libraries in place, and those are not undone -
                // deleting them would uninstall what was working. What is
                // left is genuinely neither version, so the record stops
                // naming one: `dxvk` says so, and reinstalling either build
                // puts it right.
                if !ours.is_empty() {
                    let was = self.d3d_build(rt).unwrap_or_else(|| rt.key.to_string());
                    let _ = write_manifest(
                        &self.d3d_manifest_path(rt),
                        &format!("{was} - interrupted upgrade, reinstall to settle it"),
                        &ours,
                    );
                }
                return Err(Error::Layer(to.clone(), e));
            }
            if is_new {
                created.push(rel.clone());
            }
            written.push(rel.clone());
            done.push(Shadow {
                dll: (*dll).to_string(),
                arch,
                path: to.clone(),
            });
        }

        written.sort();
        written.dedup();

        // The record goes down as soon as the files exist, and describes a
        // superset of them from here on. Everything below can fail - the
        // superseded sweep, reading and rewriting user.reg - and until this
        // was written first, a failure there left libraries in the upper
        // layer that `dxvk` could not see, `--remove` could not remove, and
        // the next install refused as somebody else's, telling the user to
        // move aside files Raven had put there itself. `d3d` already skips
        // manifest entries that are not files, so a superset is harmless.
        let version = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown".into());
        let m = self.d3d_manifest_path(rt);
        let mut described: Vec<String> = written.iter().chain(ours.iter()).cloned().collect();
        described.sort();
        described.dedup();
        write_manifest(&m, &version, &described)?;

        // Installing over an older build is the normal way to update, and
        // upstream drops modules between versions - d3d10.dll went that way.
        // Anything the previous install left that this one does not replace is
        // a stale library still shadowing the real Windows, invisible to
        // `dxvk` and untouched by `--remove`, and pairing an old module with
        // new ones is exactly how a mismatch breaks mysteriously.
        let superseded: Vec<String> = ours
            .iter()
            .filter(|rel| !written.contains(rel))
            .cloned()
            .collect();
        for rel in &superseded {
            let _ = std::fs::remove_file(self.upper().join(rel));
        }

        let reg = self.prefix().join("user.reg");
        let mut text = std::fs::read_to_string(&reg).map_err(|e| Error::Layer(reg.clone(), e))?;
        let keep = unique_dlls(&done);
        for dll in &keep {
            // "native" and not "native,builtin": a fallback to Wine's own
            // implementation would hide a DXVK that failed to load behind a
            // silent performance cliff, which is the opposite of useful.
            text = text::set_value(&text, OVERRIDES, dll, Some("native"));
        }
        for rel in &superseded {
            if let Some(dll) = module_of(rel) {
                if !keep.contains(&dll) {
                    text = text::set_value(&text, OVERRIDES, &dll, None);
                }
            }
        }
        text::write_atomic(&reg, &text)?;

        // Narrowed to exactly what is installed now that the superseded
        // files are gone and the overrides agree with them.
        write_manifest(&m, &version, &written)?;
        Ok(done)
    }

    /// Removes what `install_dxvk` put in, restoring the real Windows by
    /// uncovering it. The base was never touched, so there is nothing to undo
    /// there.
    pub fn remove_d3d(&self, rt: &Runtime) -> Result<usize, Error> {
        self.ensure_not_running()?;
        let mut gone = 0;
        for rel in self.d3d_manifest(rt) {
            if std::fs::remove_file(self.upper().join(&rel)).is_ok() {
                gone += 1;
            }
        }
        let _ = std::fs::remove_file(self.d3d_manifest_path(rt));
        let reg = self.prefix().join("user.reg");
        let mut text = std::fs::read_to_string(&reg).map_err(|e| Error::Layer(reg.clone(), e))?;
        for dll in rt.dlls {
            text = text::set_value(&text, OVERRIDES, dll, None);
        }
        text::write_atomic(&reg, &text)?;
        Ok(gone)
    }

    /// Which DXVK DLLs Raven installed and that are still in place.
    ///
    /// Read from the manifest rather than by scanning for known names: a
    /// `d3d9.dll` some installer dropped into the overlay is not ours to claim,
    /// and certainly not ours to delete.
    pub fn d3d(&self, rt: &Runtime) -> Vec<Shadow> {
        let mut found = Vec::new();
        for rel in self.d3d_manifest(rt) {
            let path = self.upper().join(&rel);
            if !path.is_file() {
                continue;
            }
            let arch = rt
                .arches
                .iter()
                .find(|(_, windir)| rel.starts_with(windir))
                .map(|(a, _)| *a)
                .unwrap_or("?");
            let dll = module_of(&rel).unwrap_or_else(|| rel.clone());
            found.push(Shadow { dll, arch, path });
        }
        found
    }

    /// Where the record of what Raven installed lives.
    fn d3d_manifest_path(&self, rt: &Runtime) -> PathBuf {
        // One manifest per runtime, so removing DXVK cannot take vkd3d's
        // libraries with it.
        self.root.join(format!("{}.files", rt.key))
    }

    /// The upper-layer paths Raven installed, relative to the upper layer.
    fn d3d_manifest(&self, rt: &Runtime) -> Vec<String> {
        std::fs::read_to_string(self.d3d_manifest_path(rt))
            .map(|t| {
                t.lines()
                    .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Which DXVK build is installed, as the release named itself.
    ///
    /// A release carries its version only in its directory name, so that is
    /// what gets recorded - "which DXVK do I have" being the first question
    /// after "is it installed", and the one an update needs answered.
    pub fn d3d_build(&self, rt: &Runtime) -> Option<String> {
        std::fs::read_to_string(self.d3d_manifest_path(rt))
            .ok()?
            .lines()
            .find_map(|l| l.strip_prefix("#build ").map(|v| v.trim().to_string()))
    }

    /// The overrides currently in `user.reg` for DXVK's modules. Reported apart
    /// from the files, because the two halves can disagree - a hand-edited
    /// prefix, or an install that failed between the copy and the registry -
    /// and a status that hides that is worse than no status.
    pub fn d3d_overrides(&self, rt: &Runtime) -> Vec<(String, String)> {
        let Ok(text) = std::fs::read_to_string(self.prefix().join("user.reg")) else {
            return Vec::new();
        };
        text::values(&text, OVERRIDES)
            .into_iter()
            .filter(|(name, _)| rt.dlls.contains(&name.as_str()))
            .collect()
    }
}

/// The module name inside a manifest path: `Windows/System32/d3d11.dll` -> `d3d11`.
fn module_of(rel: &str) -> Option<String> {
    rel.rsplit('/')
        .next()
        .and_then(|f| f.strip_suffix(".dll"))
        .map(String::from)
}

fn unique_dlls(done: &[Shadow]) -> Vec<String> {
    let mut names: Vec<String> = done.iter().map(|s| s.dll.clone()).collect();
    names.sort();
    names.dedup();
    names
}

/// Records which libraries Raven has put in the environment, and which build
/// they came from - "which DXVK do I have" being the first question after
/// "is one installed".
fn write_manifest(path: &std::path::Path, version: &str, files: &[String]) -> Result<(), Error> {
    let body = format!("#build {version}\n{}\n", files.join("\n"));
    std::fs::write(path, body).map_err(|e| Error::Layer(path.to_path_buf(), e))
}

/// A directory holding the DXVK build, plus a guard that deletes it again if we
/// created it by extracting an archive.
///
/// An archive is extracted under Raven's own cache, not `/tmp`: that is
/// shared, and a name another user can predict is a name another user can
/// create first. Extracting into a directory somebody else owns would let them
/// swap the libraries between `tar` and the copy into the upper layer, and
/// those libraries then run inside every Windows program in the environment.
fn unpack(source: &Path) -> Result<(PathBuf, Option<TempDir>), Error> {
    if source.is_dir() {
        return Ok((source.to_path_buf(), None));
    }
    if !source.is_file() {
        return Err(Error::Layer(
            source.to_path_buf(),
            std::io::Error::from(std::io::ErrorKind::NotFound),
        ));
    }
    let (dir, guard) = extract(source, &paths::cache_dir()?.join("unpack"))?;
    Ok((dir, Some(guard)))
}

/// Extracts `archive` into a fresh private directory under `under`. Returns
/// where its contents landed, and the guard that removes them again.
fn extract(archive: &Path, under: &Path) -> Result<(PathBuf, TempDir), Error> {
    let pid = std::process::id();
    let dir = private_dir(under, || {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        format!("d3d-{pid}-{nanos}")
    })?;
    let guard = TempDir(dir.clone());
    // GNU tar applies an archive's `./` entry to the directory it extracts
    // into, mode included, so extracting straight into the private directory
    // would let the archive open it up again. One level down, that entry
    // reaches only a directory nobody else can get to.
    let tree = dir.join("build");
    std::fs::create_dir(&tree).map_err(|e| Error::Layer(tree.clone(), e))?;
    let out = std::process::Command::new("tar")
        // The files are this user's whatever the archive says they belong to.
        .arg("--no-same-owner")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(&tree)
        .output()
        .map_err(|e| Error::Tool("tar", e))?;
    if !out.status.success() {
        return Err(Error::ToolFailed(
            "tar",
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    Ok((tree, guard))
}

/// Creates a directory under `parent` that did not exist before this call and
/// that only this user can enter.
///
/// The create is exclusive, so a directory that is already there - left by a
/// crash, or put there by somebody else - is never adopted; the next name is
/// tried instead.
fn private_dir(parent: &Path, mut name: impl FnMut() -> String) -> Result<PathBuf, Error> {
    use std::os::unix::fs::DirBuilderExt;

    std::fs::create_dir_all(parent).map_err(|e| Error::Layer(parent.to_path_buf(), e))?;
    // Bounded, because a clock that keeps returning a taken name would
    // otherwise spin here for ever.
    let mut retries = 16;
    loop {
        let dir = parent.join(name());
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && retries > 0 => {
                retries -= 1;
            }
            Err(e) => return Err(Error::Layer(dir, e)),
        }
    }
}

/// Whether reaching `path` from `dir` goes through a symbolic link, the file
/// itself included. `path` must lie under `dir`.
fn through_link(dir: &Path, path: &Path) -> bool {
    path.ancestors().take_while(|p| *p != dir).any(|p| {
        p.symlink_metadata()
            .map_or(true, |m| m.file_type().is_symlink())
    })
}

/// Finds the directory that actually holds `x64/`, so both a release tarball
/// (which nests everything under `dxvk-<version>/`) and an already-extracted
/// build work without the caller having to know which they have.
fn build_root(dir: &Path, key: &'static str) -> Result<PathBuf, Error> {
    if dir.join("x64").is_dir() {
        return Ok(dir.to_path_buf());
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.join("x64").is_dir() {
                return Ok(p);
            }
        }
    }
    Err(Error::NotAD3dBuild(key, dir.to_path_buf()))
}

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_root_is_found_through_a_release_wrapper() {
        let dir = std::env::temp_dir().join(format!("raven-dxvkroot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // A release tarball extracts to dxvk-2.7/x64, not to x64.
        std::fs::create_dir_all(dir.join("dxvk-2.7/x64")).unwrap();
        assert_eq!(build_root(&dir, "dxvk").unwrap(), dir.join("dxvk-2.7"));
        // An already-extracted build works too.
        assert_eq!(
            build_root(&dir.join("dxvk-2.7"), "dxvk").unwrap(),
            dir.join("dxvk-2.7")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn something_that_is_not_a_dxvk_build_is_refused_by_name() {
        let dir = std::env::temp_dir().join(format!("raven-dxvkbad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        assert!(matches!(
            build_root(&dir, "dxvk"),
            Err(Error::NotAD3dBuild("dxvk", _))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn each_runtime_covers_what_it_actually_ships() {
        for must in ["d3d9", "d3d11", "dxgi", "d3d10core", "d3d8"] {
            assert!(DXVK.dlls.contains(&must), "{must} missing from DXVK");
        }
        for must in ["d3d12", "d3d12core"] {
            assert!(VKD3D.dlls.contains(&must), "{must} missing from VKD3D");
        }
        // The two must not overlap, or removing one would strip the other's
        // overrides out of user.reg.
        for d in DXVK.dlls {
            assert!(!VKD3D.dlls.contains(d), "{d} claimed by both runtimes");
        }
        // Their manifests must differ for the same reason.
        assert_ne!(DXVK.key, VKD3D.key);
    }

    #[test]
    fn the_thirty_two_bit_directory_is_not_named_the_same_by_both() {
        // The trap this descriptor exists for: DXVK ships x32, vkd3d-proton
        // ships x86, and a hard-coded name silently installs nothing 32-bit.
        assert!(DXVK.arches.iter().any(|(a, _)| *a == "x32"));
        assert!(VKD3D.arches.iter().any(|(a, _)| *a == "x86"));
    }

    #[test]
    fn a_manifest_path_yields_its_module_name() {
        assert_eq!(module_of("Windows/System32/d3d11.dll").unwrap(), "d3d11");
        assert_eq!(
            module_of("Windows/SysWOW64/d3d10_1.dll").unwrap(),
            "d3d10_1"
        );
        assert_eq!(module_of("Windows/System32/notadll"), None);
    }

    /// A scratch directory for one test, and a `.tar` of `tree` built in it.
    fn archive_of(test: &str, tree: impl FnOnce(&Path)) -> (PathBuf, PathBuf) {
        let work = std::env::temp_dir().join(format!("raven-unpack-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work);
        let src = work.join("src");
        std::fs::create_dir_all(&src).unwrap();
        tree(&src);
        let archive = work.join("build.tar");
        let made = std::process::Command::new("tar")
            .arg("-cf")
            .arg(&archive)
            .arg("-C")
            .arg(&src)
            .arg(".")
            .status()
            .unwrap();
        assert!(made.success());
        (work, archive)
    }

    #[test]
    fn an_archive_unpacks_into_a_directory_only_its_user_can_enter() {
        use std::os::unix::fs::PermissionsExt;

        let (work, archive) = archive_of("private", |src| {
            std::fs::create_dir_all(src.join("dxvk-2.7/x64")).unwrap();
            std::fs::write(src.join("dxvk-2.7/x64/d3d11.dll"), "lib").unwrap();
            // The archive's `./` entry asks for a world-writable directory,
            // and tar honours that on whatever it extracts into.
            std::fs::set_permissions(src, std::fs::Permissions::from_mode(0o777)).unwrap();
        });
        let (tree, guard) = extract(&archive, &work.join("unpack")).unwrap();
        let private = tree.parent().unwrap().to_path_buf();
        assert_eq!(private.parent().unwrap(), work.join("unpack"));
        let mode = std::fs::metadata(&private).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "{mode:o}");
        assert!(tree.join("dxvk-2.7/x64/d3d11.dll").is_file());
        drop(guard);
        assert!(!private.exists(), "the guard takes the unpacked build away");
        let _ = std::fs::remove_dir_all(&work);
    }

    #[test]
    fn a_directory_already_at_the_chosen_name_is_never_reused() {
        let parent = std::env::temp_dir().join(format!("raven-taken-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&parent);
        // Somebody got there first, and left something in it.
        std::fs::create_dir_all(parent.join("taken")).unwrap();
        std::fs::write(parent.join("taken/d3d11.dll"), "planted").unwrap();

        let mut names = ["taken", "fresh"].into_iter();
        let dir = private_dir(&parent, || names.next().unwrap().to_string()).unwrap();
        assert_eq!(dir, parent.join("fresh"));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        assert!(parent.join("taken/d3d11.dll").is_file(), "left alone");

        // And a name that never stops being taken ends in an error, not a
        // loop.
        assert!(matches!(
            private_dir(&parent, || "taken".to_string()),
            Err(Error::Layer(_, e)) if e.kind() == std::io::ErrorKind::AlreadyExists
        ));
        let _ = std::fs::remove_dir_all(&parent);
    }

    #[test]
    fn a_symlinked_dll_in_an_archive_is_rejected() {
        let (work, archive) = archive_of("link", |src| {
            let x64 = src.join("dxvk-2.7/x64");
            std::fs::create_dir_all(&x64).unwrap();
            std::fs::write(x64.join("dxgi.dll"), "lib").unwrap();
            // Pointing out of the archive, at something that is not a DXVK
            // library at all.
            std::os::unix::fs::symlink("/etc/hostname", x64.join("d3d11.dll")).unwrap();
            // A linked directory reaches outside just as well.
            std::os::unix::fs::symlink("/usr/lib", src.join("dxvk-2.7/x32")).unwrap();
        });
        let (dir, _guard) = extract(&archive, &work.join("unpack")).unwrap();
        let root = build_root(&dir, "dxvk").unwrap();
        assert!(through_link(&dir, &root.join("x64/d3d11.dll")));
        assert!(through_link(&dir, &root.join("x32/d3d11.dll")));
        assert!(!through_link(&dir, &root.join("x64/dxgi.dll")));
        let _ = std::fs::remove_dir_all(&work);
    }

    #[test]
    fn duplicate_architectures_collapse_to_one_override_each() {
        let done = vec![
            Shadow {
                dll: "d3d11".into(),
                arch: "x64",
                path: "a".into(),
            },
            Shadow {
                dll: "d3d11".into(),
                arch: "x32",
                path: "b".into(),
            },
            Shadow {
                dll: "dxgi".into(),
                arch: "x64",
                path: "c".into(),
            },
        ];
        assert_eq!(unique_dlls(&done), vec!["d3d11", "dxgi"]);
    }
}
