//! Raven runs Windows programs against a real Windows installation mounted as C:.
//!
//! Everything Raven can do lives here as a library. The command-line interface in
//! `cli/` is a shell over this API and holds no logic of its own, so that a
//! graphical front end is a second caller rather than a rewrite.

pub mod attach;
pub mod base;
pub mod d3d;
pub mod env;
pub mod i18n;
pub mod launch;
pub mod layer;
pub mod mount;
pub mod paths;
pub mod prefix;
pub mod registry;
pub mod session;

/// Failures that are Raven's own, as opposed to the kernel's.
///
/// The words are in `i18n/`, one `error_<variant>` key per variant in English
/// and French. `Display` is the English, for logs; [`i18n::error_message`] is
/// what a user is shown.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    NoHome,
    NoRuntimeDir,
    BadName(String),
    Layer(std::path::PathBuf, #[source] std::io::Error),
    NoSuchBase(String),
    BaseExists(String),
    NotAWindowsBase(String),
    NoSuchEnvironment(String),
    EnvironmentExists(String),
    NoWine,
    Tool(&'static str, #[source] std::io::Error),
    ToolFailed(&'static str, String),
    Manifest(std::path::PathBuf, String),
    Hive(String),
    NoDefaultEnvironment,
    EnvironmentBusy { name: String, holders: String },
    StillHeld(String, String),
    BadLetter(char),
    NotABlockDevice(std::path::PathBuf),
    AlreadyAttached(char),
    LetterTaken(char),
    DeviceAttached(std::path::PathBuf, char),
    NotAttached(char),
    NotAD3dBuild(&'static str, std::path::PathBuf),
    SessionFailed(String),
    SessionHolds(String),
    WineChanged(String),
    D3dWouldOverwrite(std::path::PathBuf),
    D3dLink(std::path::PathBuf),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&i18n::error_message_in(i18n::Lang::En, self))
    }
}
