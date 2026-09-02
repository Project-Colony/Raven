//! Deployed bases, and the form and progress bar for deploying one more.
//!
//! Deploying is the only long operation in Raven - minutes of silence over
//! 143 886 files in the CLI - so this is the one screen with a
//! `progress_bar`: real feedback for the one wait a user cannot otherwise
//! tell from a hang.

use iced::widget::{
    Space, button, column, container, progress_bar, row, scrollable, text, text_input,
};
use iced::{Element, Length};

use crate::deploy::Progress;
use crate::model::BaseRow;
use crate::theme;
use crate::{DeployForm, Message};

pub fn screen<'a>(
    bases: &'a [BaseRow],
    deploying: Option<&'a Progress>,
    form: &'a DeployForm,
) -> Element<'a, Message> {
    let t = theme::typography();
    let p = theme::palette();

    let list: Element<'_, Message> = if bases.is_empty() {
        text("No bases yet")
            .size(t.sz(13))
            .color(p.text_muted)
            .into()
    } else {
        let cards = bases
            .iter()
            .fold(column![].spacing(t.sz(10)), |acc, b| acc.push(card(b)));
        scrollable(cards).height(Length::Fill).into()
    };

    container(
        column![
            text("Bases").size(t.sz(22)).color(p.text_primary),
            Space::new().height(t.sz(8)),
            list,
            Space::new().height(t.sz(16)),
            deploy_form(deploying, form),
        ]
        .spacing(t.sz(4)),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn card(b: &BaseRow) -> Element<'_, Message> {
    let t = theme::typography();
    let p = theme::palette();

    let plural = if b.environments == 1 {
        "environment"
    } else {
        "environments"
    };

    container(
        column![
            text(b.id.clone()).size(t.sz(16)).color(p.text_primary),
            text(format!("{} {plural}", b.environments))
                .size(t.sz(12))
                .color(p.text_muted),
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

/// The three fields a deployment needs, and the Deploy button. Fields are
/// disabled while a deployment is running: `Subscription::run_with` keys on
/// them, so editing one mid-flight would restart the job rather than change
/// it.
fn deploy_form<'a>(deploying: Option<&'a Progress>, form: &'a DeployForm) -> Element<'a, Message> {
    let t = theme::typography();
    let p = theme::palette();
    let busy = deploying.is_some();

    let field =
        move |placeholder: &'static str, value: &'a str, on_input: fn(String) -> Message| {
            text_input(placeholder, value)
                .size(t.sz(13))
                .padding(t.sz(8) as u16)
                .on_input_maybe((!busy).then_some(on_input))
                .style(move |_, _| text_input::Style {
                    background: p.bg_input.into(),
                    border: iced::Border {
                        color: p.border_subtle,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    icon: p.text_muted,
                    placeholder: p.text_placeholder,
                    value: p.text_primary,
                    selection: p.accent_blue,
                })
        };

    let inputs = row![
        field(
            "Path to install.wim",
            &form.image,
            Message::DeployImageChanged
        )
        .width(Length::FillPortion(3)),
        field(
            "Edition index",
            &form.edition,
            Message::DeployEditionChanged
        )
        .width(Length::FillPortion(1)),
        field("Name", &form.name, Message::DeployNameChanged).width(Length::FillPortion(1)),
    ]
    .spacing(t.sz(8));

    let deploy_button = button(text("Deploy").size(t.sz(13)));
    let deploy_button = if busy {
        deploy_button
    } else {
        deploy_button.on_press(Message::DeployStart)
    };

    let mut section = column![
        text("Deploy a base").size(t.sz(14)).color(p.text_secondary),
        inputs,
        deploy_button,
    ]
    .spacing(t.sz(8));

    if let Some(progress) = deploying {
        section = section.push(
            row![
                progress_bar(0.0..=100.0, progress.percent as f32)
                    .girth(t.sz(10))
                    .length(Length::FillPortion(4))
                    .style(move |_| progress_bar::Style {
                        background: p.bg_progress.into(),
                        bar: p.accent_progress.into(),
                        border: iced::Border::default(),
                    }),
                text(format!("{} - {}%", progress.what, progress.percent))
                    .size(t.sz(12))
                    .color(p.text_muted),
            ]
            .spacing(t.sz(8))
            .align_y(iced::Alignment::Center),
        );
    }

    container(section)
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
