//! `raven doctor` and `raven binfmt`: what this system can do, and who answers
//! a double-clicked `.exe`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use raven::mount::{MountBackend, UserNsOverlay};
use raven::{base, env, launch};

pub fn doctor() -> Result<()> {
    let userns = UserNsOverlay::is_available();
    out!(
        "unprivileged user namespaces : {}",
        if userns { "yes" } else { "no" }
    );
    if !userns {
        out!(
            "  This kernel restricts them, and Raven's only implemented mount\n  \
             backend needs them. linux-hardened and Ubuntu's AppArmor policy\n  \
             both do this."
        );
    }
    out!(
        "wine                         : {}",
        if raven::prefix::wine_available() {
            "found"
        } else {
            "MISSING - Raven cannot run anything without it"
        }
    );
    out!(
        "ntsync                       : {}",
        if PathBuf::from("/dev/ntsync").exists() {
            "present"
        } else {
            "absent - Wine falls back to wineserver for NT synchronization"
        }
    );
    match raven::prefix::media_decoders() {
        None => out!("media playback               : GStreamer decoders present"),
        Some(missing) => {
            out!("media playback               : INCOMPLETE");
            for m in missing {
                out!("  missing: {m}");
            }
            out!(
                "  Games will run, but cutscenes and music will be silently absent.\n  \
                 Wine decodes media through GStreamer; Proton sidesteps this by\n  \
                 bundling its own, and Raven uses the system's."
            );
        }
    }
    out!(
        "bases                        : {}",
        base::Base::list()?.len()
    );
    // A deploy that was interrupted leaves several gigabytes under a hidden
    // name that `base list` passes over on purpose. Nothing else would ever
    // mention it, so the diagnostics do.
    for (id, path) in base::partials()? {
        out!(
            "  interrupted deploy of {id:?}, still on disk at {}",
            path.display()
        );
        out!("  deploy {id:?} again to finish it, or remove that directory");
    }
    out!(
        "environments                 : {}",
        env::Environment::list()?.len()
    );
    report_exe_handlers();
    Ok(())
}

/// Who actually gets a double-clicked `.exe`.
///
/// Wine registers a handler for the same `MZ` magic, the kernel picks the most
/// recently registered one silently, and losing that race looks like Raven
/// losing its prefix. It cost an hour once; this is the hour, written down.
fn report_exe_handlers() {
    const LABEL: &str = ".exe handler                 ";
    if !Path::new("/proc/sys/fs/binfmt_misc").exists() {
        out!("{LABEL}: binfmt_misc is not mounted - a double-clicked .exe cannot run");
        return;
    }
    let handlers = launch::exe_handlers();
    let Some(winner) = handlers.iter().find(|h| h.enabled) else {
        if handlers.is_empty() {
            out!("{LABEL}: none - a double-clicked .exe will not run; see `raven binfmt`");
        } else {
            // Disabled is not absent: the registration exists, someone turned
            // it off, and re-registering as root is the wrong fix.
            out!("{LABEL}: all disabled - a double-clicked .exe will not run");
            for h in &handlers {
                out!("    {:10} disabled  {}", h.name, h.interpreter.display());
            }
            out!("  Re-enable one: echo 1 | sudo tee /proc/sys/fs/binfmt_misc/<name>");
        }
        return;
    };

    let raven_wins = winner.name == launch::BINFMT_NAME;
    out!(
        "{LABEL}: {} -> {}{}",
        winner.name,
        winner.interpreter.display(),
        if raven_wins { "" } else { "  (NOT Raven)" }
    );

    // Every claimant, when there is more than one: the kernel's choice is
    // silent, so the losers must not be.
    if handlers.len() > 1 {
        for h in &handlers {
            out!(
                "    {:10} {:8} {}{}",
                h.name,
                if h.enabled { "enabled" } else { "disabled" },
                h.interpreter.display(),
                if std::ptr::eq(h, winner) {
                    "  <- wins"
                } else {
                    ""
                }
            );
        }
    }

    if !raven_wins {
        out!(
            "  Every double-clicked .exe runs through {}, and the failure looks\n  \
             like Raven losing its prefix. If that is Wine's registration, mask it:\n    \
             echo -n | sudo tee /etc/binfmt.d/wine.conf\n    \
             sudo systemctl restart systemd-binfmt",
            winner.name
        );
    } else if handlers.iter().any(|h| h.enabled && h.name != winner.name) {
        out!(
            "  Another handler matches, and whichever registers LAST wins - an\n  \
             update to its package can silently take every .exe back. Mask it\n  \
             (for Wine: an empty /etc/binfmt.d/wine.conf, then restart\n  \
             systemd-binfmt)."
        );
    }

    if !winner.interpreter.exists() {
        if winner.held_open {
            out!(
                "  Its interpreter no longer exists. The registration survives on the\n  \
                 kernel's open handle until reboot - then every .exe stops running.\n  \
                 Re-register: see `raven binfmt`."
            );
        } else {
            out!(
                "  Its interpreter no longer exists, so every double-clicked .exe\n  \
                 fails right now. Re-register: see `raven binfmt`."
            );
        }
    }
}

pub fn binfmt() -> Result<()> {
    let me = std::env::current_exe().context("could not find this binary")?;
    out!(
        "registration : {}",
        if launch::registered() {
            "active"
        } else {
            "not registered - .exe files will not run on their own"
        }
    );
    out!(
        "mounted      : {}",
        Path::new("/proc/sys/fs/binfmt_misc").exists()
    );
    report_exe_handlers();
    out!();
    out!("Registering needs root, once. It belongs to the package rather than");
    out!("to Raven, so that nothing has to hold privilege while Raven runs:");
    out!();
    out!(
        "  echo '{}' | sudo tee {}",
        launch::binfmt_line(&me),
        launch::conf_path().display()
    );
    out!("  sudo systemctl restart systemd-binfmt");
    out!();
    out!("The interpreter path above is this binary. A package would name its");
    out!("installed location instead.");
    Ok(())
}
