//! One environment, in full: its session, its Direct3D runtimes, its devices
//! and its registry rules.

use iced::widget::{Space, button, column, container, row, text};
use iced::{Element, Length};

use crate::Message;
use crate::model::EnvRow;
use crate::theme;

pub fn screen(e: &EnvRow) -> Element<'_, Message> {
    let t = theme::typography();
    let p = theme::palette();

    let heading = |s: &'static str| text(s).size(t.sz(14)).color(p.text_secondary);

    let session = column![
        heading("Session"),
        text(e.status_line()).size(t.sz(13)).color(p.text_primary),
        if e.is_running() {
            button(text("Stop").size(t.sz(12))).on_press(Message::Stop(e.name.clone()))
        } else {
            button(text("Start").size(t.sz(12))).on_press(Message::Start(e.name.clone()))
        },
    ]
    .spacing(t.sz(6));

    let runtime = |label: &'static str, build: &Option<String>, vkd3d: bool| {
        let name = e.name.clone();
        match build {
            Some(v) => row![
                text(format!("{label}: {v}"))
                    .size(t.sz(13))
                    .color(p.text_primary),
                button(text("Remove").size(t.sz(12)))
                    .on_press(Message::RemoveD3d { env: name, vkd3d }),
            ],
            None => row![
                text(format!("{label}: not installed"))
                    .size(t.sz(13))
                    .color(p.text_muted),
                button(text("Install…").size(t.sz(12)))
                    .on_press(Message::InstallD3d { env: name, vkd3d }),
            ],
        }
        .spacing(t.sz(8))
        .align_y(iced::Alignment::Center)
    };

    let d3d = column![
        heading("Direct3D"),
        runtime("DXVK (D3D 8-11)", &e.dxvk, false),
        runtime("vkd3d-proton (D3D 12)", &e.vkd3d, true),
        text("They are separate runtimes, not versions of one.")
            .size(t.sz(11))
            .color(p.text_muted),
    ]
    .spacing(t.sz(6));

    let mut devices = column![heading("Devices")].spacing(t.sz(6));
    if e.attachments.is_empty() {
        devices = devices.push(
            text("None attached. Attach one with:  raven env attach <name> /dev/sdX")
                .size(t.sz(12))
                .color(p.text_muted),
        );
    } else {
        for (letter, device) in &e.attachments {
            devices = devices.push(
                row![
                    text(format!("{letter}:  {}", device.display()))
                        .size(t.sz(13))
                        .color(p.text_primary),
                    button(text("Detach").size(t.sz(12))).on_press(Message::Detach {
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
        heading("Registry"),
        text("Re-run the projection after editing registry-rules.toml.")
            .size(t.sz(12))
            .color(p.text_muted),
        button(text("Reproject").size(t.sz(12))).on_press(Message::Reproject(e.name.clone())),
    ]
    .spacing(t.sz(6));

    container(
        column![
            row![
                button(text("← Environments").size(t.sz(12)))
                    .on_press(Message::Go(crate::Screen::Environments)),
                Space::new().width(Length::Fill),
            ],
            text(e.name.clone()).size(t.sz(22)).color(p.text_primary),
            text(e.base.clone()).size(t.sz(12)).color(p.text_muted),
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
