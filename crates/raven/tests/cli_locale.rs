//! The command line speaks the language the locale names, and only that one.
//!
//! Run as a user would run it, with nothing in the environment but the
//! locale, so the test covers how the language is chosen as well as the words.

use std::process::{Command, Output};

fn raven(locale: &[(&str, &str)], args: &[&str]) -> Output {
    let home = std::env::temp_dir().join(format!(
        "raven-locale-{}-{}",
        std::process::id(),
        args.join("_")
    ));
    std::fs::create_dir_all(&home).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_raven"))
        .args(args)
        .env_clear()
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .envs(locale.iter().copied())
        .output()
        .expect("raven starts");
    let _ = std::fs::remove_dir_all(&home);
    out
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn help_is_french_under_a_french_locale() {
    let help = stdout(&raven(&[("LANG", "fr_FR.UTF-8")], &["--help"]));
    assert!(
        help.contains("Exécuter des programmes Windows sous Linux"),
        "{help}"
    );
    assert!(help.contains("Utilisation :"), "{help}");
    assert!(help.contains("Commandes:"), "{help}");
    assert!(help.contains("Afficher l'aide"), "{help}");

    let start = stdout(&raven(
        &[("LC_MESSAGES", "fr_CA")],
        &["env", "start", "--help"],
    ));
    assert!(
        start.contains("Le montage coûte 0,04 s"),
        "the long help is French too: {start}"
    );

    let detach = stdout(&raven(&[("LANG", "fr")], &["env", "detach", "-h"]));
    assert!(detach.contains("[par défaut : d]"), "{detach}");
    assert!(!detach.contains("[default"), "{detach}");
}

#[test]
fn help_is_english_under_the_c_locale() {
    let help = stdout(&raven(&[("LC_ALL", "C")], &["--help"]));
    assert!(
        help.contains("Run Windows programs on Linux against a real Windows installation"),
        "{help}"
    );
    assert!(help.contains("Usage:"), "{help}");
    assert!(help.contains("Commands:"), "{help}");
}

#[test]
fn lc_all_outranks_lang_the_way_posix_orders_them() {
    let help = stdout(&raven(
        &[("LC_ALL", "C"), ("LANG", "fr_FR.UTF-8")],
        &["--help"],
    ));
    assert!(help.contains("Usage:"), "LC_ALL=C must win: {help}");
}

#[test]
fn a_library_error_reaches_the_terminal_in_french() {
    let out = raven(&[("LANG", "fr_FR.UTF-8")], &["env", "status", "nowhere"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(
        err.starts_with("Erreur : aucun environnement nommé \"nowhere\""),
        "{err}"
    );
}
