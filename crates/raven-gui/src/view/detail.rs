//! One environment, in full: its session, its Direct3D runtimes, its devices
//! and its registry rules.

use colony_ui::Typography;
use iced::widget::{Space, button, column, container, row, text};
use iced::{Element, Length};

use crate::app::Message;
use crate::i18n;
use crate::model::EnvRow;
use crate::theme;

pub fn screen(e: &EnvRow) -> Element<'_, Message> {
    let t = Typography::current();
    let p = theme::palette();

    let heading = |s: &'static str| {
        text(s)
            .size(t.sz(14))
            .font(t.regular)
            .color(p.text_secondary)
    };

    let session = column![
        heading(i18n::t("detail_session")),
        text(e.status_line())
            .size(t.sz(13))
            .font(t.regular)
            .color(p.text_primary),
        if e.is_running() {
            button(text(i18n::t("action_stop")).size(t.sz(12)).font(t.regular))
                .style(theme::button_style(p))
                .on_press(Message::Stop(e.name.clone()))
        } else {
            button(text(i18n::t("action_start")).size(t.sz(12)).font(t.regular))
                .style(theme::button_style(p))
                .on_press(Message::Start(e.name.clone()))
        },
    ]
    .spacing(t.sz(6));

    let runtime = |label: &'static str, build: &Option<String>, vkd3d: bool| {
        let name = e.name.clone();
        match build {
            Some(v) => row![
                text(format!("{label}: {v}"))
                    .size(t.sz(13))
                    .font(t.regular)
                    .color(p.text_primary),
                button(
                    text(i18n::t("action_remove"))
                        .size(t.sz(12))
                        .font(t.regular)
                )
                .style(theme::button_style(p))
                .on_press(Message::RemoveD3d { env: name, vkd3d }),
            ],
            None => row![
                text(i18n::tf("detail_runtime_missing", &[("runtime", &label)]))
                    .size(t.sz(13))
                    .font(t.regular)
                    .color(p.text_muted),
                button(
                    text(i18n::t("action_install"))
                        .size(t.sz(12))
                        .font(t.regular)
                )
                .style(theme::button_style(p))
                .on_press(Message::InstallD3d { env: name, vkd3d }),
            ],
        }
        .spacing(t.sz(8))
        .align_y(iced::Alignment::Center)
    };

    let d3d = column![
        heading(i18n::t("detail_direct3d")),
        runtime("DXVK (D3D 8-11)", &e.dxvk, false),
        runtime("vkd3d-proton (D3D 12)", &e.vkd3d, true),
        text(i18n::t("detail_separate_runtimes"))
            .size(t.sz(11))
            .font(t.regular)
            .color(p.text_muted),
    ]
    .spacing(t.sz(6));

    let mut devices = column![heading(i18n::t("detail_devices"))].spacing(t.sz(6));
    if e.attachments.is_empty() {
        devices = devices.push(
            text(i18n::t("detail_no_devices"))
                .size(t.sz(12))
                .font(t.regular)
                .color(p.text_muted),
        );
    } else {
        for (letter, device) in &e.attachments {
            devices = devices.push(
                row![
                    text(format!("{letter}:  {}", device.display()))
                        .size(t.sz(13))
                        .font(t.regular)
                        .color(p.text_primary),
                    button(
                        text(i18n::t("action_detach"))
                            .size(t.sz(12))
                            .font(t.regular)
                    )
                    .style(theme::button_style(p))
                    .on_press(Message::Detach {
                        env: e.name.clone(),
                        letter: *letter,
                    }),
                ]
                .spacing(t.sz(8))
                .align_y(iced::Alignment::Center),
            );
        }
    }

    let registry = column![
        heading(i18n::t("detail_registry")),
        text(i18n::t("detail_registry_hint"))
            .size(t.sz(12))
            .font(t.regular)
            .color(p.text_muted),
        button(
            text(i18n::t("action_reproject"))
                .size(t.sz(12))
                .font(t.regular)
        )
        .style(theme::button_style(p))
        .on_press(Message::Reproject(e.name.clone())),
    ]
    .spacing(t.sz(6));

    container(
        column![
            row![
                button(text(i18n::t("detail_back")).size(t.sz(12)).font(t.regular))
                    .style(theme::button_style(p))
                    .on_press(Message::Go(crate::app::Screen::Environments)),
                Space::new().width(Length::Fill),
            ],
            text(e.name.clone())
                .size(t.sz(22))
                .font(t.bold)
                .color(p.text_primary),
            text(e.base.clone())
                .size(t.sz(12))
                .font(t.regular)
                .color(p.text_muted),
            Space::new().height(t.sz(12)),
            session,
            Space::new().height(t.sz(12)),
            d3d,
            Space::new().height(t.sz(12)),
            devices,
            Space::new().height(t.sz(12)),
            registry,
        ]
        .spacing(t.sz(4)),
    )
    .width(Length::Fill)
    .into()
}
