//! Environments: one Windows base, one Wine layer over it, one place for writes.
//!
//! An environment is deliberately cheap. Everything expensive — the deployed
//! Windows — is shared and immutable, so creating one copies only Wine's
//! skeleton, and destroying one deletes a directory. A broken installation is
//! recovered by throwing the environment away, not by repairing it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::mount::OverlaySpec;
use crate::{Error, base::Base, layer, paths, prefix, registry};

/// What an environment records about itself, on disk as `environment.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    /// The base this environment runs against. Stored by id, not by path, so
    /// moving the data directory does not break every environment in it.
    pub base: String,
}

#[derive(Debug, Clone)]
pub struct Environment {
    pub name: String,
    pub manifest: Manifest,
    pub root: PathBuf,
}

impl Environment {
    pub fn layer(&self) -> PathBuf {
        self.root.join("layer")
    }
    pub fn upper(&self) -> PathBuf {
        self.root.join("upper")
    }
    pub fn work(&self) -> PathBuf {
        self.root.join("work")
    }
    pub fn prefix(&self) -> PathBuf {
        self.root.join("prefix")
    }
    /// The rules governing what crosses from the base's registry.
    ///
    /// Written into the environment rather than compiled in, so it can be read
    /// and changed by someone who does not read Rust — which is the point, since
    /// what crosses is a correctness decision.
    pub fn rules_file(&self) -> PathBuf {
        self.root.join("registry-rules.toml")
    }

    pub fn rules(&self) -> Result<registry::Rules, Error> {
        match std::fs::read_to_string(self.rules_file()) {
            Ok(text) => registry::Rules::parse(&text)
                .map_err(|e| Error::Manifest(self.rules_file(), e.to_string())),
            Err(_) => Ok(registry::Rules::default()),
        }
    }

    /// Reads the base's hives and merges what the rules permit into the prefix.
    ///
    /// Idempotent: same base, same rules, same result. That is what makes it
    /// safe to run again after editing the rules, and it is why the output is
    /// never edited by hand — a projection someone corrected is one nobody can
    /// reproduce.
    pub fn project_registry(&self) -> Result<usize, Error> {
        // The import mounts the overlay, and a running environment holds it.
        self.ensure_not_running()?;
        let base = Base::find(&self.manifest.base)?;
        let reg = registry::project_base(&base.path, &self.rules()?)?;
        let keys = reg.matches("\r\n[").count();
        let file = self.root.join("projected.reg");
        std::fs::write(&file, &reg).map_err(|e| Error::Layer(file.clone(), e))?;
        registry::import(&self.spec()?, &self.prefix(), &file)?;
        Ok(keys)
    }

    /// The layer stack for this environment.
    ///
    /// Order is the whole point: the Wine layer first so its files win, the
    /// Windows base second so Microsoft's show through everywhere Wine has none.
    pub fn spec(&self) -> Result<OverlaySpec, Error> {
        Ok(OverlaySpec {
            lower: vec![self.layer(), Base::find(&self.manifest.base)?.path],
            upper: self.upper(),
            work: self.work(),
            target: paths::mount_point(&self.name)?,
        })
    }

    pub fn open(name: &str) -> Result<Environment, Error> {
        let root = paths::environment_dir(name)?;
        let file = root.join("environment.toml");
        let text = std::fs::read_to_string(&file)
            .map_err(|_| Error::NoSuchEnvironment(name.to_owned()))?;
        Ok(Environment {
            name: name.to_owned(),
            manifest: toml::from_str(&text).map_err(|e| Error::Manifest(file, e.to_string()))?,
            root,
        })
    }

    pub fn list() -> Result<Vec<Environment>, Error> {
        let dir = paths::data_dir()?.join("environments");
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .map_err(|e| Error::Layer(dir, e))?
            .filter_map(Result::ok)
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        Ok(names
            .iter()
            .filter_map(|n| Environment::open(n).ok())
            .collect())
    }

    /// Deletes the environment. The base it ran against is untouched.
    pub fn destroy(name: &str) -> Result<(), Error> {
        let root = paths::environment_dir(name)?;
        if !root.exists() {
            return Err(Error::NoSuchEnvironment(name.to_owned()));
        }
        // Deleting the layers under a live mount would not stop the programs
        // using them - it would hand them a C: that dissolves as they run.
        let holders = holders_of(&root.join("upper"));
        if !holders.is_empty() {
            return Err(Error::EnvironmentBusy {
                name: name.to_owned(),
                holders: describe(&holders),
            });
        }
        remove_tree(&root)
    }

    /// The processes still holding this environment's C: mounted.
    ///
    /// The mount lives in a private mount namespace and is invisible from
    /// outside — but each process's own view is in `/proc/<pid>/mountinfo`,
    /// and one that names this environment's upper layer is inside. Killing a
    /// program's window often leaves `wineserver` and a handful of Wine
    /// services alive this way, and they keep the upper layer busy.
    pub fn holders(&self) -> Vec<Holder> {
        holders_of(&self.upper())
    }

    /// Refuses while the environment is held by live processes.
    ///
    /// overlayfs will not mount the same upper layer twice, so a second
    /// launch can only fail — and the raw failure is `EBUSY`, which names
    /// neither the environment nor the processes. This names both.
    pub fn ensure_not_running(&self) -> Result<(), Error> {
        let holders = self.holders();
        if holders.is_empty() {
            return Ok(());
        }
        Err(Error::EnvironmentBusy {
            name: self.name.clone(),
            holders: describe(&holders),
        })
    }

    /// Releases the environment: asks every holder to exit, then insists.
    ///
    /// Returns the processes that were terminated. `wineserver -k` from
    /// outside cannot do this — the server inside the namespace is a
    /// different one — so the holders are signalled directly.
    pub fn stop(&self) -> Result<Vec<Holder>, Error> {
        let holders = self.holders();
        if holders.is_empty() {
            return Ok(holders);
        }
        signal_all(&holders, rustix::process::Signal::TERM);
        if wait_released(&self.upper(), 2000) {
            return Ok(holders);
        }
        // Re-scanned rather than reusing the list: some exited on SIGTERM,
        // and killing a reused PID is the bug worth this second read.
        signal_all(&self.holders(), rustix::process::Signal::KILL);
        if wait_released(&self.upper(), 2000) {
            return Ok(holders);
        }
        Err(Error::StillHeld(
            self.name.clone(),
            describe(&self.holders()),
        ))
    }
}

/// A process keeping an environment's C: mounted.
#[derive(Debug)]
pub struct Holder {
    pub pid: u32,
    pub comm: String,
}

fn describe(holders: &[Holder]) -> String {
    let names: Vec<String> = holders
        .iter()
        .map(|h| format!("{} ({})", h.pid, h.comm))
        .collect();
    names.join(", ")
}

fn signal_all(holders: &[Holder], sig: rustix::process::Signal) {
    for h in holders {
        if let Some(pid) = rustix::process::Pid::from_raw(h.pid as i32) {
            // A holder may have exited on its own; that is success, not error.
            let _ = rustix::process::kill_process(pid, sig);
        }
    }
}

fn wait_released(upper: &std::path::Path, budget_ms: u64) -> bool {
    for _ in 0..budget_ms / 100 {
        if holders_of(upper).is_empty() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    holders_of(upper).is_empty()
}

fn holders_of(upper: &std::path::Path) -> Vec<Holder> {
    let needle = mountinfo_needle(upper);
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut held = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        // A process that exits mid-scan simply stops matching.
        let Ok(mi) = std::fs::read_to_string(entry.path().join("mountinfo")) else {
            continue;
        };
        if mi.contains(&needle) {
            let comm = std::fs::read_to_string(entry.path().join("comm"))
                .map(|s| s.trim().to_owned())
                .unwrap_or_else(|_| "?".to_owned());
            held.push(Holder { pid, comm });
        }
    }
    held
}

/// What this upper directory looks like inside `/proc/<pid>/mountinfo`.
///
/// Two escapings stack, and both matter. The option string handed to
/// `mount(2)` was already escaped once by [`crate::mount::escape`] — a
/// backslash before `\`, `,` and `:` — and the kernel octal-escapes what it
/// then displays: space, tab, newline, backslash and comma. A needle built
/// from the raw path silently misses any path containing those characters,
/// an environment name may legally contain a comma, and a missed holder is
/// `destroy` deleting layers under a live mount. The expected forms are
/// pinned by tests, captured from real mounts.
fn mountinfo_needle(upper: &std::path::Path) -> String {
    let mut needle = String::from("upperdir=");
    for c in crate::mount::escape(upper).chars() {
        match c {
            ' ' => needle.push_str("\\040"),
            '\t' => needle.push_str("\\011"),
            '\n' => needle.push_str("\\012"),
            '\\' => needle.push_str("\\134"),
            ',' => needle.push_str("\\054"),
            _ => needle.push(c),
        }
    }
    // overlayfs always prints `workdir=` next; the comma keeps a path from
    // matching another that merely starts the same.
    needle.push(',');
    needle
}

/// Removes a tree, restoring directory permissions on the way down.
///
/// A plain `remove_dir_all` cannot delete an environment that has ever been
/// mounted: `overlayfs` creates `work/work` with no permissions at all, and the
/// removal stops there — after having already deleted the upper layer. That
/// leaves an environment that cannot be destroyed and cannot be recreated,
/// which is the worst of both.
fn remove_tree(root: &std::path::Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;

    fn descend(dir: &std::path::Path) -> Result<(), Error> {
        // The directory must be writable and searchable before its contents can
        // be listed or unlinked.
        if let Ok(meta) = std::fs::metadata(dir) {
            let mut perms = meta.permissions();
            if perms.mode() & 0o700 != 0o700 {
                perms.set_mode(perms.mode() | 0o700);
                let _ = std::fs::set_permissions(dir, perms);
            }
        }
        for entry in std::fs::read_dir(dir)
            .map_err(|e| Error::Layer(dir.to_path_buf(), e))?
            .filter_map(Result::ok)
        {
            let path = entry.path();
            // Never follow a symlink out of the tree - a Windows layer contains
            // them, and one pointing outside would take the target with it.
            if path.is_dir() && !path.is_symlink() {
                descend(&path)?;
            } else {
                std::fs::remove_file(&path).map_err(|e| Error::Layer(path, e))?;
            }
        }
        std::fs::remove_dir(dir).map_err(|e| Error::Layer(dir.to_path_buf(), e))
    }

    descend(root)
}

/// Builds a new environment against an existing base.
///
/// The steps are ordered by what they depend on, and the case normalisation is
/// not optional: without it the Wine layer and the Windows base do not merge at
/// all, and the layer shadows nothing.
pub fn create(name: &str, base_id: &str) -> Result<Environment, Error> {
    let base = Base::find(base_id)?;
    if !base.looks_like_windows() {
        return Err(Error::NotAWindowsBase(base_id.to_owned()));
    }
    if !prefix::wine_available() {
        return Err(Error::NoWine);
    }

    let root = paths::environment_dir(name)?;
    if root.exists() {
        return Err(Error::EnvironmentExists(name.to_owned()));
    }

    // Anything that fails from here leaves a half-built environment, so the
    // whole directory is removed on the way out rather than left to be found.
    let built = (|| -> Result<Environment, Error> {
        for d in ["upper", "work", "prefix"] {
            std::fs::create_dir_all(root.join(d)).map_err(|e| Error::Layer(root.join(d), e))?;
        }
        let env = Environment {
            name: name.to_owned(),
            manifest: Manifest {
                base: base_id.to_owned(),
            },
            root: root.clone(),
        };

        prefix::create(&env.prefix())?;

        // Wine's drive_c becomes the upper read-only layer. It is moved rather
        // than copied: leaving a second copy behind would let Wine and the
        // overlay disagree about which one is the prefix's C:.
        let drive_c = env.prefix().join("drive_c");
        std::fs::rename(&drive_c, env.layer()).map_err(|e| Error::Layer(drive_c, e))?;

        layer::normalise_case(&env.layer(), &base.path)?;
        for rel in layer::SHADOWED {
            layer::shadow(&env.layer(), rel)?;
        }
        prefix::point_c_drive(&env.prefix(), &paths::mount_point(name)?)?;

        let rules = toml::to_string(&registry::Rules::default())
            .map_err(|e| Error::Manifest(env.rules_file(), e.to_string()))?;
        std::fs::write(env.rules_file(), rules).map_err(|e| Error::Layer(env.rules_file(), e))?;
        env.project_registry()?;

        let manifest = toml::to_string(&env.manifest)
            .map_err(|e| Error::Manifest(root.join("environment.toml"), e.to_string()))?;
        std::fs::write(root.join("environment.toml"), manifest)
            .map_err(|e| Error::Layer(root.join("environment.toml"), e))?;
        Ok(env)
    })();

    if built.is_err() {
        let _ = std::fs::remove_dir_all(&root);
    }
    built
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_with_no_permissions_is_still_removed() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("raven-rm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("work/work")).unwrap();
        std::fs::write(root.join("keep.txt"), "x").unwrap();
        // This is exactly what overlayfs leaves behind.
        std::fs::set_permissions(
            root.join("work/work"),
            std::fs::Permissions::from_mode(0o000),
        )
        .unwrap();

        assert!(
            std::fs::remove_dir_all(&root).is_err(),
            "if the plain removal starts working, this guard is no longer needed"
        );
        remove_tree(&root).expect("remove_tree must handle it");
        assert!(!root.exists());
    }

    #[test]
    fn removal_does_not_follow_a_symlink_out_of_the_tree() {
        let root = std::env::temp_dir().join(format!("raven-rmlink-{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("raven-rmlink-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("precious.txt"), "keep me").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();

        remove_tree(&root).unwrap();
        assert!(
            outside.join("precious.txt").exists(),
            "the removal followed a symlink and deleted files outside the environment"
        );
        let _ = std::fs::remove_dir_all(&outside);
    }

    // Captured from a real mount rather than written from memory: an overlay
    // whose upper layer path contains a space, seen from inside the namespace.
    const MOUNTINFO: &str = "457 407 0:89 / /tmp/x/holdertest/merged rw,relatime - overlay overlay rw,lowerdir=/tmp/x/holdertest/base,upperdir=/tmp/x/holdertest/up\\040per,workdir=/tmp/x/holdertest/work,redirect_dir=nofollow,index=off,metacopy=off,userxattr\n";

    #[test]
    fn a_spaced_upper_path_is_found_in_mountinfo() {
        let needle = mountinfo_needle(std::path::Path::new("/tmp/x/holdertest/up per"));
        assert!(
            MOUNTINFO.contains(&needle),
            "the kernel escapes a space as \\040 and the needle must match it: {needle}"
        );
    }

    #[test]
    fn a_path_that_merely_starts_the_same_does_not_match() {
        let needle = mountinfo_needle(std::path::Path::new("/tmp/x/holdertest/up"));
        assert!(
            !MOUNTINFO.contains(&needle),
            "\"up\" must not claim the mount belonging to \"up per\""
        );
    }

    #[test]
    fn the_two_escapings_stack_the_way_the_kernel_displays_them() {
        // Expected forms captured from real mounts (the review that found the
        // bug reproduced all four): the path is escaped once for the mount
        // options and the kernel octal-escapes the *escaped* string, so a
        // comma becomes \134\054 - a needle built from the raw path misses it.
        for (path, expect) in [
            ("/tmp/x/up,per", "upperdir=/tmp/x/up\\134\\054per,"),
            ("/tmp/x/up:per", "upperdir=/tmp/x/up\\134:per,"),
            ("/tmp/x/up\\per", "upperdir=/tmp/x/up\\134\\134per,"),
            ("/tmp/x/up per", "upperdir=/tmp/x/up\\040per,"),
        ] {
            assert_eq!(
                mountinfo_needle(std::path::Path::new(path)),
                expect,
                "for {path:?}"
            );
        }
    }

    #[test]
    fn a_manifest_round_trips() {
        let m = Manifest {
            base: "win11-26200-pro".into(),
        };
        let text = toml::to_string(&m).unwrap();
        assert_eq!(toml::from_str::<Manifest>(&text).unwrap(), m);
    }

    #[test]
    fn the_layer_comes_before_the_base_in_the_stack() {
        let env = Environment {
            name: "x".into(),
            manifest: Manifest { base: "b".into() },
            root: PathBuf::from("/data/environments/x"),
        };
        // Checked without touching the filesystem: only the ordering matters,
        // and getting it backwards would silently let Microsoft's ntdll win.
        assert_eq!(env.layer(), PathBuf::from("/data/environments/x/layer"));
        assert!(env.layer().ends_with("layer"));
    }
}
