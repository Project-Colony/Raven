//! `raven doctor` and `raven binfmt`: what this system can do, and who answers
//! a double-clicked `.exe`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use raven::i18n::{t, tf};
use raven::mount::{MountBackend, UserNsOverlay};
use raven::{base, env, launch};

/// Every label the two reports print, so their values share one column in
/// whichever language is speaking.
const LABELS: [&str; 9] = [
    "doctor_userns",
    "doctor_wine",
    "ntsync",
    "doctor_media",
    "doctor_bases",
    "doctor_environments",
    "doctor_exe",
    "binfmt_registration",
    "binfmt_mounted",
];

/// `label :`, padded to the widest label.
fn field(key: &'static str) -> String {
    let width = LABELS
        .iter()
        .map(|k| t(k).chars().count())
        .max()
        .unwrap_or(0);
    format!("{:<width$} :", t(key))
}

fn yes_no(b: bool) -> &'static str {
    t(if b { "yes" } else { "no" })
}

pub fn doctor() -> Result<()> {
    let userns = UserNsOverlay::is_available();
    out!("{} {}", field("doctor_userns"), yes_no(userns));
    if !userns {
        out!("{}", t("doctor_userns_restricted"));
    }
    out!(
        "{} {}",
        field("doctor_wine"),
        t(if raven::prefix::wine_available() {
            "found"
        } else {
            "doctor_wine_missing"
        })
    );
    out!(
        "{} {}",
        field("ntsync"),
        t(if PathBuf::from("/dev/ntsync").exists() {
            "ntsync_present"
        } else {
            "ntsync_absent"
        })
    );
    match raven::prefix::media_decoders() {
        None => out!("{} {}", field("doctor_media"), t("media_present")),
        Some(missing) => {
            out!("{} {}", field("doctor_media"), t("doctor_media_incomplete"));
            for m in missing {
                out!("{}", tf("doctor_media_missing", &[("what", &m)]));
            }
            out!("{}", t("doctor_media_consequence"));
        }
    }
    out!("{} {}", field("doctor_bases"), base::Base::list()?.len());
    // A deploy that was interrupted leaves several gigabytes under a hidden
    // name that `base list` passes over on purpose. Nothing else would ever
    // mention it, so the diagnostics do.
    for (id, path) in base::partials()? {
        let id = format!("{id:?}");
        out!(
            "{}",
            tf("doctor_partial", &[("id", &id), ("path", &path.display())])
        );
        out!("{}", tf("doctor_partial_hint", &[("id", &id)]));
    }
    out!(
        "{} {}",
        field("doctor_environments"),
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
    let label = field("doctor_exe");
    if !Path::new("/proc/sys/fs/binfmt_misc").exists() {
        out!("{label} {}", t("exe_no_binfmt"));
        return;
    }
    let handlers = launch::exe_handlers();
    let Some(winner) = handlers.iter().find(|h| h.enabled) else {
        if handlers.is_empty() {
            out!("{label} {}", t("doctor_exe_none"));
        } else {
            // Disabled is not absent: the registration exists, someone turned
            // it off, and re-registering as root is the wrong fix.
            out!("{label} {}", t("doctor_exe_all_disabled"));
            for h in &handlers {
                out!(
                    "    {:10} {}  {}",
                    h.name,
                    t("disabled"),
                    h.interpreter.display()
                );
            }
            out!("{}", t("doctor_exe_reenable"));
        }
        return;
    };

    let raven_wins = winner.name == launch::BINFMT_NAME;
    out!(
        "{label} {} -> {}{}",
        winner.name,
        winner.interpreter.display(),
        if raven_wins {
            ""
        } else {
            t("doctor_exe_not_raven")
        }
    );

    // Every claimant, when there is more than one: the kernel's choice is
    // silent, so the losers must not be.
    if handlers.len() > 1 {
        let state = |on: bool| t(if on { "enabled" } else { "disabled" });
        let width = state(true)
            .chars()
            .count()
            .max(state(false).chars().count());
        for h in &handlers {
            out!(
                "    {:10} {:width$} {}{}",
                h.name,
                state(h.enabled),
                h.interpreter.display(),
                if std::ptr::eq(h, winner) {
                    t("doctor_exe_wins")
                } else {
                    ""
                }
            );
        }
    }

    if !raven_wins {
        out!("{}", tf("doctor_exe_other_wins", &[("name", &winner.name)]));
        out!("    echo -n | sudo tee /etc/binfmt.d/wine.conf");
        out!("    sudo systemctl restart systemd-binfmt");
    } else if handlers.iter().any(|h| h.enabled && h.name != winner.name) {
        out!("{}", t("doctor_exe_rival"));
    }

    if !winner.interpreter.exists() {
        out!(
            "{}",
            t(if winner.held_open {
                "doctor_exe_gone_held"
            } else {
                "doctor_exe_gone"
            })
        );
    }
}

pub fn binfmt() -> Result<()> {
    let me = std::env::current_exe().context(t("binfmt_no_binary"))?;
    out!(
        "{} {}",
        field("binfmt_registration"),
        t(if launch::registered() {
            "binfmt_active"
        } else {
            "binfmt_not_registered"
        })
    );
    out!(
        "{} {}",
        field("binfmt_mounted"),
        yes_no(Path::new("/proc/sys/fs/binfmt_misc").exists())
    );
    report_exe_handlers();
    out!();
    out!("{}", t("binfmt_how"));
    out!();
    out!(
        "  echo '{}' | sudo tee {}",
        launch::binfmt_line(&me),
        launch::conf_path().display()
    );
    out!("  sudo systemctl restart systemd-binfmt");
    out!();
    out!("{}", t("binfmt_interpreter"));
    Ok(())
}
