//! The Preferences page: colony-ui's skeleton, with Raven's language in
//! General, an Effects section that says Raven has none, and About last.
//!
//! The page replaces the screen and the sidebar stays (design/settings-page.md).
//! Every section arrives collapsed, and there is no Save button: a change is
//! written the moment it is made.

use colony_ui::Typography;
use colony_ui::widgets::{self, ABOUT_CATEGORY, PreferencesView, STANDARD_CATEGORIES};
use iced::Element;
use iced::widget::{Space, column};

use crate::app::{App, Message};
use crate::i18n;
use crate::preferences::{Language, Preferences};

pub fn page<'a>(app: &'a App, t: &Typography) -> Element<'a, Message> {
    let kit = PreferencesView::new(
        *t,
        &app.prefs.standard,
        &app.expanded,
        Message::Preference,
        Message::ToggleSection,
    );

    let content: Element<'a, Message> = match app.preferences_category {
        0 => kit.general().push(language(app, t)).into(),
        // Appearance as the kit draws it, but for Effects: the kit's
        // animations switch would do nothing here, because Raven animates
        // nothing, and a switch that does nothing says something false.
        1 => widgets::category_heading(
            t,
            i18n::t("settings_cat_appearance"),
            i18n::t("settings_appearance_desc"),
        )
        .push(kit.theme_section())
        .push(kit.colors_section())
        .push(kit.typography_section())
        .push(section(
            app,
            t,
            "effects",
            i18n::t("settings_section_effects"),
            column![widgets::section_description(
                t,
                i18n::t("preferences_effects_none")
            )],
        ))
        .push(kit.preview_section())
        .into(),
        2 => kit.accessibility().into(),
        _ => about(t),
    };

    let labels = STANDARD_CATEGORIES
        .into_iter()
        .chain([ABOUT_CATEGORY])
        .map(i18n::t);

    widgets::preferences_page(
        t,
        widgets::category_list(
            t,
            labels,
            app.preferences_category,
            Message::PreferencesCategory,
        ),
        content,
        Message::TogglePreferences,
    )
}

/// One collapsible section, open when the user opened it.
fn section<'a>(
    app: &App,
    t: &Typography,
    key: &'static str,
    title: &str,
    content: iced::widget::Column<'a, Message>,
) -> Element<'a, Message> {
    widgets::collapsible_section(
        t,
        title,
        app.expanded.contains(key),
        Message::ToggleSection(key),
        content.into(),
    )
}

/// General → Language: the system's, or one chosen here, applied at once.
fn language<'a>(app: &App, t: &Typography) -> Element<'a, Message> {
    let options = Language::ALL.map(|l| {
        let key = match l {
            Language::System => "preferences_language_system",
            Language::En => "preferences_language_en",
            Language::Fr => "preferences_language_fr",
        };
        (l, i18n::t(key).to_owned())
    });
    section(
        app,
        t,
        "language",
        i18n::t("preferences_section_language"),
        column![
            widgets::section_description(t, i18n::t("preferences_language_desc")),
            widgets::setting_row(
                t,
                i18n::t("preferences_language"),
                i18n::t("preferences_language_row_desc"),
                widgets::dropdown(t, options, &app.prefs.language, Message::Language),
            ),
        ],
    )
}

/// About: the version, the licence, and where Raven keeps its files - shown,
/// not created.
fn about<'a>(t: &Typography) -> Element<'a, Message> {
    let shown = |p: std::io::Result<std::path::PathBuf>| match p {
        Ok(p) => p.display().to_string(),
        Err(e) => e.to_string(),
    };
    let row = |title: &'static str, value: String| {
        widgets::setting_row(t, i18n::t(title), &value, Space::new())
    };
    use colony_ui::paths::locate;
    widgets::category_heading(
        t,
        i18n::t(ABOUT_CATEGORY),
        i18n::t("preferences_about_desc"),
    )
    .push(row(
        "preferences_about_version",
        env!("CARGO_PKG_VERSION").to_owned(),
    ))
    .push(row(
        "preferences_about_license",
        "GPL-3.0-or-later".to_owned(),
    ))
    .push(row(
        "preferences_about_preferences",
        shown(Preferences::path()),
    ))
    .push(row(
        "preferences_about_config",
        shown(locate::config_dir(crate::PROGRAM)),
    ))
    .push(row(
        "preferences_about_data",
        shown(locate::data_dir(crate::PROGRAM)),
    ))
    .push(row(
        "preferences_about_cache",
        shown(locate::cache_dir(crate::PROGRAM)),
    ))
    .into()
}
