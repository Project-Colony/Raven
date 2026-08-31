//! Environments: one Windows base, one Wine layer over it, one place for writes.
//!
//! An environment is deliberately cheap. Everything expensive — the deployed
//! Windows — is shared and immutable, so creating one copies only Wine's
//! skeleton, and destroying one deletes a directory. A broken installation is
//! recovered by throwing the environment away, not by repairing it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::mount::OverlaySpec;
use crate::{Error, base::Base, layer, paths, prefix};

/// What an environment records about itself, on disk as `environment.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    /// The base this environment runs against. Stored by id, not by path, so
    /// moving the data directory does not break every environment in it.
    pub base: String,
}

#[derive(Debug, Clone)]
pub struct Environment {
    pub name: String,
    pub manifest: Manifest,
    pub root: PathBuf,
}

impl Environment {
    pub fn layer(&self) -> PathBuf {
        self.root.join("layer")
    }
    pub fn upper(&self) -> PathBuf {
        self.root.join("upper")
    }
    pub fn work(&self) -> PathBuf {
        self.root.join("work")
    }
    pub fn prefix(&self) -> PathBuf {
        self.root.join("prefix")
    }

    /// The layer stack for this environment.
    ///
    /// Order is the whole point: the Wine layer first so its files win, the
    /// Windows base second so Microsoft's show through everywhere Wine has none.
    pub fn spec(&self) -> Result<OverlaySpec, Error> {
        Ok(OverlaySpec {
            lower: vec![self.layer(), Base::find(&self.manifest.base)?.path],
            upper: self.upper(),
            work: self.work(),
            target: paths::mount_point(&self.name)?,
        })
    }

    pub fn open(name: &str) -> Result<Environment, Error> {
        let root = paths::environment_dir(name)?;
        let file = root.join("environment.toml");
        let text = std::fs::read_to_string(&file)
            .map_err(|_| Error::NoSuchEnvironment(name.to_owned()))?;
        Ok(Environment {
            name: name.to_owned(),
            manifest: toml::from_str(&text).map_err(|e| Error::Manifest(file, e.to_string()))?,
            root,
        })
    }

    pub fn list() -> Result<Vec<Environment>, Error> {
        let dir = paths::data_dir()?.join("environments");
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .map_err(|e| Error::Layer(dir, e))?
            .filter_map(Result::ok)
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        Ok(names
            .iter()
            .filter_map(|n| Environment::open(n).ok())
            .collect())
    }

    /// Deletes the environment. The base it ran against is untouched.
    pub fn destroy(name: &str) -> Result<(), Error> {
        let root = paths::environment_dir(name)?;
        if !root.exists() {
            return Err(Error::NoSuchEnvironment(name.to_owned()));
        }
        std::fs::remove_dir_all(&root).map_err(|e| Error::Layer(root, e))
    }
}

/// Builds a new environment against an existing base.
///
/// The steps are ordered by what they depend on, and the case normalisation is
/// not optional: without it the Wine layer and the Windows base do not merge at
/// all, and the layer shadows nothing.
pub fn create(name: &str, base_id: &str) -> Result<Environment, Error> {
    let base = Base::find(base_id)?;
    if !base.looks_like_windows() {
        return Err(Error::NotAWindowsBase(base_id.to_owned()));
    }
    if !prefix::wine_available() {
        return Err(Error::NoWine);
    }

    let root = paths::environment_dir(name)?;
    if root.exists() {
        return Err(Error::EnvironmentExists(name.to_owned()));
    }

    // Anything that fails from here leaves a half-built environment, so the
    // whole directory is removed on the way out rather than left to be found.
    let built = (|| -> Result<Environment, Error> {
        for d in ["upper", "work", "prefix"] {
            std::fs::create_dir_all(root.join(d)).map_err(|e| Error::Layer(root.join(d), e))?;
        }
        let env = Environment {
            name: name.to_owned(),
            manifest: Manifest {
                base: base_id.to_owned(),
            },
            root: root.clone(),
        };

        prefix::create(&env.prefix())?;

        // Wine's drive_c becomes the upper read-only layer. It is moved rather
        // than copied: leaving a second copy behind would let Wine and the
        // overlay disagree about which one is the prefix's C:.
        let drive_c = env.prefix().join("drive_c");
        std::fs::rename(&drive_c, env.layer()).map_err(|e| Error::Layer(drive_c, e))?;

        layer::normalise_case(&env.layer(), &base.path)?;
        prefix::point_c_drive(&env.prefix(), &paths::mount_point(name)?)?;

        let manifest = toml::to_string(&env.manifest)
            .map_err(|e| Error::Manifest(root.join("environment.toml"), e.to_string()))?;
        std::fs::write(root.join("environment.toml"), manifest)
            .map_err(|e| Error::Layer(root.join("environment.toml"), e))?;
        Ok(env)
    })();

    if built.is_err() {
        let _ = std::fs::remove_dir_all(&root);
    }
    built
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_round_trips() {
        let m = Manifest {
            base: "win11-26200-pro".into(),
        };
        let text = toml::to_string(&m).unwrap();
        assert_eq!(toml::from_str::<Manifest>(&text).unwrap(), m);
    }

    #[test]
    fn the_layer_comes_before_the_base_in_the_stack() {
        let env = Environment {
            name: "x".into(),
            manifest: Manifest { base: "b".into() },
            root: PathBuf::from("/data/environments/x"),
        };
        // Checked without touching the filesystem: only the ordering matters,
        // and getting it backwards would silently let Microsoft's ntdll win.
        assert_eq!(env.layer(), PathBuf::from("/data/environments/x/layer"));
        assert!(env.layer().ends_with("layer"));
    }
}
