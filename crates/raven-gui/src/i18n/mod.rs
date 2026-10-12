//! The window's own words, in English and French.
//!
//! Three tables answer a key, in this order: this one, for what only the
//! window says; `raven::i18n`, for what the library and the command line say
//! too - errors and diagnostics, worded once for both; and colony-ui's shared
//! labels, for the Preferences skeleton, themes and accents that every Colony
//! program shows alike. The language is the library's: [`set_language`] sets
//! it for all three.

mod en;
mod fr;

use std::fmt::Display;

use raven::i18n::{Lang, fill, lang};

/// Speaks `lang` from the next frame on, in every table.
pub fn set_language(lang: Lang) {
    raven::i18n::set(lang);
    colony_ui::i18n::set_locale(colony_ui::i18n::Locale::from_tag(lang.tag()));
}

fn table(lang: Lang) -> &'static [(&'static str, &'static str)] {
    match lang {
        Lang::En => en::STRINGS,
        Lang::Fr => fr::STRINGS,
    }
}

/// This table's string for `key` in `lang`, falling back to English.
fn get_in(lang: Lang, key: &str) -> Option<&'static str> {
    let find = |l: Lang| table(l).iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
    find(lang).or_else(|| find(Lang::En))
}

fn lookup(key: &str) -> Option<&'static str> {
    get_in(lang(), key).or_else(|| raven::i18n::get_in(lang(), key))
}

/// The string for `key` in the current language, from whichever table has
/// it; colony-ui's renders an unknown key as itself.
pub fn t(key: &'static str) -> &'static str {
    lookup(key).unwrap_or_else(|| colony_ui::i18n::t(key))
}

/// [`t`], with every `{name}` replaced by its value.
pub fn tf(key: &'static str, args: &[(&str, &dyn Display)]) -> String {
    fill(t(key), args)
}

/// [`tf`] for a count: `key_one` or `key_other`, with the count as `{n}`.
pub fn tn(key: &'static str, n: usize, args: &[(&str, &dyn Display)]) -> String {
    let counted = format!("{key}_{}", lang().plural(n));
    let mut all: Vec<(&str, &dyn Display)> = vec![("n", &n)];
    all.extend_from_slice(args);
    fill(lookup(&counted).unwrap_or(&counted), &all)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn keys(lang: Lang) -> BTreeSet<&'static str> {
        table(lang).iter().map(|(k, _)| *k).collect()
    }

    #[test]
    fn fr_and_en_have_identical_key_sets() {
        let (en, fr) = (keys(Lang::En), keys(Lang::Fr));
        let only_en: Vec<_> = en.difference(&fr).collect();
        let only_fr: Vec<_> = fr.difference(&en).collect();
        assert!(
            only_en.is_empty() && only_fr.is_empty(),
            "only in en.rs: {only_en:?}\nonly in fr.rs: {only_fr:?}"
        );
        assert_eq!(en.len(), table(Lang::En).len(), "a key is declared twice");
    }

    #[test]
    fn no_key_shadows_one_the_library_or_colony_ui_already_owns() {
        // A second copy of a shared string is how two wordings for one thing
        // start to drift; this table is for what only the window says.
        for (key, _) in table(Lang::En) {
            assert!(
                raven::i18n::get_in(Lang::En, key).is_none(),
                "{key} is the library's"
            );
            assert!(
                !colony_ui::i18n::all(colony_ui::i18n::Locale::En).any(|(k, _)| k == *key),
                "{key} is colony-ui's"
            );
        }
    }

    #[test]
    fn every_placeholder_in_french_exists_in_english() {
        let names = |s: &str| -> BTreeSet<String> {
            s.split('{')
                .skip(1)
                .filter_map(|rest| rest.split_once('}').map(|(n, _)| n.to_owned()))
                .collect()
        };
        for (key, fr) in table(Lang::Fr) {
            let en = get_in(Lang::En, key).unwrap_or_default();
            assert_eq!(names(fr), names(en), "placeholders differ for {key}");
        }
    }

    /// I18N-2: a word in a widget that skips this module is a word nobody
    /// translates. Looks for a string literal handed straight to `text`,
    /// `button(text(..))` or a `text_input` placeholder.
    #[test]
    fn no_view_draws_a_bare_literal() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/view");
        let mut found = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let source = std::fs::read_to_string(&path).unwrap();
            for (n, line) in source.lines().enumerate() {
                for needle in ["text(\"", "text_input(\""] {
                    let mut rest = line;
                    while let Some(at) = rest.find(needle) {
                        rest = &rest[at + needle.len()..];
                        if rest.starts_with(|c: char| c.is_alphabetic()) {
                            found.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                        }
                    }
                }
            }
        }
        assert!(found.is_empty(), "bare literals:\n{}", found.join("\n"));
    }
}
