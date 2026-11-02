//! Creating the Wine prefix that ties an environment together.
//!
//! Wine's prefix holds the registry and the drive mapping. Raven keeps the
//! registry Wine writes - the projection from the real hives is merged into it
//! rather than replacing it - and takes over only `dosdevices/c:`, pointing it
//! at the overlay mount instead of at a directory Wine owns.

use std::path::Path;
use std::process::Command;

use crate::Error;

/// Builds a fresh Wine prefix at `prefix`, reporting `version` if one is given.
///
/// The Mono and Gecko installers are suppressed: they open dialogs, they need
/// the network, and nothing Raven does at this stage depends on them.
///
/// `version` is one of Wine's own names for a Windows, as `winecfg` takes it;
/// see [`wine_version_for`]. It is set here, while the prefix still has Wine's
/// own `drive_c`, because `winecfg` is a Windows program and needs a
/// `C:\Windows\System32` to start from, which later exists only while the
/// environment is mounted.
pub fn create(prefix: &Path, version: Option<&str>) -> Result<(), Error> {
    // `wine wineboot` rather than a bare `wineboot`: the latter is a wrapper
    // script that a distribution's wine package happens to ship, and several
    // Wine builds - Proton's among them - provide only `wine` and `wineserver`
    // in their bin directory. Going through `wine` works with every one of
    // them and asks nothing of the packaging.
    wine(prefix, "wine wineboot", &["wineboot", "-u"])?;
    if let Some(version) = version {
        wine(prefix, "wine winecfg", &["winecfg", "/v", version])?;
    }
    // wineserver lingers and holds the prefix open; a later step that moves
    // drive_c out from under it would race with a live server.
    let _ = Command::new("wineserver")
        .arg("-w")
        .env("WINEPREFIX", prefix)
        .status();
    Ok(())
}

/// Runs `wine` with `args` against `prefix`, failing if it does.
fn wine(prefix: &Path, what: &'static str, args: &[&str]) -> Result<(), Error> {
    let status = Command::new("wine")
        .args(args)
        .env("WINEPREFIX", prefix)
        .env("WINEDLLOVERRIDES", "mscoree,mshtml=")
        .env("WINEDEBUG", "-all")
        .status()
        .map_err(|e| Error::Tool(what, e))?;
    if !status.success() {
        return Err(Error::ToolFailed(
            what,
            crate::i18n::tf("tool_exited", &[("status", &status)]),
        ));
    }
    Ok(())
}

/// Which of Wine's Windows versions an environment on a base of `build`
/// reports, when it should not be Wine's default.
///
/// The projection keeps the base's own version key out, so what a program is
/// told is Wine's account: one of the versions `winecfg` offers, each with the
/// build number Wine stands behind. Left alone that is Windows 10, build
/// 19045, whatever the base is. A Windows 11 base gets Wine's Windows 11,
/// build 22000, instead - not the base's own build, because the calls that
/// answer a program are still Wine's.
///
/// Telling a Windows 11 base's programs they run on Windows 10 is not merely
/// untidy. FiveM patches a flag in ntdll on Windows 10 builds 18362 to 19627,
/// assuming Microsoft's ntdll, and under Wine's 19045 that write lands in
/// read-only code and crashes it.
pub fn wine_version_for(build: u32) -> Option<&'static str> {
    // 22000 is the first Windows 11 build, and the build Wine's own "win11"
    // reports.
    (build >= 22000).then_some("win11")
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

/// Whether Wine can decode the media formats games actually ship.
///
/// Wine plays video and audio through GStreamer, and a distribution that
/// installs GStreamer's *libraries* without its *plugins* leaves that silently
/// broken: the game runs, the cutscene is black and the music is absent, and
/// the only clue is a `Missing decoder` line on a stderr nobody is reading.
/// Proton avoids the whole question by bundling its own GStreamer; Raven uses
/// the system's, so the least it can do is say when the system has none.
///
/// MP3 is the probe because it is the format most likely to be present in an
/// older game and the one whose decoder lives in an optional package.
pub fn media_decoders() -> Option<Vec<&'static str>> {
    let dirs = [
        "/usr/lib/gstreamer-1.0",
        "/usr/lib64/gstreamer-1.0",
        "/usr/lib/x86_64-linux-gnu/gstreamer-1.0",
    ];
    let has = |plugin: &str| {
        dirs.iter()
            .any(|d| std::path::Path::new(d).join(plugin).exists())
    };
    let mut missing = Vec::new();
    // Either one decodes MP3: libav through ffmpeg, or good through mpg123.
    if !has("libgstlibav.so") && !has("libgstgood.so") {
        missing.push("MP3 and most video (install gst-libav)");
    }
    if !has("libgstplayback.so") {
        missing.push("the playback pipeline itself (install gst-plugins-base)");
    }
    if missing.is_empty() {
        None
    } else {
        Some(missing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_windows_11_base_reports_wines_windows_11() {
        assert_eq!(wine_version_for(26200), Some("win11"));
        assert_eq!(wine_version_for(22000), Some("win11"));
    }

    #[test]
    fn a_windows_10_base_keeps_wines_default() {
        // Wine's default already is Windows 10, and setting it would only
        // pin a prefix to today's idea of it.
        assert_eq!(wine_version_for(21999), None);
        assert_eq!(wine_version_for(19045), None);
    }
}
