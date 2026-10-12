//! Raven's administration window.
//!
//! A second caller of the same library the command line uses - see
//! docs/internals/gui.md. Nothing here decides
//! anything about environments; it draws what the library reports and asks the
//! library to act.

mod app;
mod deploy;
mod errors;
mod i18n;
mod load;
mod model;
mod theme;
mod view;

/// The program's name, which no language translates.
const PROGRAM: &str = "Raven";

/// Answers a command line rather than opening a window, and never returns.
///
/// The library re-runs a Raven binary for the two things it cannot do in the
/// calling process - holding a session open, and mounting for a registry
/// import - and it prefers the `raven` beside us for both. Should it ever
/// fall back to this binary, falling through to iced would put a second,
/// unexplained window on the user's screen and, worse, hand the caller the
/// exit status of a window they closed: a registry import that projected
/// nothing would report success. So anything that is not a window is
/// answered here, before iced and tokio exist - which `session-anchor` needs
/// anyway, since its `unshare(CLONE_NEWUSER)` is refused to a process that
/// already has threads.
///
/// Everything is said on stdout: that is the stream the library reads.
fn answer(verb: &str) -> ! {
    match verb {
        "session-anchor" => match std::env::args().nth(2) {
            Some(name) => raven::session::anchor(&name),
            None => {
                println!("{}", i18n::t("answer_anchor_needs_name"));
                std::process::exit(1);
            }
        },
        "--help" | "-h" => {
            println!("{}", i18n::t("answer_help"));
            std::process::exit(0);
        }
        "--version" | "-V" => {
            println!("raven-gui {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        other => {
            println!(
                "{}",
                i18n::tf(
                    "answer_unknown",
                    &[("quoted", &format!("{other:?}")), ("verb", &other)]
                )
            );
            std::process::exit(1);
        }
    }
}

fn main() -> iced::Result {
    // Before anything is said, the session anchor's reports included.
    i18n::set_language(raven::i18n::Lang::from_env());
    if let Some(verb) = std::env::args().nth(1) {
        answer(&verb);
    }
    iced::application(app::App::boot, app::App::update, app::App::view)
        .subscription(app::App::subscription)
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
