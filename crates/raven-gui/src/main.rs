//! Raven's administration window.
//!
//! A second caller of the same library the command line uses - see
//! docs/superpowers/specs/2026-09-01-raven-gui-design.md. Nothing here decides
//! anything about environments; it draws what the library reports and asks the
//! library to act.

mod errors;
mod load;
mod model;
mod theme;
mod view;

use iced::{Element, Task};

use model::EnvRow;

/// Which screen is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    Environments,
    Bases,
    Doctor,
}

#[derive(Debug, Clone)]
pub enum Message {
    Go(Screen),
    Environments(load::EnvRows),
    Refresh,
    Start(String),
    Stop(String),
    // `raven::Error` holds `std::io::Error` in some variants, so it cannot be
    // `Clone` - and iced's widgets (`Button::on_press` among them) require
    // `Message: Clone`. `Arc` is `Clone` regardless of what it wraps, so it
    // carries the error across that boundary without the library changing.
    Acted(Result<(), std::sync::Arc<raven::Error>>),
}

#[derive(Default)]
pub struct App {
    pub(crate) screen: Screen,
    pub(crate) envs: Vec<EnvRow>,
    pub(crate) offer: Option<errors::Offer>,
}

impl App {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Go(screen) => {
                self.screen = screen;
                Task::none()
            }
            Message::Environments(Ok(rows)) => {
                self.envs = rows;
                self.offer = None;
                Task::none()
            }
            Message::Environments(Err(e)) => {
                self.offer = Some(errors::Offer {
                    message: e,
                    action: None,
                });
                Task::none()
            }
            Message::Refresh => load::environments(),
            Message::Start(name) => Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        let e = raven::env::Environment::open(&name)?;
                        e.ensure_session().map(|_| ())
                    })
                    .await
                    .expect("the blocking task panicked")
                    .map_err(std::sync::Arc::new)
                },
                Message::Acted,
            ),
            Message::Stop(name) => Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        let e = raven::env::Environment::open(&name)?;
                        e.stop().map(|_| ())
                    })
                    .await
                    .expect("the blocking task panicked")
                    .map_err(std::sync::Arc::new)
                },
                Message::Acted,
            ),
            Message::Acted(Ok(())) => load::environments(),
            Message::Acted(Err(e)) => {
                self.offer = Some(errors::explain(&e));
                load::environments()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        view::shell(self)
    }
}

fn main() -> iced::Result {
    iced::application(
        || (App::default(), load::environments()),
        App::update,
        App::view,
    )
    .title("Raven")
    .default_font(theme::APP_FONT)
    .run()
}
