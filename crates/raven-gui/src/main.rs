//! Raven's administration window.
//!
//! A second caller of the same library the command line uses - see
//! docs/superpowers/specs/2026-09-01-raven-gui-design.md. Nothing here decides
//! anything about environments; it draws what the library reports and asks the
//! library to act.

mod deploy;
mod errors;
mod load;
mod model;
mod theme;
mod view;

use std::path::PathBuf;

use iced::{Element, Subscription, Task};

use model::{BaseRow, Check, EnvRow};

/// Which screen is showing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    Environments,
    Detail(String),
    Bases,
    Doctor,
}

/// The bases screen's three text fields, since this batch has no file dialog:
/// an image path typed or pasted, an edition index, and a name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DeployForm {
    pub image: String,
    pub edition: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    Go(Screen),
    Environments(load::EnvRows),
    Bases(load::BaseRows),
    Doctor(load::Checks),
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
    DeployImageChanged(String),
    DeployEditionChanged(String),
    DeployNameChanged(String),
    DeployStart,
    DeployProgress(deploy::Progress),
    DeployDone(bool),
}

#[derive(Default)]
pub struct App {
    pub(crate) screen: Screen,
    pub(crate) envs: Vec<EnvRow>,
    pub(crate) bases: Vec<BaseRow>,
    pub(crate) checks: Vec<Check>,
    pub(crate) offer: Option<errors::Offer>,
    pub(crate) deploy_form: DeployForm,
    /// The bar the bases screen draws, when a deployment is running.
    pub(crate) deploying: Option<deploy::Progress>,
    /// What the running deployment was started with - kept so `subscription`
    /// can keep pointing `Subscription::run_with` at the same job, and `None`
    /// once it's over. Distinct from `deploying`: that one is the drawn
    /// state, cleared to show the bar at all; this one is the stream's
    /// identity, and would restart the job if it changed mid-flight.
    deploy_job: Option<deploy::Args>,
}

impl App {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Go(screen) => {
                let task = match screen {
                    Screen::Bases => load::bases(),
                    Screen::Doctor => load::doctor(),
                    _ => Task::none(),
                };
                self.screen = screen;
                task
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
            Message::Bases(Ok(rows)) => {
                self.bases = rows;
                self.offer = None;
                Task::none()
            }
            Message::Bases(Err(e)) => {
                self.offer = Some(errors::Offer {
                    message: e,
                    action: None,
                });
                Task::none()
            }
            Message::Doctor(rows) => {
                self.checks = rows;
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
            Message::DeployImageChanged(image) => {
                self.deploy_form.image = image;
                Task::none()
            }
            Message::DeployEditionChanged(edition) => {
                self.deploy_form.edition = edition;
                Task::none()
            }
            Message::DeployNameChanged(name) => {
                self.deploy_form.name = name;
                Task::none()
            }
            Message::DeployStart => {
                let form = &self.deploy_form;
                match form.edition.trim().parse::<u32>() {
                    Ok(edition)
                        if !form.image.trim().is_empty() && !form.name.trim().is_empty() =>
                    {
                        self.deploy_job = Some(deploy::Args {
                            image: PathBuf::from(form.image.trim()),
                            edition,
                            name: form.name.trim().to_owned(),
                        });
                        self.deploying = Some(deploy::Progress {
                            percent: 0,
                            what: "Starting".into(),
                        });
                        self.offer = None;
                    }
                    _ => {
                        self.offer = Some(errors::Offer {
                            message: "Fill in the image path, a numeric edition, and a name."
                                .into(),
                            action: None,
                        });
                    }
                }
                Task::none()
            }
            Message::DeployProgress(progress) => {
                self.deploying = Some(progress);
                Task::none()
            }
            Message::DeployDone(success) => {
                self.deploy_job = None;
                self.deploying = None;
                if success {
                    self.deploy_form = DeployForm::default();
                    load::bases()
                } else {
                    self.offer = Some(errors::Offer {
                        message: "Deployment failed. Run the same `raven base deploy` from a terminal to see why."
                            .into(),
                        action: None,
                    });
                    Task::none()
                }
            }
        }
    }

    /// While a deployment is running, keeps its stream alive; otherwise
    /// subscribes to nothing. `deploy_job` - not `deploying` - is the
    /// identity `Subscription::run_with` hashes on, so editing the progress
    /// text alone can never be mistaken for a new job.
    fn subscription(&self) -> Subscription<Message> {
        match &self.deploy_job {
            Some(args) => Subscription::run_with(args.clone(), deploy::run),
            None => Subscription::none(),
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
    .subscription(App::subscription)
    .title("Raven")
    .default_font(theme::APP_FONT)
    .run()
}
