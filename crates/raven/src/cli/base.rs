//! `raven base`: the deployed Windows installations.

use anyhow::Result;
use raven::base;

use super::BaseCmd;

pub fn base_cmd(cmd: BaseCmd) -> Result<()> {
    match cmd {
        BaseCmd::List => {
            let bases = base::Base::list()?;
            if bases.is_empty() {
                out!("No bases. Deploy one with `raven base deploy`.");
            }
            for b in bases {
                let ok = if b.looks_like_windows() {
                    ""
                } else {
                    "  (does not look like Windows)"
                };
                out!("{}{ok}", b.id);
            }
            Ok(())
        }
        BaseCmd::Editions(a) => {
            for e in base::editions(&a.image)? {
                let build = e.build.as_deref().unwrap_or("-");
                out!("{:>3}  {}  (build {build})", e.index, e.name);
            }
            Ok(())
        }
        BaseCmd::Deploy {
            image,
            edition,
            name,
        } => {
            out!("Applying edition {edition}. This writes tens of gigabytes and takes a while.");
            let b = base::deploy(&image.image, edition, &name)?;
            out!("Deployed {} to {}", b.id, b.path.display());
            Ok(())
        }
    }
}
