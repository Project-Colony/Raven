//! Plain data the screens draw.
//!
//! Kept apart from the drawing so it can be tested: an iced widget tree cannot
//! be meaningfully asserted on, and pretending otherwise produces tests that
//! pass whatever the window looks like.

use std::path::PathBuf;

use raven::d3d::{DXVK, VKD3D};
use raven::env::Environment;

/// One environment, as a card draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvRow {
    pub name: String,
    pub base: String,
    /// The session anchor's pid, when one is live.
    pub session: Option<u32>,
    /// How many of the user's processes hold the mount. The anchor and
    /// Wine's services are the session itself, and with the server
    /// persistent they are always there.
    pub programs: usize,
    /// The installed build, as the release named itself.
    pub dxvk: Option<String>,
    pub vkd3d: Option<String>,
    /// Drive letter and the device behind it.
    pub attachments: Vec<(char, PathBuf)>,
}

impl EnvRow {
    pub fn is_running(&self) -> bool {
        self.session.is_some()
    }

    /// The line under the environment's name. Written here rather than in the
    /// view so it can be tested, and because "1 process" is not "1 processes".
    pub fn status_line(&self) -> String {
        if !self.is_running() {
            return crate::i18n::t("card_not_running").into();
        }
        if self.programs == 0 {
            return crate::i18n::t("card_idle").into();
        }
        crate::i18n::tn("card_running", self.programs, &[])
    }
}

/// Reads every environment's state. Touches `/proc` for each process on the
/// machine and stats dozens of files, so this must not run on the interface
/// thread - `load::environments` is the only intended caller.
pub fn env_rows(envs: Vec<Environment>) -> Vec<EnvRow> {
    envs.into_iter()
        .map(|e| EnvRow {
            name: e.name.clone(),
            base: e.manifest.base.clone(),
            session: e.session(),
            programs: e.holders().iter().filter(|h| !h.is_session()).count(),
            dxvk: e.d3d_build(&DXVK),
            vkd3d: e.d3d_build(&VKD3D),
            attachments: e
                .attachments()
                .into_iter()
                .map(|a| (a.letter, a.device))
                .collect(),
        })
        .collect()
}

/// One deployed Windows, as the bases screen draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseRow {
    pub id: String,
    /// How many environments run against it, so destroying one is informed.
    pub environments: usize,
}

/// `env_bases` is one base id per environment, straight from the manifests.
/// A count needs nothing more, and reading an environment's state to get it
/// would walk `/proc` for every process on the machine, twice over.
pub fn base_rows(bases: Vec<raven::base::Base>, env_bases: &[String]) -> Vec<BaseRow> {
    bases
        .into_iter()
        .map(|b| BaseRow {
            environments: env_bases.iter().filter(|base| **base == b.id).count(),
            id: b.id,
        })
        .collect()
}

/// One line of the diagnostics screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub label: String,
    pub ok: bool,
    /// What it means, not just whether it passed. `raven doctor` reports
    /// "absent - Wine falls back to wineserver for NT synchronization" rather
    /// than "no", and the window keeps that.
    pub detail: String,
}

/// The same judgements `raven doctor` prints, from the same functions and in
/// the same words - `raven::i18n`'s - so a difference between the two would
/// be a bug here, not a second opinion.
pub fn checks() -> Vec<Check> {
    use raven::i18n::{t, tf};
    use raven::mount::MountBackend as _;
    let userns = raven::mount::UserNsOverlay::is_available();
    let wine = raven::prefix::wine_available();
    let ntsync = std::path::Path::new("/dev/ntsync").exists();
    let media = raven::prefix::media_decoders();

    vec![
        exe_handler(),
        Check {
            label: t("check_userns").into(),
            ok: userns,
            detail: t(if userns {
                "check_userns_ok"
            } else {
                "check_userns_restricted"
            })
            .into(),
        },
        Check {
            label: t("check_wine").into(),
            ok: wine,
            detail: t(if wine { "found" } else { "check_wine_missing" }).into(),
        },
        Check {
            label: t("ntsync").into(),
            ok: ntsync,
            detail: t(if ntsync {
                "ntsync_present"
            } else {
                "ntsync_absent"
            })
            .into(),
        },
        Check {
            label: t("check_media").into(),
            ok: media.is_none(),
            detail: match media {
                None => t("media_present").into(),
                Some(missing) => tf(
                    "check_media_incomplete",
                    &[("missing", &missing.join("; "))],
                ),
            },
        },
    ]
}

/// Who the kernel hands a double-clicked `.exe` to.
///
/// The check `raven doctor` closes with, and the one the window most needs:
/// Wine registers a handler for the same `MZ` magic, the kernel silently
/// picks the most recently registered, and losing that race looks exactly
/// like Raven losing its prefix. Someone using the window rather than a
/// terminal is precisely who would never find that out.
fn exe_handler() -> Check {
    use raven::i18n::{t, tf};
    let label = t("check_exe").to_string();
    if !std::path::Path::new("/proc/sys/fs/binfmt_misc").exists() {
        return Check {
            label,
            ok: false,
            detail: t("exe_no_binfmt").into(),
        };
    }
    let handlers = raven::launch::exe_handlers();
    match handlers.iter().find(|h| h.enabled) {
        None if handlers.is_empty() => Check {
            label,
            ok: false,
            detail: t("check_exe_none").into(),
        },
        None => Check {
            label,
            ok: false,
            detail: t("check_exe_all_disabled").into(),
        },
        Some(w) if w.name == raven::launch::BINFMT_NAME => Check {
            label,
            ok: true,
            detail: tf(
                "check_exe_raven",
                &[("name", &w.name), ("interpreter", &w.interpreter.display())],
            ),
        },
        Some(w) => Check {
            label,
            ok: false,
            detail: tf(
                "check_exe_other",
                &[("name", &w.name), ("interpreter", &w.interpreter.display())],
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_environment_with_a_session_is_marked_running() {
        let row = EnvRow {
            name: "games".into(),
            base: "win11-26200-pro".into(),
            session: Some(4242),
            programs: 2,
            dxvk: Some("dxvk-3.1".into()),
            vkd3d: None,
            attachments: vec![],
        };
        assert!(row.is_running());
        assert_eq!(row.status_line(), "running - 2 processes");
    }

    #[test]
    fn an_environment_without_one_says_so_in_the_singular_when_it_should() {
        let mut row = EnvRow {
            name: "games".into(),
            base: "b".into(),
            session: None,
            programs: 0,
            dxvk: None,
            vkd3d: None,
            attachments: vec![],
        };
        assert!(!row.is_running());
        assert_eq!(row.status_line(), "not running");
        row.session = Some(1);
        assert_eq!(row.status_line(), "running - idle", "only the session");
        row.programs = 1;
        assert_eq!(row.status_line(), "running - 1 process");
    }

    #[test]
    fn a_base_counts_the_environments_built_on_it_from_their_ids_alone() {
        // Counting must not cost a read of each environment's state: that
        // walks /proc for every process on the machine, and the id is in the
        // manifest already.
        let bases = vec![
            raven::base::Base {
                id: "win11".into(),
                path: PathBuf::new(),
            },
            raven::base::Base {
                id: "win10".into(),
                path: PathBuf::new(),
            },
        ];
        let rows = base_rows(bases, &["win11".into(), "win11".into()]);
        assert_eq!(rows[0].environments, 2);
        assert_eq!(rows[1].environments, 0);
    }

    #[test]
    fn every_check_carries_its_consequence_and_not_just_a_no() {
        // `checks()` reads the machine, so which of them pass differs between
        // machines and cannot be asserted on. What holds everywhere is the
        // shape: four judgements, each one saying what it costs. Building a
        // `Check` here and asserting on the string just typed would run none
        // of that.
        let checks = checks();
        assert!(
            checks.len() >= 5,
            "the window shows what raven doctor judges, the .exe handler included"
        );
        assert!(
            checks.iter().any(|c| c.label.contains(".exe")),
            "the handler race is the failure that looks like Raven losing its prefix"
        );
        for c in &checks {
            assert!(!c.label.is_empty());
            // A bare verdict is what `raven doctor` refuses to print, and the
            // window shows whatever this returns, so it must refuse it too.
            assert!(
                !matches!(c.detail.as_str(), "" | "yes" | "no" | "ok"),
                "{} said only {:?}, which is the green tick this screen exists to avoid",
                c.label,
                c.detail
            );
        }
    }
}
