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
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    Environments,
    Detail(String),
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
    Open(String),
    InstallD3d { env: String, vkd3d: bool },
    RemoveD3d { env: String, vkd3d: bool },
    Detach { env: String, letter: char },
    Reproject(String),
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
            Message::Open(name) => {
                self.screen = Screen::Detail(name);
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
            Message::InstallD3d { env, vkd3d } => {
                let which = if vkd3d { "vkd3d" } else { "dxvk" };
                self.offer = Some(errors::Offer {
                    message: format!(
                        "Install it from a build you already have:  raven env {which} {env} --from <path>"
                    ),
                    action: None,
                });
                Task::none()
            }
            Message::RemoveD3d { env, vkd3d } => Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        let rt = if vkd3d {
                            &raven::d3d::VKD3D
                        } else {
                            &raven::d3d::DXVK
                        };
                        let e = raven::env::Environment::open(&env)?;
                        e.remove_d3d(rt).map(|_| ())
                    })
                    .await
                    .expect("the blocking task panicked")
                    .map_err(std::sync::Arc::new)
                },
                Message::Acted,
            ),
            Message::Detach { env, letter } => Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        raven::env::Environment::open(&env)?.detach(letter)
                    })
                    .await
                    .expect("the blocking task panicked")
                    .map_err(std::sync::Arc::new)
                },
                Message::Acted,
            ),
            Message::Reproject(env) => Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        raven::env::Environment::open(&env)?
                            .project_registry()
                            .map(|_| ())
                    })
                    .await
                    .expect("the blocking task panicked")
                    .map_err(std::sync::Arc::new)
                },
                Message::Acted,
            ),
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
