//! Calling the library without freezing the window.
//!
//! Reading one environment's state opens `/proc` for every process on the
//! machine. Doing that on the interface thread stutters the window, so every
//! library call in the program goes through this module.

use iced::Task;

use crate::Message;
use crate::model::{self, BaseRow, Check, EnvRow};

/// Loads every environment's state on a blocking worker.
pub fn environments() -> Task<Message> {
    Task::perform(
        async {
            tokio::task::spawn_blocking(|| {
                raven::env::Environment::list()
                    .map(model::env_rows)
                    .map_err(|e| e.to_string())
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()))
        },
        Message::Environments,
    )
}

/// Loads every base, and the base id of every environment so each count is
/// right. The ids come from the manifests alone, so unlike `environments`
/// this never touches `/proc`; it stays off the interface thread only
/// because it reads the disk.
pub fn bases() -> Task<Message> {
    Task::perform(
        async {
            tokio::task::spawn_blocking(|| -> Loaded<Vec<BaseRow>> {
                let bases = raven::base::Base::list().map_err(|e| e.to_string())?;
                let env_bases: Vec<String> = raven::env::Environment::list()
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .map(|e| e.manifest.base)
                    .collect();
                Ok(model::base_rows(bases, &env_bases))
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()))
        },
        Message::Bases,
    )
}

/// Loads every diagnostic on a blocking worker. `checks` shells out to `wine
/// --version`, which is enough of a subprocess call that it must not run on
/// the interface thread either. Unlike `environments` and `bases`, `checks`
/// cannot fail - it turns absence into a judgement rather than an error - so
/// there is no `Err` case to carry.
pub fn doctor() -> Task<Message> {
    Task::perform(
        async {
            tokio::task::spawn_blocking(model::checks)
                .await
                .unwrap_or_default()
        },
        Message::Doctor,
    )
}

/// The shape every loader returns: the data, or a message already fit to show.
pub type Loaded<T> = Result<T, String>;

/// Named so the signature above reads: `Task<Message>` carrying `Loaded<Vec<EnvRow>>`.
pub type EnvRows = Loaded<Vec<EnvRow>>;

/// Named so the signature above reads: `Task<Message>` carrying `Loaded<Vec<BaseRow>>`.
pub type BaseRows = Loaded<Vec<BaseRow>>;

/// Named so the signature above reads: `Task<Message>` carrying `Vec<Check>`.
pub type Checks = Vec<Check>;
