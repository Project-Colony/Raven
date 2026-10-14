//! What the user chose in Preferences, and the file it is kept in.
//!
//! colony-ui's `StandardPreferences` carries the theme, accent, both text
//! sizes and the accessibility switches, under the field names every Colony
//! program writes; Raven adds only the language. The file is
//! `~/.config/Colony/Raven/preferences/preferences.toml`, written on every
//! change - there is no Save button - and read once, before the first frame.
//! A file that cannot be read, or holds values this build does not know,
//! never stops the window: whatever is not understood takes its default.

use std::io;
use std::path::PathBuf;

use colony_ui::preferences::StandardPreferences;
use raven::i18n::Lang;
use serde::{Deserialize, Deserializer, Serialize};

/// The language the window speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// The locale's, the same rule as the command line's.
    #[default]
    System,
    En,
    Fr,
}

impl Language {
    pub const ALL: [Language; 3] = [Language::System, Language::En, Language::Fr];

    pub fn resolve(self) -> Lang {
        match self {
            Language::System => Lang::from_env(),
            Language::En => Lang::En,
            Language::Fr => Lang::Fr,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(flatten)]
    pub standard: StandardPreferences,
    #[serde(default, deserialize_with = "lenient_language")]
    pub language: Language,
}

/// A language this build does not know - one a newer build added, or a typo -
/// is the default, not a reason to throw the whole file away.
fn lenient_language<'de, D: Deserializer<'de>>(d: D) -> Result<Language, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Known(Language),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Raw::deserialize(d)? {
        Raw::Known(language) => language,
        Raw::Other(_) => Language::default(),
    })
}

/// The file, under Raven's config directory.
fn file_in(config: PathBuf) -> PathBuf {
    config.join("preferences").join("preferences.toml")
}

impl Preferences {
    /// Where the file lives, without creating anything.
    pub fn path() -> io::Result<PathBuf> {
        Ok(file_in(colony_ui::paths::locate::config_dir(
            crate::PROGRAM,
        )?))
    }

    /// The saved preferences, or the defaults when there are none or they
    /// cannot be read.
    pub fn load() -> Self {
        Self::path()
            .and_then(std::fs::read_to_string)
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    /// Lenient field by field; a file that is not TOML at all is the defaults.
    pub fn parse(text: &str) -> Self {
        toml::from_str(text).unwrap_or_default()
    }

    /// Writes the file, through a temporary beside it and a rename, so a
    /// crash mid-write leaves the previous preferences rather than half of
    /// these.
    pub fn save(&self) -> io::Result<()> {
        // `config_dir` creates Colony/Raven/ the way every Colony program
        // does; the preferences/ folder under it is Raven's own.
        let path = file_in(colony_ui::paths::config_dir(crate::PROGRAM)?);
        let dir = path.parent().expect("the file has a directory");
        std::fs::create_dir_all(dir)?;
        let text = toml::to_string(self).map_err(io::Error::other)?;
        let partial = dir.join(".preferences.toml.partial");
        std::fs::write(&partial, text)?;
        std::fs::rename(&partial, &path)
    }
}

#[cfg(test)]
mod tests {
    use colony_ui::typography::{FontSize, TextSize};

    use super::*;

    #[test]
    fn what_is_saved_is_what_is_read_back() {
        let mut prefs = Preferences::default();
        prefs.standard.selected_theme = "kanagawa".into();
        prefs.standard.selected_variant = "wave".into();
        prefs.standard.selected_accent = Some("violet".into());
        prefs.standard.font_size = FontSize::Large;
        prefs.standard.text_size_a11y = TextSize::XLarge;
        prefs.standard.dyslexia_font = true;
        prefs.language = Language::Fr;
        let text = toml::to_string(&prefs).unwrap();
        assert_eq!(Preferences::parse(&text), prefs, "{text}");
    }

    #[test]
    fn a_bad_value_costs_that_value_and_nothing_else() {
        let prefs = Preferences::parse(
            r#"
            selected_theme = "kanagawa"
            selected_variant = "wave"
            font_size = "enormous"
            high_contrast = "yes"
            language = "de"
            "#,
        );
        assert_eq!(prefs.standard.selected_theme, "kanagawa");
        assert_eq!(prefs.standard.font_size, FontSize::Default);
        assert!(!prefs.standard.high_contrast);
        assert_eq!(prefs.language, Language::System);

        let prefs = Preferences::parse("language = 3\nselected_theme = \"nord\"");
        assert_eq!(prefs.language, Language::System);
        assert_eq!(prefs.standard.selected_theme, "nord");
    }

    #[test]
    fn a_file_that_is_not_toml_is_the_defaults() {
        assert_eq!(Preferences::parse("{ not toml"), Preferences::default());
        assert_eq!(Preferences::parse(""), Preferences::default());
    }
}
