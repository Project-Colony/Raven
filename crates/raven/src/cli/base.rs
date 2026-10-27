//! `raven base`: the deployed Windows installations.

use anyhow::Result;
use raven::base;
use raven::i18n::{t, tf, tn};

use super::BaseCmd;

pub fn base_cmd(cmd: BaseCmd) -> Result<()> {
    match cmd {
        BaseCmd::List => {
            let bases = base::Base::list()?;
            if bases.is_empty() {
                out!("{}", t("base_none"));
            }
            for b in bases {
                let ok = if b.looks_like_windows() {
                    ""
                } else {
                    t("base_not_windows")
                };
                out!("{}{ok}", b.id);
            }
            Ok(())
        }
        BaseCmd::Editions(a) => {
            for e in base::editions(&a.image)? {
                let build = e.build.as_deref().unwrap_or("-");
                out!(
                    "{}",
                    tf(
                        "base_edition",
                        &[
                            ("index", &format!("{:>3}", e.index)),
                            ("name", &e.name),
                            ("build", &build)
                        ]
                    )
                );
            }
            Ok(())
        }
        BaseCmd::Respell { id } => {
            let done = raven::env::respell_base(&id)?;
            if done.directories == 0 {
                out!("{}", tf("base_respelled_already", &[("id", &id)]));
            } else {
                out!(
                    "{}",
                    tn(
                        "base_respelled",
                        done.environments,
                        &[("id", &id), ("dirs", &done.directories)]
                    )
                );
            }
            Ok(())
        }
        BaseCmd::Deploy {
            image,
            edition,
            name,
        } => {
            out!("{}", tf("base_applying", &[("edition", &edition)]));
            let b = base::deploy(&image.image, edition, &name)?;
            out!(
                "{}",
                tf(
                    "base_deployed",
                    &[("id", &b.id), ("path", &b.path.display())]
                )
            );
            Ok(())
        }
    }
}
