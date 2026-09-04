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
use std::time::Duration;

use iced::{Element, Subscription, Task};

use errors::Offer;
use model::{BaseRow, Check, EnvRow};

/// How often the environments screen asks who holds a session, per the design's
/// Data flow section. The one clock in the program, and it runs only while that
/// screen is showing.
const POLL: Duration = Duration::from_secs(2);

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
    // `Err` carries the last thing the child said before it gave up, so a
    // failure that took minutes does not have to be reproduced in a terminal
    // to be understood.
    DeployDone(Result<(), String>),
}

#[derive(Default)]
pub struct App {
    pub(crate) screen: Screen,
    pub(crate) envs: Vec<EnvRow>,
    pub(crate) bases: Vec<BaseRow>,
    pub(crate) checks: Vec<Check>,
    pub(crate) offer: Option<Offer>,
    /// Whether an environments load is in flight. The poll fires on a clock
    /// that knows nothing about how long `/proc` takes, so without this a slow
    /// scan would have a second one queued behind it every two seconds.
    loading_envs: bool,
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
    /// The window and the first load. `reload_environments` rather than
    /// `load::environments` so `loading_envs` is true from the first frame:
    /// the boot load and the first poll must not both be in flight.
    fn boot() -> (Self, Task<Message>) {
        let mut app = Self::default();
        let task = app.reload_environments();
        (app, task)
    }

    /// Reads every environment's state, and records that it is being read.
    fn reload_environments(&mut self) -> Task<Message> {
        self.loading_envs = true;
        load::environments()
    }

    /// Forgets a banner the user has navigated away from. An error raised on
    /// one screen means nothing above another, and leaving it there makes the
    /// new screen look broken.
    fn dismiss_offer(&mut self) {
        self.offer = None;
    }

    /// Forgets a banner a *load* raised, and only that. A load succeeding says
    /// the read works again; it says nothing about the action that failed, and
    /// taking that message and its button away is what a two-second poll would
    /// otherwise do to every error in the program.
    fn dismiss_stale_load_error(&mut self) {
        if matches!(
            self.offer,
            Some(Offer {
                kind: errors::Kind::LoadError,
                ..
            })
        ) {
            self.offer = None;
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Go(screen) => {
                self.dismiss_offer();
                // The design asks for a reload on arriving at the environments
                // screen: the poll only runs while it is showing, so without
                // this the first two seconds show whatever was true when the
                // user last left it.
                let task = match &screen {
                    Screen::Environments | Screen::Detail(_) => self.reload_environments(),
                    Screen::Bases => load::bases(),
                    Screen::Doctor => load::doctor(),
                };
                self.screen = screen;
                task
            }
            // Opening a card is navigating to its detail, and gets the same
            // treatment rather than a second, quietly different one.
            Message::Open(name) => self.update(Message::Go(Screen::Detail(name))),
            Message::Environments(Ok(rows)) => {
                self.envs = rows;
                self.loading_envs = false;
                self.dismiss_stale_load_error();
                Task::none()
            }
            Message::Environments(Err(e)) => {
                self.loading_envs = false;
                self.offer = Some(Offer::load_error(e));
                Task::none()
            }
            Message::Bases(Ok(rows)) => {
                self.bases = rows;
                self.dismiss_stale_load_error();
                Task::none()
            }
            Message::Bases(Err(e)) => {
                self.offer = Some(Offer::load_error(e));
                Task::none()
            }
            Message::Doctor(rows) => {
                self.checks = rows;
                Task::none()
            }
            // A poll that arrives while the last one is still reading `/proc`
            // is dropped rather than queued: the answer it would fetch is the
            // answer already on its way.
            Message::Refresh => {
                if self.loading_envs {
                    Task::none()
                } else {
                    self.reload_environments()
                }
            }
            Message::Start(name) => act(name, |e| e.ensure_session().map(|_| ())),
            Message::Stop(name) => act(name, |e| e.stop().map(|_| ())),
            Message::Acted(Ok(())) => {
                self.dismiss_offer();
                self.reload_environments()
            }
            // The reload still happens: whatever the action did before it
            // failed is part of the state. It no longer takes the banner with
            // it - that is what `dismiss_stale_load_error` is careful about.
            Message::Acted(Err(e)) => {
                self.offer = Some(errors::explain(&e));
                self.reload_environments()
            }
            Message::InstallD3d { env, vkd3d } => {
                let which = if vkd3d { "vkd3d" } else { "dxvk" };
                self.offer = Some(Offer::notice(format!(
                    "Install it from a build you already have:  raven env {which} {env} --from <path>"
                )));
                Task::none()
            }
            Message::RemoveD3d { env, vkd3d } => act(env, move |e| {
                let rt = if vkd3d {
                    &raven::d3d::VKD3D
                } else {
                    &raven::d3d::DXVK
                };
                e.remove_d3d(rt).map(|_| ())
            }),
            Message::Detach { env, letter } => act(env, move |e| e.detach(letter)),
            Message::Reproject(env) => act(env, |e| e.project_registry().map(|_| ())),
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
                        self.dismiss_offer();
                    }
                    _ => {
                        self.offer = Some(Offer::action_error(
                            "Fill in the image path, a numeric edition, and a name.".into(),
                        ));
                    }
                }
                Task::none()
            }
            Message::DeployProgress(progress) => {
                self.deploying = Some(progress);
                Task::none()
            }
            Message::DeployDone(outcome) => {
                self.deploy_job = None;
                self.deploying = None;
                match outcome {
                    Ok(()) => {
                        self.deploy_form = DeployForm::default();
                        load::bases()
                    }
                    Err(reason) => {
                        self.offer =
                            Some(Offer::action_error(format!("Deployment failed. {reason}")));
                        Task::none()
                    }
                }
            }
        }
    }

    /// The two things that run without being asked.
    ///
    /// A deployment's stream, while one is running: `deploy_job` - not
    /// `deploying` - is the identity `Subscription::run_with` hashes on, so
    /// editing the progress text alone can never be mistaken for a new job.
    ///
    /// And the design's one clock. Who holds a session changes without the
    /// window doing anything, so the environments screen and its details poll
    /// for it - and nothing else does, because reading `/proc` for every
    /// process on the machine on behalf of a screen nobody is looking at is
    /// exactly the waste the design refuses.
    fn subscription(&self) -> Subscription<Message> {
        let deploying = match &self.deploy_job {
            Some(args) => Subscription::run_with(args.clone(), deploy::run),
            None => Subscription::none(),
        };
        let polling = match self.screen {
            Screen::Environments | Screen::Detail(_) => {
                iced::time::every(POLL).map(|_| Message::Refresh)
            }
            Screen::Bases | Screen::Doctor => Subscription::none(),
        };
        Subscription::batch([deploying, polling])
    }

    fn view(&self) -> Element<'_, Message> {
        view::shell(self)
    }
}

/// Opens the environment on a blocking thread, runs one action against it,
/// and reports the outcome as `Message::Acted`.
///
/// Every action in the window has this shape - open, one library call, the
/// `Arc` bridge for the error - and writing it once means a sixth action
/// cannot quietly do any of it differently. Off the interface thread for the
/// reason `load` gives: the calls behind these read `/proc` and start
/// processes.
fn act(
    env: String,
    f: impl FnOnce(&raven::env::Environment) -> Result<(), raven::Error> + Send + 'static,
) -> Task<Message> {
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || f(&raven::env::Environment::open(&env)?))
                .await
                .expect("the blocking task panicked")
                .map_err(std::sync::Arc::new)
        },
        Message::Acted,
    )
}

fn main() -> iced::Result {
    // `ensure_session` starts the anchor as `current_exe() session-anchor
    // <name>` - whichever binary asked. From the window, that is this one,
    // so it answers here, before iced and tokio exist: the anchor's
    // `unshare(CLONE_NEWUSER)` is refused to a process that has threads.
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("session-anchor") {
        match args.next() {
            Some(name) => raven::session::anchor(&name),
            None => {
                // The launcher reads stdout and nothing else.
                println!("session-anchor needs an environment name");
                std::process::exit(1);
            }
        }
    }
    iced::application(App::boot, App::update, App::view)
        .subscription(App::subscription)
        .title("Raven")
        .default_font(theme::APP_FONT)
        // A Wayland compositor identifies a window by the application id it
        // reports and looks for a desktop file with that basename; iced leaves
        // it empty, so nothing would ever match `raven-gui.desktop` and the
        // `Icon=raven` it carries would never reach a taskbar. The three-way
        // agreement this belongs to is written down in assets/brand/README.md.
        .window(iced::window::Settings {
            platform_specific: iced::window::settings::PlatformSpecific {
                application_id: "raven-gui".to_owned(),
                ..Default::default()
            },
            ..Default::default()
        })
        .run()
}
