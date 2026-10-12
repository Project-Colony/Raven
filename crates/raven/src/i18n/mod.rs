//! Raven's words, in English and French.
//!
//! English is canonical: every key is named after its English string and is
//! written in `en.rs` first, and `fr.rs` carries the same keys written as
//! French. A key exists in both tables or in neither, which
//! `fr_and_en_have_identical_key_sets` enforces. Program names - Raven, Wine,
//! DXVK, vkd3d-proton - and the commands a message tells you to type are never
//! translated.
//!
//! The language is process-wide: [`set`] once at start-up, from [`Lang::from_env`]
//! for the command line or from a saved preference for the window, and every
//! [`t`] after it follows. The library's own messages, [`error_message`]
//! included, go through the same tables, so the window and the command line
//! never disagree about how a failure is worded.

mod en;
mod fr;

use std::fmt::Display;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::Error;
use crate::mount::MountError;

/// A language Raven speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    En,
    Fr,
}

impl Lang {
    /// French for a tag or locale name that starts with `fr` - `fr`, `fr_FR.UTF-8`,
    /// `fr-CA` - and English for anything else, `C` and `POSIX` included.
    pub fn from_tag(tag: &str) -> Lang {
        match tag.get(..2) {
            Some(p) if p.eq_ignore_ascii_case("fr") => Lang::Fr,
            _ => Lang::En,
        }
    }

    /// The system's language, read the way POSIX orders it: the first of
    /// `LC_ALL`, `LC_MESSAGES` and `LANG` that is set and not empty decides,
    /// so `LC_ALL=C` means English whatever `LANG` says.
    pub fn from_env() -> Lang {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .find_map(|var| std::env::var(var).ok().filter(|v| !v.is_empty()))
            .map_or(Lang::En, |tag| Lang::from_tag(&tag))
    }

    /// The tag colony-ui's `Locale::from_tag` understands.
    pub fn tag(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Fr => "fr",
        }
    }

    fn table(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Lang::En => en::STRINGS,
            Lang::Fr => fr::STRINGS,
        }
    }

    /// The plural form `n` takes, as the suffix of a counted key: `one` or
    /// `other`. French treats zero as singular - "0 fichier" - where English
    /// does not.
    pub fn plural(self, n: usize) -> &'static str {
        let one = match self {
            Lang::En => n == 1,
            Lang::Fr => n <= 1,
        };
        if one { "one" } else { "other" }
    }
}

static FRENCH: AtomicBool = AtomicBool::new(false);

/// Sets the language every later [`t`] speaks.
pub fn set(lang: Lang) {
    FRENCH.store(lang == Lang::Fr, Ordering::Relaxed);
}

/// The language [`set`] chose; English until something sets it.
pub fn lang() -> Lang {
    if FRENCH.load(Ordering::Relaxed) {
        Lang::Fr
    } else {
        Lang::En
    }
}

/// The string for `key` in `lang`, falling back to English. `None` only for a
/// key neither table has.
pub fn get_in(lang: Lang, key: &str) -> Option<&'static str> {
    let find = |l: Lang| l.table().iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
    find(lang).or_else(|| find(Lang::En))
}

/// The string for `key` in the current language. A key missing from both
/// tables renders as itself, which is visible without being fatal.
pub fn t(key: &'static str) -> &'static str {
    get_in(lang(), key).unwrap_or(key)
}

/// [`t`], with every `{name}` replaced by its value.
pub fn tf(key: &'static str, args: &[(&str, &dyn Display)]) -> String {
    tf_in(lang(), key, args)
}

/// [`tf`] for a count: `key_one` or `key_other` by the language's plural
/// rule, with the count available as `{n}`.
pub fn tn(key: &'static str, n: usize, args: &[(&str, &dyn Display)]) -> String {
    tn_in(lang(), key, n, args)
}

fn tf_in(lang: Lang, key: &str, args: &[(&str, &dyn Display)]) -> String {
    fill(get_in(lang, key).unwrap_or(key), args)
}

/// Replaces each `{name}` in one pass, so a value that happens to contain
/// `{name}` - a process called that, say - is printed rather than expanded.
/// A placeholder with no value is left as it is, visibly.
pub fn fill(template: &str, args: &[(&str, &dyn Display)]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let value = after.find('}').and_then(|close| {
            let name = &after[..close];
            args.iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| (v, close))
        });
        match value {
            Some((v, close)) => {
                let _ = write!(out, "{v}");
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn tn_in(lang: Lang, key: &str, n: usize, args: &[(&str, &dyn Display)]) -> String {
    let form = lang.plural(n);
    let mut all: Vec<(&str, &dyn Display)> = vec![("n", &n)];
    all.extend_from_slice(args);
    tf_in(lang, &format!("{key}_{form}"), &all)
}

/// `error` in the current language. `Display` stays English, for logs.
pub fn error_message(error: &Error) -> String {
    error_message_in(lang(), error)
}

/// `error` in `lang`. Every variant has a key in both tables, so a new variant
/// fails to compile here until it has words.
pub fn error_message_in(lang: Lang, error: &Error) -> String {
    let f = |key: &str, args: &[(&str, &dyn Display)]| tf_in(lang, key, args);
    let q = |s: &str| format!("{s:?}");
    match error {
        Error::NoHome => f("error_no_home", &[]),
        Error::NoRuntimeDir => f("error_no_runtime_dir", &[]),
        Error::BadName(name) => f("error_bad_name", &[("name", &q(name))]),
        Error::Layer(path, _) => f("error_layer", &[("path", &path.display())]),
        Error::NoSuchBase(id) => f("error_no_such_base", &[("id", &q(id))]),
        Error::BaseExists(id) => f("error_base_exists", &[("id", &q(id))]),
        Error::NotAWindowsBase(id) => f("error_not_a_windows_base", &[("id", &q(id))]),
        Error::NoSuchEnvironment(name) => f("error_no_such_environment", &[("name", &q(name))]),
        Error::EnvironmentExists(name) => f("error_environment_exists", &[("name", &q(name))]),
        Error::NoWine => f("error_no_wine", &[]),
        Error::Tool(tool, _) => f("error_tool", &[("tool", tool)]),
        Error::ToolFailed(tool, why) => f("error_tool_failed", &[("tool", tool), ("why", why)]),
        Error::Manifest(path, why) => {
            f("error_manifest", &[("path", &path.display()), ("why", why)])
        }
        Error::Hive(why) => f("error_hive", &[("why", why)]),
        Error::NoDefaultEnvironment => f("error_no_default_environment", &[]),
        Error::EnvironmentBusy { name, holders } => f(
            "error_environment_busy",
            &[("quoted", &q(name)), ("name", name), ("holders", holders)],
        ),
        Error::StillHeld(name, holders) => f(
            "error_still_held",
            &[("name", &q(name)), ("holders", holders)],
        ),
        Error::BadLetter(c) => f("error_bad_letter", &[("letter", &format!("{c:?}"))]),
        Error::NotABlockDevice(path) => f("error_not_a_block_device", &[("path", &path.display())]),
        Error::AlreadyAttached(c) => f("error_already_attached", &[("letter", c)]),
        Error::LetterTaken(c) => f("error_letter_taken", &[("letter", c)]),
        Error::DeviceAttached(path, c) => f(
            "error_device_attached",
            &[("path", &path.display()), ("letter", c)],
        ),
        Error::NotAttached(c) => f("error_not_attached", &[("letter", c)]),
        Error::NotAD3dBuild(runtime, path) => f(
            "error_not_a_d3d_build",
            &[("runtime", runtime), ("path", &path.display())],
        ),
        Error::SessionFailed(why) => f("error_session_failed", &[("why", why)]),
        Error::SessionHolds(name) => f(
            "error_session_holds",
            &[("quoted", &q(name)), ("name", name)],
        ),
        Error::D3dWouldOverwrite(path) => {
            f("error_d3d_would_overwrite", &[("path", &path.display())])
        }
        Error::D3dLink(path) => f("error_d3d_link", &[("path", &path.display())]),
    }
}

/// `error` in `lang`.
pub fn mount_error_message_in(lang: Lang, error: &MountError) -> String {
    let f = |key: &str, args: &[(&str, &dyn Display)]| tf_in(lang, key, args);
    match error {
        MountError::NoUserNamespace(why) => f("mount_error_no_user_namespace", &[("why", why)]),
        MountError::IdMap(why) => f("mount_error_id_map", &[("why", why)]),
        MountError::Mount(why) => f("mount_error_mount", &[("why", why)]),
        MountError::MissingPath(path) => {
            f("mount_error_missing_path", &[("path", &path.display())])
        }
        MountError::NoLowerLayer => f("mount_error_no_lower_layer", &[]),
    }
}

/// Any error in the current language, when it is one of Raven's own, and as
/// it describes itself otherwise - the kernel's errors are the system's to
/// translate.
pub fn describe(error: &(dyn std::error::Error + 'static)) -> String {
    if let Some(e) = error.downcast_ref::<Error>() {
        error_message(e)
    } else if let Some(e) = error.downcast_ref::<MountError>() {
        mount_error_message_in(lang(), e)
    } else {
        error.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use super::*;

    fn keys(lang: Lang) -> BTreeSet<&'static str> {
        lang.table().iter().map(|(k, _)| *k).collect()
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
    }

    #[test]
    fn no_key_is_declared_twice() {
        for lang in [Lang::En, Lang::Fr] {
            assert_eq!(
                keys(lang).len(),
                lang.table().len(),
                "a key appears twice in the {lang:?} table, and only the first would ever be read"
            );
        }
    }

    #[test]
    fn a_value_is_printed_not_expanded() {
        let name: &dyn Display = &"{holders}";
        let holders: &dyn Display = &"wine (1)";
        assert_eq!(
            fill(
                "{name} held by {holders}, {missing}",
                &[("name", name), ("holders", holders)]
            ),
            "{holders} held by wine (1), {missing}"
        );
    }

    #[test]
    fn a_locale_is_french_only_when_it_says_so() {
        for tag in ["fr", "fr_FR.UTF-8", "fr-CA", "FR_be"] {
            assert_eq!(Lang::from_tag(tag), Lang::Fr, "{tag}");
        }
        for tag in ["", "C", "POSIX", "en_US.UTF-8", "de_DE", "f"] {
            assert_eq!(Lang::from_tag(tag), Lang::En, "{tag}");
        }
    }

    #[test]
    fn french_counts_zero_as_singular_and_english_does_not() {
        assert_eq!(
            tn_in(Lang::En, "env_stopped", 0, &[("name", &"g")]),
            "Stopped g: 0 processes terminated."
        );
        assert_eq!(
            tn_in(Lang::En, "env_stopped", 1, &[("name", &"g")]),
            "Stopped g: 1 process terminated."
        );
        assert_eq!(
            tn_in(
                Lang::Fr,
                "d3d_removed",
                0,
                &[("name", &"g"), ("runtime", &"dxvk")]
            ),
            "0 bibliothèque dxvk retirée de g ; le vrai Windows est de nouveau visible."
        );
    }

    /// One of every variant. The match below has no wildcard, so a new
    /// variant fails to compile here until it is added to the list too.
    fn every_error() -> Vec<Error> {
        let io = || std::io::Error::other("x");
        let p = || PathBuf::from("/p");
        let all = vec![
            Error::NoHome,
            Error::NoRuntimeDir,
            Error::BadName("a/b".into()),
            Error::Layer(p(), io()),
            Error::NoSuchBase("w11".into()),
            Error::BaseExists("w11".into()),
            Error::NotAWindowsBase("w11".into()),
            Error::NoSuchEnvironment("games".into()),
            Error::EnvironmentExists("games".into()),
            Error::NoWine,
            Error::Tool("tar", io()),
            Error::ToolFailed("tar", "why".into()),
            Error::Manifest(p(), "why".into()),
            Error::Hive("why".into()),
            Error::NoDefaultEnvironment,
            Error::EnvironmentBusy {
                name: "games".into(),
                holders: "wine (1)".into(),
            },
            Error::StillHeld("games".into(), "wine (1)".into()),
            Error::BadLetter('c'),
            Error::NotABlockDevice(p()),
            Error::AlreadyAttached('d'),
            Error::LetterTaken('d'),
            Error::DeviceAttached(p(), 'd'),
            Error::NotAttached('d'),
            Error::NotAD3dBuild("dxvk", p()),
            Error::SessionFailed("why".into()),
            Error::SessionHolds("games".into()),
            Error::D3dWouldOverwrite(p()),
            Error::D3dLink(p()),
        ];
        for e in &all {
            match e {
                Error::NoHome
                | Error::NoRuntimeDir
                | Error::BadName(_)
                | Error::Layer(..)
                | Error::NoSuchBase(_)
                | Error::BaseExists(_)
                | Error::NotAWindowsBase(_)
                | Error::NoSuchEnvironment(_)
                | Error::EnvironmentExists(_)
                | Error::NoWine
                | Error::Tool(..)
                | Error::ToolFailed(..)
                | Error::Manifest(..)
                | Error::Hive(_)
                | Error::NoDefaultEnvironment
                | Error::EnvironmentBusy { .. }
                | Error::StillHeld(..)
                | Error::BadLetter(_)
                | Error::NotABlockDevice(_)
                | Error::AlreadyAttached(_)
                | Error::LetterTaken(_)
                | Error::DeviceAttached(..)
                | Error::NotAttached(_)
                | Error::NotAD3dBuild(..)
                | Error::SessionFailed(_)
                | Error::SessionHolds(_)
                | Error::D3dWouldOverwrite(_)
                | Error::D3dLink(_) => {}
            }
        }
        all
    }

    #[test]
    fn every_error_has_an_english_and_a_french_message() {
        for e in every_error() {
            let en = error_message_in(Lang::En, &e);
            let fr = error_message_in(Lang::Fr, &e);
            assert!(!en.starts_with("error_"), "{e:?} has no English words");
            assert!(!fr.starts_with("error_"), "{e:?} has no French words");
            assert_ne!(en, fr, "{e:?} is not translated");
            for m in [&en, &fr] {
                assert!(!m.contains('{'), "{e:?} left a placeholder: {m}");
            }
        }
    }

    #[test]
    fn every_mount_error_has_an_english_and_a_french_message() {
        let io = || std::io::Error::other("x");
        for e in [
            MountError::NoUserNamespace(io()),
            MountError::IdMap(io()),
            MountError::Mount(io()),
            MountError::MissingPath(PathBuf::from("/p")),
            MountError::NoLowerLayer,
        ] {
            let en = mount_error_message_in(Lang::En, &e);
            let fr = mount_error_message_in(Lang::Fr, &e);
            assert!(!en.starts_with("mount_error_") && !fr.starts_with("mount_error_"));
            assert_ne!(en, fr, "{e:?} is not translated");
        }
    }

    #[test]
    fn every_placeholder_in_french_exists_in_english() {
        // A French string naming `{nom}` where the code passes `{name}` would
        // print the braces to the user.
        let names = |s: &str| -> BTreeSet<String> {
            s.split('{')
                .skip(1)
                .filter_map(|rest| rest.split_once('}').map(|(n, _)| n.to_owned()))
                .collect()
        };
        for (key, fr) in Lang::Fr.table() {
            let en = get_in(Lang::En, key).unwrap_or_default();
            assert_eq!(names(fr), names(en), "placeholders differ for {key}");
        }
    }
}
