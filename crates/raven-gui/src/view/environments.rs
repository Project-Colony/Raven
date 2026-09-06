//! One card per environment. Start and Stop sit on the card because starting
//! is the most frequent action and `env start` is what makes launches instant.

use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Element, Length};

use crate::Message;
use crate::model::EnvRow;
use crate::theme;

pub fn screen(envs: &[EnvRow]) -> Element<'_, Message> {
    let t = theme::typography();
    let p = theme::palette();

    if envs.is_empty() {
        return column![
            text("No environments yet")
                .size(t.sz(22))
                .color(p.text_primary),
            Space::new().height(t.sz(8)),
            text("Create one with:  raven env create <name> --base <base>")
                .size(t.sz(13))
                .color(p.text_muted),
        ]
        .into();
    }

    let cards = envs
        .iter()
        .fold(column![].spacing(t.sz(10)), |acc, e| acc.push(card(e)));

    scrollable(cards).height(Length::Fill).into()
}

fn card(e: &EnvRow) -> Element<'_, Message> {
    let t = theme::typography();
    let p = theme::palette();

    let action = if e.is_running() {
        button(text("Stop").size(t.sz(13)))
            .style(theme::button_style(p))
            .on_press(Message::Stop(e.name.clone()))
    } else {
        button(text("Start").size(t.sz(13)))
            .style(theme::button_style(p))
            .on_press(Message::Start(e.name.clone()))
    };

    let mut runtimes = row![].spacing(t.sz(8));
    if let Some(v) = &e.dxvk {
        runtimes = runtimes.push(text(v.clone()).size(t.sz(11)).color(p.text_muted));
    }
    if let Some(v) = &e.vkd3d {
        runtimes = runtimes.push(text(v.clone()).size(t.sz(11)).color(p.text_muted));
    }
    for (letter, device) in &e.attachments {
        runtimes = runtimes.push(
            text(format!("{letter}: {}", device.display()))
                .size(t.sz(11))
                .color(p.text_muted),
        );
    }

    // The name is the card's own heading, not a control beside it, so it is
    // drawn as text and only behaves like a button. No fill, and its colour is
    // stated rather than left to `Style::default()`, which is black.
    let name = button(text(e.name.clone()).size(t.sz(16)).color(p.text_primary))
        .style(move |_, _| button::Style {
            background: None,
            text_color: p.text_primary,
            ..Default::default()
        })
        .padding(0.0)
        .on_press(Message::Open(e.name.clone()));

    container(
        row![
            column![
                name,
                text(e.base.clone()).size(t.sz(11)).color(p.text_muted),
                text(e.status_line())
                    .size(t.sz(12))
                    .color(if e.is_running() {
                        p.success
                    } else {
                        p.text_muted
                    }),
                runtimes,
            ]
            .spacing(t.sz(3))
            .width(Length::Fill),
            action,
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(t.sz(12) as u16)
    .style(move |_| container::Style {
        background: Some(p.bg_card.into()),
        border: iced::Border {
            color: p.border_subtle,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    })
    .into()
}
