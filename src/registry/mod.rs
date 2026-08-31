//! Projecting a real Windows registry into a Wine prefix.
//!
//! Windows keeps the registry in binary hive files; Wine keeps it as text. The
//! bridge between them is deliberately **selective**: the keys describing
//! installed *software* cross, and the keys describing a *machine* — its
//! drivers, its services, its devices — do not. Importing the latter replaces
//! Wine's true account of the environment it provides with a true account of a
//! different, absent one.
//!
//! The projection is derived, idempotent and driven by a reviewable rules file.
//! Correcting its output by hand would produce something nobody can reproduce;
//! the rules are what you edit.

pub mod emit;
pub mod hive;
pub mod rules;

use std::path::Path;
use std::process::Command;

use crate::Error;
pub use emit::{Data, Key, Value};
pub use rules::Rules;

/// Which hive file supplies which part of the registry.
///
/// A hive does not record where it belongs — that is decided by whoever loads
/// it — so the mapping lives here.
pub const SOURCES: &[(&str, &str)] = &[
    ("Windows/System32/config/SOFTWARE", r"HKLM\Software"),
    ("Users/Default/NTUSER.DAT", r"HKCU"),
];

/// Reads every hive in `base` that Raven knows about and renders what the rules
/// permit as a single `.reg` file.
pub fn project_base(base: &Path, rules: &Rules) -> Result<String, Error> {
    let mut keys = Vec::new();
    for (rel, mount) in SOURCES {
        let path = base.join(rel);
        // A base is allowed not to have every hive. A never-booted Windows has
        // no user profiles, so NTUSER.DAT is routinely absent.
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        keys.extend(hive::project(&bytes, mount, rules)?);
    }
    keys.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(emit::to_reg(&keys))
}

/// Imports a `.reg` file into a prefix, with the environment's C: mounted.
///
/// `regedit` is a Windows program like any other: it needs a working
/// `C:\Windows\System32` to load `kernel32` from. Once an environment's
/// `dosdevices/c:` points at the runtime mount, that path exists only while the
/// overlay is mounted — so the import runs inside a mount rather than beside it.
///
/// The mount can only happen in a process that is about to be replaced by the
/// program, so this spawns `raven exec` rather than mounting here. `WINEPREFIX`
/// is set on that child and inherited by the `wine` it execs.
pub fn import(
    spec: &crate::mount::OverlaySpec,
    prefix: &Path,
    reg_file: &Path,
) -> Result<(), Error> {
    std::fs::create_dir_all(&spec.target).map_err(|e| Error::Layer(spec.target.clone(), e))?;

    let me = std::env::current_exe().map_err(|e| Error::Tool("raven", e))?;
    let mut cmd = Command::new(me);
    cmd.arg("exec");
    for l in &spec.lower {
        cmd.arg("--lower").arg(l);
    }
    let out = cmd
        .arg("--upper")
        .arg(&spec.upper)
        .arg("--work")
        .arg(&spec.work)
        .arg("--target")
        .arg(&spec.target)
        .arg("--")
        // `wineserver -w` waits for the server to exit, and the server writes
        // the registry to disk on its way out. Without it the import lives only
        // in a server that outlives this call: anything reading system.reg
        // sooner sees nothing, and a server killed before it idles out loses
        // the whole projection silently.
        .args([
            "/bin/sh",
            "-c",
            "wine regedit /S \"$1\" && wineserver -w",
            "sh",
        ])
        .arg(reg_file)
        .env("WINEPREFIX", prefix)
        .env("WINEDEBUG", "-all")
        .output()
        .map_err(|e| Error::Tool("wine regedit", e))?;

    if !out.status.success() {
        return Err(Error::ToolFailed(
            "wine regedit",
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .filter(|l| !l.contains("MESA-EGL") && !l.contains("pci id"))
                .collect::<Vec<_>>()
                .join("; "),
        ));
    }
    Ok(())
}
