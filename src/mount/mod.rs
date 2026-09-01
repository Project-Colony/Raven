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

/// The layers making up an environment's C: drive.
///
/// `lower` holds the read-only layers, **highest priority first**. overlayfs
/// gives the leftmost `lowerdir` precedence, and that ordering is what makes the
/// design work: with Wine's skeleton first and the real Windows second, Wine's
/// files win wherever it has one and Microsoft's show through everywhere else.
/// That is the shadow set, expressed as a filesystem layer.
///
/// No layer in `lower` is ever written to. `upper` receives every write the
/// running program makes, and `work` is overlayfs's own scratch area, which must
/// sit on the same filesystem as `upper`.
#[derive(Debug, Clone)]
pub struct OverlaySpec {
    pub lower: Vec<PathBuf>,
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

    #[error("at least one read-only layer is required")]
    NoLowerLayer,
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
        if self.lower.is_empty() {
            return Err(MountError::NoLowerLayer);
        }
        for p in self
            .lower
            .iter()
            .chain([&self.upper, &self.work, &self.target])
        {
            if !p.exists() {
                return Err(MountError::MissingPath(p.clone()));
            }
        }
        Ok(())
    }

    /// The option string overlayfs expects.
    ///
    /// `userxattr` is set because Raven always mounts unprivileged. Without it,
    /// overlayfs looks for its whiteout and opaque markers in `trusted.*`
    /// extended attributes, which an unprivileged process cannot set; with it,
    /// they live in `user.*` and become usable. That is what allows a layer to
    /// *hide* part of the real Windows rather than only add to it.
    pub(crate) fn options(&self) -> String {
        let lower: Vec<String> = self.lower.iter().map(|p| escape(p)).collect();
        format!(
            "userxattr,lowerdir={},upperdir={},workdir={}",
            lower.join(":"),
            escape(&self.upper),
            escape(&self.work),
        )
    }
}

/// overlayfs separates options with commas and layers with colons, and escapes
/// with a backslash. A path containing either character silently corrupts the
/// option string unless it is escaped — a comma truncates a path, and a colon
/// splits one layer into two that do not exist.
///
/// `pub(crate)` because this escaped form is also what the kernel *stores*:
/// anything comparing against `/proc/<pid>/mountinfo` must start from it,
/// not from the raw path.
pub(crate) fn escape(p: &Path) -> String {
    p.to_string_lossy()
        .replace('\\', r"\\")
        .replace(',', r"\,")
        .replace(':', r"\:")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(lower: &[&str]) -> OverlaySpec {
        OverlaySpec {
            lower: lower.iter().map(PathBuf::from).collect(),
            upper: PathBuf::from("/u"),
            work: PathBuf::from("/w"),
            target: PathBuf::from("/t"),
        }
    }

    #[test]
    fn layers_are_joined_in_priority_order() {
        assert_eq!(
            spec(&["/wine", "/windows"]).options(),
            "userxattr,lowerdir=/wine:/windows,upperdir=/u,workdir=/w",
            "the leftmost layer must stay leftmost - it is the one that wins"
        );
    }

    #[test]
    fn options_escape_the_characters_overlayfs_treats_specially() {
        assert_eq!(
            spec(&["/a,b", "/c:d"]).options(),
            r"userxattr,lowerdir=/a\,b:/c\:d,upperdir=/u,workdir=/w",
            "a raw comma truncates a path and a raw colon splits one layer into two"
        );
    }

    #[test]
    fn check_names_the_missing_path() {
        let mut s = spec(&["/definitely/not/here"]);
        s.upper = PathBuf::from("/tmp");
        s.work = PathBuf::from("/tmp");
        s.target = PathBuf::from("/tmp");
        let err = s.check().unwrap_err();
        assert!(matches!(err, MountError::MissingPath(p) if p.ends_with("here")));
    }

    #[test]
    fn a_spec_with_no_read_only_layer_is_refused() {
        assert!(matches!(
            spec(&[]).check().unwrap_err(),
            MountError::NoLowerLayer
        ));
    }
}
