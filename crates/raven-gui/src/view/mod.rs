//! Drawing. Nothing in here decides anything - `update` does that.

pub mod environments;

use iced::widget::{Space, button, column, container, row, text};
use iced::{Element, Length};

use crate::theme;
use crate::{App, Message, Screen};

/// The sidebar and whichever screen is selected.
pub fn shell(app: &App) -> Element<'_, Message> {
    let t = theme::typography();
    let p = theme::palette();

    let item = |label: &'static str, screen: Screen, current: Screen| {
        button(text(label).size(t.sz(13)))
            .width(Length::Fill)
            .style(move |_, _| button::Style {
                background: Some(
                    if screen == current {
                        p.bg_selected
                    } else {
                        p.bg_sidebar
                    }
                    .into(),
                ),
                text_color: if screen == current {
                    p.accent_blue
                } else {
                    p.text_secondary
                },
                ..Default::default()
            })
            .on_press(Message::Go(screen))
    };

    let sidebar = container(
        column![
            text("Raven").size(t.sz(20)).color(p.text_primary),
            Space::new().height(t.sz(12)),
            item("Environments", Screen::Environments, app.screen),
            item("Bases", Screen::Bases, app.screen),
            item("Diagnostics", Screen::Doctor, app.screen),
        ]
        .spacing(t.sz(4)),
    )
    .width(Length::Fixed(t.sz(180)))
    .height(Length::Fill)
    .padding(t.sz(12) as u16)
    .style(move |_| container::Style {
        background: Some(p.bg_sidebar.into()),
        ..Default::default()
    });

    let body = match app.screen {
        Screen::Environments => environments::screen(&app.envs),
        Screen::Bases => text("Bases").size(t.sz(22)).color(p.text_primary).into(),
        Screen::Doctor => text("Diagnostics")
            .size(t.sz(22))
            .color(p.text_primary)
            .into(),
    };

    let banner: Element<'_, Message> = match &app.offer {
        None => Space::new().height(0).into(),
        Some(offer) => {
            let mut r = row![text(offer.message.clone()).size(t.sz(13)).color(p.error)]
                .spacing(t.sz(8))
                .align_y(iced::Alignment::Center);
            if let Some(crate::errors::Action::Stop(name)) = &offer.action {
                r = r.push(
                    button(text("Stop the session").size(t.sz(12)))
                        .on_press(Message::Stop(name.clone())),
                );
            }
            container(r)
                .padding(t.sz(10) as u16)
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(p.error_bg.into()),
                    ..Default::default()
                })
                .into()
        }
    };

    container(row![
        sidebar,
        container(column![banner, body].spacing(t.sz(8)))
            .padding(t.sz(16) as u16)
            .width(Length::Fill)
    ])
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |_| container::Style {
        background: Some(p.bg_primary.into()),
        ..Default::default()
    })
    .into()
}
