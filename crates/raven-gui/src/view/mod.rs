//! Drawing. Nothing in here decides anything - `update` does that.

pub mod bases;
pub mod detail;
pub mod doctor;
pub mod environments;
pub mod preferences;

use colony_ui::{Typography, widgets};
use iced::widget::{Space, button, column, container, row, stack, text};
use iced::{Element, Length};

use crate::app::{App, Message, Screen};
use crate::{i18n, theme};

/// The sidebar, and the selected screen or the Preferences page beside it.
///
/// The sidebar follows design/navigation.md: the program's name is the way
/// into Preferences and shows whether they are open, then the list's label,
/// the three screens, and the shortcut hint at the bottom, the quietest text
/// in the window. It grows with the text so every label still fits at 1.68x
/// and in French.
pub fn shell(app: &App) -> Element<'_, Message> {
    let t = Typography::current();
    let p = theme::palette();

    // An environment's detail is the Environments screen one level down:
    // `Open` is `Go(Detail)` and its back button returns here. So that entry
    // stays lit while a detail shows, rather than the whole sidebar going dark
    // on the most common navigation in the window. Nothing is lit while
    // Preferences stands in for the screens.
    let item = |key: &'static str, screen: Screen| {
        let selected = !app.preferences_open
            && match (&screen, &app.screen) {
                (Screen::Environments, Screen::Detail(_)) => true,
                (screen, current) => screen == current,
            };
        widgets::nav_item(&t, i18n::t(key), selected, Message::Go(screen))
    };

    let sidebar = container(
        column![
            widgets::identity_button(
                &t,
                crate::PROGRAM,
                30,
                app.preferences_open,
                Message::TogglePreferences
            ),
            Space::new().height(t.sz(16)),
            widgets::nav_label(&t, i18n::t("nav_label")),
            item("nav_environments", Screen::Environments),
            item("nav_bases", Screen::Bases),
            item("nav_diagnostics", Screen::Doctor),
            Space::new().height(Length::Fill),
            widgets::nav_hint(&t, i18n::t("nav_hint")),
        ]
        .spacing(t.sz(4)),
    )
    .width(Length::Fixed(210.0 * t.scale))
    .height(Length::Fill)
    .padding(t.sz(12) as u16)
    .style(move |_| container::Style {
        background: Some(p.bg_sidebar.into()),
        ..Default::default()
    });

    let body = if app.preferences_open {
        preferences::page(app, &t)
    } else {
        match &app.screen {
            Screen::Environments => environments::screen(&app.envs),
            Screen::Detail(name) => match app.envs.iter().find(|e| &e.name == name) {
                Some(e) => detail::screen(e),
                // The environment was destroyed from elsewhere between the
                // click and the draw. Falling back is better than an empty page.
                None => environments::screen(&app.envs),
            },
            Screen::Bases => bases::screen(&app.bases, app.deploying.as_ref(), &app.deploy_form),
            Screen::Doctor => doctor::screen(&app.checks),
        }
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
            let mut r = row![
                text(offer.message.clone())
                    .size(t.sz(13))
                    .font(t.regular)
                    .color(ink)
                    .width(Length::Fill)
            ]
            .spacing(t.sz(8))
            .align_y(iced::Alignment::Center);
            // Both actions stop the environment; only the words differ, and
            // they are the action's own so the view cannot mislabel one.
            if let Some(action) = &offer.action {
                r = r.push(
                    button(text(action.label()).size(t.sz(12)).font(t.regular))
                        .style(theme::button_style(p))
                        .on_press(action.message()),
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

    let main: Element<'_, Message> = column![banner, body].spacing(t.sz(8)).into();
    // The theme_applied toast, over the bottom of whatever is showing.
    let main: Element<'_, Message> = match app.toast {
        Some(message) => stack![
            main,
            container(widgets::toast(&t, message, Message::DismissToast))
                .padding(t.sz(16) as u16)
                .align_bottom(Length::Fill),
        ]
        .into(),
        None => main,
    };

    container(row![
        sidebar,
        container(main)
            .padding(t.sz(16) as u16)
            .width(Length::Fill)
            .height(Length::Fill)
    ])
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |_| container::Style {
        background: Some(p.bg_primary.into()),
        ..Default::default()
    })
    .into()
}
