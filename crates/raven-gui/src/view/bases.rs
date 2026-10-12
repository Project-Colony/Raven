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

use crate::app::{DeployForm, Message};
use crate::deploy::Progress;
use crate::i18n;
use crate::model::BaseRow;
use crate::theme;

pub fn screen<'a>(
    bases: &'a [BaseRow],
    deploying: Option<&'a Progress>,
    form: &'a DeployForm,
) -> Element<'a, Message> {
    let t = theme::typography();
    let p = theme::palette();

    let list: Element<'_, Message> = if bases.is_empty() {
        text(i18n::t("bases_empty"))
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
            text(i18n::t("nav_bases"))
                .size(t.sz(22))
                .color(p.text_primary),
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

    container(
        column![
            text(b.id.clone()).size(t.sz(16)).color(p.text_primary),
            text(i18n::tn("bases_environments", b.environments, &[]))
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
            i18n::t("deploy_image"),
            &form.image,
            Message::DeployImageChanged
        )
        .width(Length::FillPortion(3)),
        field(
            i18n::t("deploy_edition"),
            &form.edition,
            Message::DeployEditionChanged
        )
        .width(Length::FillPortion(1)),
        field(
            i18n::t("deploy_name"),
            &form.name,
            Message::DeployNameChanged
        )
        .width(Length::FillPortion(1)),
    ]
    .spacing(t.sz(8));

    let deploy_button =
        button(text(i18n::t("action_deploy")).size(t.sz(13))).style(theme::button_style(p));
    let deploy_button = if busy {
        deploy_button
    } else {
        deploy_button.on_press(Message::DeployStart)
    };

    let mut section = column![
        text(i18n::t("deploy_title"))
            .size(t.sz(14))
            .color(p.text_secondary),
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
                text(i18n::tf(
                    "deploy_progress",
                    &[
                        ("what", &phase(&progress.what)),
                        ("percent", &progress.percent)
                    ]
                ))
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

/// wimlib's name for what it is doing, in the window's language when it is
/// one of the phases a deployment goes through, and as wimlib wrote it
/// otherwise.
fn phase(what: &str) -> &str {
    match what {
        "Starting" => i18n::t("deploy_starting"),
        "Creating files" => i18n::t("deploy_phase_creating_files"),
        "Extracting file data" => i18n::t("deploy_phase_extracting"),
        other => other,
    }
}
