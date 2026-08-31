//! Creating the Wine prefix that ties an environment together.
//!
//! Wine's prefix holds the registry and the drive mapping. Raven keeps the
//! registry Wine writes — the projection from the real hives is merged into it
//! rather than replacing it — and takes over only `dosdevices/c:`, pointing it
//! at the overlay mount instead of at a directory Wine owns.

use std::path::Path;
use std::process::Command;

use crate::Error;

/// Builds a fresh Wine prefix at `prefix`.
///
/// The Mono and Gecko installers are suppressed: they open dialogs, they need
/// the network, and nothing Raven does at this stage depends on them.
pub fn create(prefix: &Path) -> Result<(), Error> {
    let status = Command::new("wineboot")
        .arg("-u")
        .env("WINEPREFIX", prefix)
        .env("WINEDLLOVERRIDES", "mscoree,mshtml=")
        .env("WINEDEBUG", "-all")
        .status()
        .map_err(|e| Error::Tool("wineboot", e))?;
    if !status.success() {
        return Err(Error::ToolFailed(
            "wineboot",
            format!("exited with {status}"),
        ));
    }
    // wineserver lingers and holds the prefix open; a later step that moves
    // drive_c out from under it would race with a live server.
    let _ = Command::new("wineserver")
        .arg("-w")
        .env("WINEPREFIX", prefix)
        .status();
    Ok(())
}

/// Points the prefix's C: drive at `target`.
///
/// Wine follows `dosdevices/c:` wherever it leads, which is what lets a mount
/// stand in for the directory Wine would otherwise have created.
pub fn point_c_drive(prefix: &Path, target: &Path) -> Result<(), Error> {
    let link = prefix.join("dosdevices/c:");
    if link.exists() || link.is_symlink() {
        std::fs::remove_file(&link).map_err(|e| Error::Layer(link.clone(), e))?;
    }
    std::os::unix::fs::symlink(target, &link).map_err(|e| Error::Layer(link, e))
}

/// Whether Wine is installed at all, checked before a long operation rather
/// than after it.
pub fn wine_available() -> bool {
    Command::new("wine")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}
