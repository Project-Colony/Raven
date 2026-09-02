//! Calling the library without freezing the window.
//!
//! Reading one environment's state opens `/proc` for every process on the
//! machine. Doing that on the interface thread stutters the window, so every
//! library call in the program goes through this module.

use iced::Task;

use crate::Message;
use crate::model::{self, EnvRow};

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

/// The shape every loader returns: the data, or a message already fit to show.
pub type Loaded<T> = Result<T, String>;

/// Named so the signature above reads: `Task<Message>` carrying `Loaded<Vec<EnvRow>>`.
pub type EnvRows = Loaded<Vec<EnvRow>>;
