//! Diagnostics: the same four judgements `raven doctor` prints, from the same
//! functions. A green tick and a red cross would throw away the only useful
//! half of what the CLI says - "absent - Wine falls back to wineserver for NT
//! synchronization" rather than "no" - so every row, passing or not, keeps
//! its consequence in the detail line.

use iced::widget::{Space, column, container, scrollable, text};
use iced::{Element, Length};

use crate::Message;
use crate::model::Check;
use crate::theme;

pub fn screen(checks: &[Check]) -> Element<'_, Message> {
    let t = theme::typography();
    let p = theme::palette();

    let list: Element<'_, Message> = if checks.is_empty() {
        text("No diagnostics yet")
            .size(t.sz(13))
            .color(p.text_muted)
            .into()
    } else {
        let cards = checks
            .iter()
            .fold(column![].spacing(t.sz(10)), |acc, c| acc.push(card(c)));
        scrollable(cards).height(Length::Fill).into()
    };

    container(
        column![
            text("Diagnostics").size(t.sz(22)).color(p.text_primary),
            Space::new().height(t.sz(8)),
            list,
        ]
        .spacing(t.sz(4)),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn card(c: &Check) -> Element<'_, Message> {
    let t = theme::typography();
    let p = theme::palette();

    container(
        column![
            text(c.label.clone()).size(t.sz(16)).color(p.text_primary),
            text(c.detail.clone())
                .size(t.sz(12))
                .color(if c.ok { p.success } else { p.error }),
        ]
        .spacing(t.sz(3)),
    )
    .padding(t.sz(12) as u16)
    .width(Length::Fill)
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
