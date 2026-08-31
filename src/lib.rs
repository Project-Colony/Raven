//! Raven runs Windows programs against a real Windows installation mounted as C:.
//!
//! Everything Raven can do lives here as a library. The command-line interface in
//! `main.rs` is a shell over this API and holds no logic of its own, so that a
//! graphical front end is a second caller rather than a rewrite.

pub mod layer;
pub mod mount;
pub mod paths;

/// Failures that are Raven's own, as opposed to the kernel's.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HOME is not set, so the user's directories cannot be located")]
    NoHome,

    #[error("XDG_RUNTIME_DIR is not set; Raven needs it to place the mount point")]
    NoRuntimeDir,

    #[error("{0:?} is not a usable environment name")]
    BadName(String),

    #[error("preparing the layer failed at {0}")]
    Layer(std::path::PathBuf, #[source] std::io::Error),
}
