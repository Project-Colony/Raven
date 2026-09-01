//! Raven runs Windows programs against a real Windows installation mounted as C:.
//!
//! Everything Raven can do lives here as a library. The command-line interface in
//! `main.rs` is a shell over this API and holds no logic of its own, so that a
//! graphical front end is a second caller rather than a rewrite.

pub mod attach;
pub mod base;
pub mod d3d;
pub mod env;
pub mod launch;
pub mod layer;
pub mod mount;
pub mod paths;
pub mod prefix;
pub mod registry;
pub mod session;

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

    #[error("no environment given and no default set; run `raven env default <name>`")]
    NoDefaultEnvironment,

    #[error(
        "environment {name:?} is still running - held by {holders}\n  \
         see them: raven env status {name}\n  \
         stop them: raven env stop {name}"
    )]
    EnvironmentBusy { name: String, holders: String },

    #[error(
        "environment {0:?} is still held even after SIGKILL, by {1} - \
         a process in uninterruptible sleep cannot be killed; check `ps`"
    )]
    StillHeld(String, String),

    #[error("{0:?} is not a usable drive letter - use a single letter from d to z")]
    BadLetter(char),

    #[error(
        "{0} is not a block device - attaching anything else would hand a program raw access to the wrong thing"
    )]
    NotABlockDevice(std::path::PathBuf),

    #[error("drive {0}: already has a device attached; detach it first")]
    AlreadyAttached(char),

    #[error(
        "drive {0}: is already mapped in this environment - not by attach, \
         so not Raven's to overwrite; pick a free letter with --letter"
    )]
    LetterTaken(char),

    #[error("{0} is already attached as drive {1}:")]
    DeviceAttached(std::path::PathBuf, char),

    #[error("drive {0}: has no device attached")]
    NotAttached(char),

    #[error(
        "{1} does not look like a {0} build - expected a directory holding \
         x64/ (and a 32-bit directory beside it), or a release archive of one"
    )]
    NotAD3dBuild(&'static str, std::path::PathBuf),

    #[error("could not start a session for the environment: {0}")]
    SessionFailed(String),

    #[error(
        "environment {0:?} has a live session holding its C:, which is what \
         makes launches fast\n  \
         release it: raven env stop {0}"
    )]
    SessionHolds(String),

    #[error(
        "{0} is already in this environment and Raven did not put it there - \
         installing would overwrite it and removing would delete it; move it \
         aside first if you want this runtime to take over"
    )]
    D3dWouldOverwrite(std::path::PathBuf),
}
