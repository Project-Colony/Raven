//! Raven's administration window.
//!
//! A second caller of the same library the command line uses - see
//! docs/superpowers/specs/2026-09-01-raven-gui-design.md. Nothing here decides
//! anything about environments; it draws what the library reports and asks the
//! library to act.

mod theme;

use iced::widget::{column, container, text};
use iced::{Element, Length, Task};

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
}

#[derive(Default)]
pub struct App {
    screen: Screen,
}

impl App {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Go(screen) => {
                self.screen = screen;
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let t = theme::typography();
        let p = theme::palette();
        container(column![text("Raven").size(t.sz(30)).color(p.text_primary)].spacing(t.sz(8)))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(t.sz(16) as u16)
            .style(move |_| container::Style {
                background: Some(p.bg_primary.into()),
                ..Default::default()
            })
            .into()
    }
}

fn main() -> iced::Result {
    iced::application(App::default, App::update, App::view)
        .title("Raven")
        .default_font(theme::APP_FONT)
        .run()
}
