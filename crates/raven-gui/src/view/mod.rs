//! Drawing. Nothing in here decides anything - `update` does that.

pub mod bases;
pub mod detail;
pub mod doctor;
pub mod environments;

use iced::widget::{Space, button, column, container, row, text};
use iced::{Element, Length};

use crate::theme;
use crate::{App, Message, Screen};

/// The sidebar and whichever screen is selected.
pub fn shell(app: &App) -> Element<'_, Message> {
    let t = theme::typography();
    let p = theme::palette();

    // `Screen::Detail` carries a `String`, so `Screen` is no longer `Copy`;
    // `current` is taken by reference so the same `&app.screen` can feed all
    // three calls below, and `selected` is settled before the `move` closure
    // so `screen` stays available afterward for `on_press`.
    let item = |label: &'static str, screen: Screen, current: &Screen| {
        // An environment's detail is the Environments screen one level down:
        // `Open` is `Go(Detail)` and its back button returns here. So that
        // entry stays lit while a detail shows, rather than the whole sidebar
        // going dark on the most common navigation in the window.
        let selected = match (&screen, current) {
            (Screen::Environments, Screen::Detail(_)) => true,
            _ => screen == *current,
        };
        button(text(label).size(t.sz(13)))
            .width(Length::Fill)
            .style(move |_, _| button::Style {
                background: Some(
                    if selected {
                        p.bg_selected
                    } else {
                        p.bg_sidebar
                    }
                    .into(),
                ),
                text_color: if selected {
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
            item("Environments", Screen::Environments, &app.screen),
            item("Bases", Screen::Bases, &app.screen),
            item("Diagnostics", Screen::Doctor, &app.screen),
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

    let body = match &app.screen {
        Screen::Environments => environments::screen(&app.envs),
        Screen::Detail(name) => match app.envs.iter().find(|e| &e.name == name) {
            Some(e) => detail::screen(e),
            // The environment was destroyed from elsewhere between the click and
            // the draw. Falling back is better than an empty page.
            None => environments::screen(&app.envs),
        },
        Screen::Bases => bases::screen(&app.bases, app.deploying.as_ref(), &app.deploy_form),
        Screen::Doctor => doctor::screen(&app.checks),
    };

    let banner: Element<'_, Message> = match &app.offer {
        None => Space::new().height(0).into(),
        Some(offer) => {
            // A notice is guidance the window cannot carry out itself, not a
            // failure, and painting it in the error colours would tell the user
            // something went wrong when nothing did.
            let (ink, ground) = match offer.kind {
                crate::errors::Kind::Notice => (p.text_primary, p.bg_modal_section),
                _ => (p.error, p.error_bg),
            };
            let mut r = row![text(offer.message.clone()).size(t.sz(13)).color(ink)]
                .spacing(t.sz(8))
                .align_y(iced::Alignment::Center);
            // Both actions stop the environment; only the words differ, and
            // they are the action's own so the view cannot mislabel one.
            if let Some(action) = &offer.action {
                r = r.push(
                    button(text(action.label()).size(t.sz(12)))
                        .style(theme::button_style(p))
                        .on_press(Message::Stop(action.env().to_owned())),
                );
            }
            container(r)
                .padding(t.sz(10) as u16)
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(ground.into()),
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
