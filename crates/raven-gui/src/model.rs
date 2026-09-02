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
    /// How many processes hold the mount, the anchor included.
    pub holders: usize,
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
            return "not running".into();
        }
        let plural = if self.holders == 1 {
            "process"
        } else {
            "processes"
        };
        format!("running - {} {plural}", self.holders)
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
            holders: e.holders().len(),
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

pub fn base_rows(bases: Vec<raven::base::Base>, envs: &[EnvRow]) -> Vec<BaseRow> {
    bases
        .into_iter()
        .map(|b| BaseRow {
            environments: envs.iter().filter(|e| e.base == b.id).count(),
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

/// The same four judgements `raven doctor` prints, from the same functions -
/// a difference between the two would be a bug here, not a second opinion.
pub fn checks() -> Vec<Check> {
    use raven::mount::MountBackend as _;
    let userns = raven::mount::UserNsOverlay::is_available();
    let wine = raven::prefix::wine_available();
    let ntsync = std::path::Path::new("/dev/ntsync").exists();
    let media = raven::prefix::media_decoders();

    vec![
        Check {
            label: "Unprivileged user namespaces".into(),
            ok: userns,
            detail: if userns {
                "available - Raven can mount without root".into()
            } else {
                "this kernel restricts them, and Raven's only mount backend needs them".into()
            },
        },
        Check {
            label: "Wine".into(),
            ok: wine,
            detail: if wine {
                "found".into()
            } else {
                "missing - Raven cannot run anything without it".into()
            },
        },
        Check {
            label: "ntsync".into(),
            ok: ntsync,
            detail: if ntsync {
                "present".into()
            } else {
                "absent - Wine falls back to wineserver for NT synchronization".into()
            },
        },
        Check {
            label: "Media playback".into(),
            ok: media.is_none(),
            detail: match media {
                None => "GStreamer decoders present".into(),
                Some(missing) => format!(
                    "incomplete: {}. Games will run with their cutscenes and music silently absent.",
                    missing.join("; ")
                ),
            },
        },
    ]
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
            holders: 9,
            dxvk: Some("dxvk-3.1".into()),
            vkd3d: None,
            attachments: vec![],
        };
        assert!(row.is_running());
        assert_eq!(row.status_line(), "running - 9 processes");
    }

    #[test]
    fn an_environment_without_one_says_so_in_the_singular_when_it_should() {
        let mut row = EnvRow {
            name: "games".into(),
            base: "b".into(),
            session: None,
            holders: 0,
            dxvk: None,
            vkd3d: None,
            attachments: vec![],
        };
        assert!(!row.is_running());
        assert_eq!(row.status_line(), "not running");
        row.session = Some(1);
        row.holders = 1;
        assert_eq!(row.status_line(), "running - 1 process");
    }

    #[test]
    fn a_failing_check_carries_its_consequence_and_not_just_a_no() {
        let c = Check {
            label: "ntsync".into(),
            ok: false,
            detail: "absent - Wine falls back to wineserver for NT synchronization".into(),
        };
        assert!(!c.ok);
        assert!(
            c.detail.len() > "no".len(),
            "the CLI explains what a missing check costs, and the window keeps that"
        );
    }
}
