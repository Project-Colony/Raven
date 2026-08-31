//! Raven runs Windows programs against a real Windows installation mounted as C:.
//!
//! Everything Raven can do lives here as a library. The command-line interface in
//! `main.rs` is a shell over this API and holds no logic of its own, so that a
//! graphical front end is a second caller rather than a rewrite.

pub mod base;
pub mod env;
pub mod layer;
pub mod mount;
pub mod paths;
pub mod prefix;
pub mod registry;

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

    #[error("no base called {0:?}; run `raven base list`")]
    NoSuchBase(String),

    #[error("a base called {0:?} already exists, and bases are immutable")]
    BaseExists(String),

    #[error(
        "{0:?} does not look like a Windows installation - no Windows/System32/config/SOFTWARE"
    )]
    NotAWindowsBase(String),

    #[error("no environment called {0:?}; run `raven env list`")]
    NoSuchEnvironment(String),

    #[error("an environment called {0:?} already exists")]
    EnvironmentExists(String),

    #[error("Wine is not installed, or not on PATH")]
    NoWine,

    #[error("could not run {0}")]
    Tool(&'static str, #[source] std::io::Error),

    #[error("{0} failed: {1}")]
    ToolFailed(&'static str, String),

    #[error("{0} is not a readable manifest: {1}")]
    Manifest(std::path::PathBuf, String),

    #[error("could not read the registry hive: {0}")]
    Hive(String),
}
