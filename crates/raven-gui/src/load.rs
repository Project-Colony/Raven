//! Calling the library without freezing the window.
//!
//! Reading one environment's state opens `/proc` for every process on the
//! machine. Doing that on the interface thread stutters the window, so every
//! library call in the program goes through this module.

use iced::Task;

use crate::Message;
use crate::model::{self, BaseRow, EnvRow};

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

/// Loads every base and, so each one's count is right, every environment.
/// Touches the same `/proc` state `environments` does, so this must not run
/// on the interface thread either.
pub fn bases() -> Task<Message> {
    Task::perform(
        async {
            tokio::task::spawn_blocking(|| -> Loaded<Vec<BaseRow>> {
                let bases = raven::base::Base::list().map_err(|e| e.to_string())?;
                let envs = raven::env::Environment::list().map_err(|e| e.to_string())?;
                Ok(model::base_rows(bases, &model::env_rows(envs)))
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()))
        },
        Message::Bases,
    )
}

/// The shape every loader returns: the data, or a message already fit to show.
pub type Loaded<T> = Result<T, String>;

/// Named so the signature above reads: `Task<Message>` carrying `Loaded<Vec<EnvRow>>`.
pub type EnvRows = Loaded<Vec<EnvRow>>;

/// Named so the signature above reads: `Task<Message>` carrying `Loaded<Vec<BaseRow>>`.
pub type BaseRows = Loaded<Vec<BaseRow>>;
