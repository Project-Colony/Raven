//! Acquiring a writable C: drive backed by an immutable Windows base.
//!
//! The base is never written to. Writes land in an overlay upper layer, which is
//! what makes a base shareable between environments and an environment
//! disposable by deleting a directory.
//!
//! There is deliberately an interface here rather than a direct call to
//! `unshare` and `mount`. Unprivileged user namespaces are exactly the feature
//! hardened kernels disable — `linux-hardened`, Ubuntu's AppArmor policy — so
//! more than one backend will eventually exist. Only [`UserNsOverlay`] is
//! implemented; the others are additions behind this trait rather than a rewrite
//! of everything that mounts.

mod userns;

pub use userns::UserNsOverlay;

use std::path::{Path, PathBuf};

/// Where the three layers of an environment's C: drive live.
///
/// `base` is mounted read-only and is never written to by any backend. `upper`
/// receives every write the running program makes. `work` is overlayfs's own
/// scratch area and must sit on the same filesystem as `upper`.
#[derive(Debug, Clone)]
pub struct OverlaySpec {
    pub base: PathBuf,
    pub upper: PathBuf,
    pub work: PathBuf,
    pub target: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum MountError {
    #[error("unprivileged user namespaces are unavailable on this kernel: {0}")]
    NoUserNamespace(#[source] std::io::Error),

    #[error("could not map this user inside the namespace: {0}")]
    IdMap(#[source] std::io::Error),

    #[error("mounting the overlay failed: {0}")]
    Mount(#[source] std::io::Error),

    #[error("{0} does not exist")]
    MissingPath(PathBuf),
}

/// A way of assembling [`OverlaySpec`] into a mounted C: drive.
///
/// Implementations mount into the *calling process's* mount namespace. The
/// caller is expected to be a process that is about to become the program being
/// launched, so the mount is destroyed with the process tree that owns it and a
/// crash leaves nothing behind to clean up.
pub trait MountBackend {
    /// Whether this backend can work on the running system, cheaply enough to
    /// call before choosing one.
    fn is_available() -> bool
    where
        Self: Sized;

    /// A name for diagnostics. Users need to be told which backend was chosen
    /// and why the others were not.
    fn name(&self) -> &'static str;

    fn mount(&self, spec: &OverlaySpec) -> Result<(), MountError>;
}

impl OverlaySpec {
    /// Fails when a path is missing rather than letting the kernel report a
    /// bare `ENOENT` that names nothing.
    pub(crate) fn check(&self) -> Result<(), MountError> {
        for p in [&self.base, &self.upper, &self.work, &self.target] {
            if !p.exists() {
                return Err(MountError::MissingPath(p.clone()));
            }
        }
        Ok(())
    }

    /// The comma-separated option string overlayfs expects.
    pub(crate) fn options(&self) -> String {
        format!(
            "lowerdir={},upperdir={},workdir={}",
            escape(&self.base),
            escape(&self.upper),
            escape(&self.work),
        )
    }
}

/// overlayfs separates its options with commas and escapes with a backslash, so
/// a path containing a comma corrupts the option string unless it is escaped.
fn escape(p: &Path) -> String {
    p.to_string_lossy().replace('\\', r"\\").replace(',', r"\,")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_escape_commas_in_paths() {
        let spec = OverlaySpec {
            base: PathBuf::from("/a,b"),
            upper: PathBuf::from("/u"),
            work: PathBuf::from("/w"),
            target: PathBuf::from("/t"),
        };
        assert_eq!(
            spec.options(),
            r"lowerdir=/a\,b,upperdir=/u,workdir=/w",
            "an unescaped comma would silently truncate the lowerdir"
        );
    }

    #[test]
    fn check_names_the_missing_path() {
        let spec = OverlaySpec {
            base: PathBuf::from("/definitely/not/here"),
            upper: PathBuf::from("/tmp"),
            work: PathBuf::from("/tmp"),
            target: PathBuf::from("/tmp"),
        };
        let err = spec.check().unwrap_err();
        assert!(matches!(err, MountError::MissingPath(p) if p.ends_with("here")));
    }
}
