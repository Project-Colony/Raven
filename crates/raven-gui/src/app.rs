//! The window's state, and everything that changes it: the messages, `update`,
//! and the subscriptions that run without being asked.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use colony_ui::preferences::Change;
use iced::keyboard::{self, key};
use iced::{Element, Subscription, Task};

use crate::errors::{self, Offer};
use crate::i18n;
use crate::model::{BaseRow, Check, EnvRow};
use crate::preferences::{Language, Preferences};
use crate::{deploy, load, view};

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
    /// A stop offered on the promise that only Raven's anchor holds the
    /// environment. Distinct from `Stop` because that promise is checked
    /// again before it acts; see `errors::Action::message`.
    StopSession(String),
    // `raven::Error` holds `std::io::Error` in some variants, so it cannot be
    // `Clone` - and iced's widgets (`Button::on_press` among them) require
    // `Message: Clone`. `Arc` is `Clone` regardless of what it wraps, so it
    // carries the error across that boundary without the library changing.
    Acted(Result<(), std::sync::Arc<raven::Error>>),
    Open(String),
    InstallD3d {
        env: String,
        vkd3d: bool,
    },
    RemoveD3d {
        env: String,
        vkd3d: bool,
    },
    Detach {
        env: String,
        letter: char,
    },
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
    /// Sent by the program's name and by the Preferences page's Close alike:
    /// the way in is the way out.
    TogglePreferences,
    PreferencesCategory(usize),
    /// Opens or closes one collapsible section of the Preferences page.
    ToggleSection(&'static str),
    /// A change from colony-ui's Appearance and Accessibility controls.
    Preference(Change),
    Language(Language),
    DismissToast,
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
    /// What the user chose in Preferences, written back on every change.
    pub(crate) prefs: Preferences,
    /// Whether the Preferences page stands in for the screen. A flag rather
    /// than a `Screen`, so closing it returns to whatever was showing.
    pub(crate) preferences_open: bool,
    pub(crate) preferences_category: usize,
    /// The Preferences sections the user opened; every one starts closed.
    pub(crate) expanded: HashSet<String>,
    /// "Theme applied.", after a theme change that could otherwise go
    /// unnoticed between neighbouring variants.
    pub(crate) toast: Option<&'static str>,
}

impl App {
    /// The window and the first load, with `prefs` already in effect.
    /// `reload_environments` rather than `load::environments` so
    /// `loading_envs` is true from the first frame: the boot load and the
    /// first poll must not both be in flight.
    pub(crate) fn boot(prefs: Preferences) -> (Self, Task<Message>) {
        let mut app = Self {
            prefs,
            ..Self::default()
        };
        let task = app.reload_environments();
        (app, task)
    }

    /// Writes the preferences; a failure is said, not swallowed, since the
    /// change still applies until the window closes and then would not.
    fn save_preferences(&mut self) -> Task<Message> {
        if let Err(e) = self.prefs.save() {
            self.offer = Some(Offer::action_error(i18n::tf(
                "preferences_not_saved",
                &[("why", &e)],
            )));
        }
        Task::none()
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

    pub(crate) fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Go(screen) => {
                self.dismiss_offer();
                self.preferences_open = false;
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
            Message::Start(name) => act(name, |e| {
                // Mounting is the cheap half. The card's Start exists because
                // `env start` makes a launch soon after it fast, and what makes
                // it fast is Wine's services already standing - so this
                // waits for them too, rather than flipping the card to
                // "running" and leaving the first launch to pay the seconds
                // the button was pressed to avoid. Their failure is not this
                // action's failure: the session is up either way.
                e.ensure_session()?;
                e.warm_up();
                Ok(())
            }),
            // What every Stop control the user can see asks for: the card's,
            // the detail screen's, and the banner that already says it will
            // end the programs it lists. Wine's services hold the mount from
            // the moment an environment is started, so a stop that refused
            // while anything but the anchor held it would refuse always.
            Message::Stop(name) => act(name, |e| e.stop().map(|_| ())),
            Message::StopSession(name) => act(name, |e| {
                // This one is offered on the library's judgement that only
                // Raven's own anchor holds the environment, and the banner
                // carrying it is deliberately long-lived - exempt from the
                // poll that clears the others. A program started in between
                // would be killed by a button whose words promised it would
                // not be, so the judgement is taken again; when it has
                // changed, the error relabels the banner honestly.
                match e.ensure_not_running() {
                    Err(busy @ raven::Error::EnvironmentBusy { .. }) => Err(busy),
                    _ => e.stop().map(|_| ()),
                }
            }),
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
                self.offer = Some(Offer::notice(i18n::tf(
                    "install_d3d_notice",
                    &[("which", &which), ("env", &env)],
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
                        // Named the way wimlib names its phases, so the view
                        // words it with them.
                        self.deploying = Some(deploy::Progress {
                            percent: 0,
                            what: "Starting".into(),
                        });
                        self.dismiss_offer();
                    }
                    _ => {
                        self.offer = Some(Offer::action_error(
                            i18n::t("deploy_incomplete_form").into(),
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
                        self.offer = Some(Offer::action_error(i18n::tf(
                            "deploy_failed",
                            &[("reason", &reason)],
                        )));
                        Task::none()
                    }
                }
            }
            Message::TogglePreferences => {
                self.preferences_open = !self.preferences_open;
                self.preferences_category = 0;
                if self.preferences_open {
                    self.dismiss_offer();
                    Task::none()
                } else {
                    // Back to the screen as it is now, not as it was when
                    // Preferences opened: the poll stood still meanwhile.
                    self.update(Message::Go(self.screen.clone()))
                }
            }
            Message::PreferencesCategory(index) => {
                self.preferences_category = index;
                Task::none()
            }
            Message::ToggleSection(key) => {
                if !self.expanded.remove(key) {
                    self.expanded.insert(key.to_owned());
                }
                Task::none()
            }
            Message::Preference(change) => {
                self.prefs.standard.update(change);
                if matches!(change, Change::Theme { .. }) {
                    self.toast = Some(i18n::t("theme_applied"));
                }
                self.save_preferences()
            }
            Message::Language(language) => {
                self.prefs.language = language;
                i18n::set_language(language.resolve());
                self.save_preferences()
            }
            Message::DismissToast => {
                self.toast = None;
                Task::none()
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
    pub(crate) fn subscription(&self) -> Subscription<Message> {
        let deploying = match &self.deploy_job {
            Some(args) => Subscription::run_with(args.clone(), deploy::run),
            None => Subscription::none(),
        };
        let polling = match self.screen {
            _ if self.preferences_open => Subscription::none(),
            Screen::Environments | Screen::Detail(_) => {
                iced::time::every(POLL).map(|_| Message::Refresh)
            }
            Screen::Bases | Screen::Doctor => Subscription::none(),
        };
        let shortcuts = keyboard::listen().filter_map(shortcut);
        Subscription::batch([deploying, polling, shortcuts])
    }

    pub(crate) fn view(&self) -> Element<'_, Message> {
        view::shell(self)
    }
}

/// Ctrl+1, Ctrl+2 and Ctrl+3: the three screens, in sidebar order.
///
/// Read by physical key rather than by character, so the shortcut is the
/// same on AZERTY, where the key marked 1 types `&` without Shift.
fn shortcut(event: keyboard::Event) -> Option<Message> {
    let keyboard::Event::KeyPressed {
        physical_key: key::Physical::Code(code),
        modifiers,
        ..
    } = event
    else {
        return None;
    };
    if !modifiers.command() {
        return None;
    }
    let screen = match code {
        key::Code::Digit1 | key::Code::Numpad1 => Screen::Environments,
        key::Code::Digit2 | key::Code::Numpad2 => Screen::Bases,
        key::Code::Digit3 | key::Code::Numpad3 => Screen::Doctor,
        _ => return None,
    };
    Some(Message::Go(screen))
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
