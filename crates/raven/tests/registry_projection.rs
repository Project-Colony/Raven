//! The registry projection, tested against hives this repository builds itself.
//!
//! Raven cannot ship a Microsoft hive — that would be distributing Microsoft's
//! software — so the fixtures are synthetic, built at test time from a small
//! `.reg` description by an **independent** implementation. That independence is
//! the point: if Raven both wrote and read the fixtures, a shared
//! misunderstanding of the format would pass every test and fail on a real
//! Windows.

use raven::registry::{Rules, hive};
use regf::reg_import::RegImporter;

/// Builds a hive from a `.reg` description, the way a real one would be laid out.
fn hive_from(reg: &str) -> Vec<u8> {
    RegImporter::from_string(reg)
        .build_hive()
        .expect("regf should build a hive from valid .reg text")
}

const SOFTWARE: &str = r#"Windows Registry Editor Version 5.00

[HKEY_LOCAL_MACHINE\Software\Valve\Steam]
"InstallPath"="C:\\Program Files (x86)\\Steam"
"Language"="english"

[HKEY_LOCAL_MACHINE\Software\Microsoft\DirectX]
"Version"="4.09.00.0904"

[HKEY_LOCAL_MACHINE\Software\Microsoft\Windows NT\CurrentVersion]
"ProductName"="Windows 11 Pro"
"SystemRoot"="X:\\Windows"

[HKEY_LOCAL_MACHINE\Software\Microsoft\Cryptography]
"MachineGuid"="deadbeef-0000-0000-0000-000000000000"

[HKEY_LOCAL_MACHINE\Software\WOW6432Node\Microsoft\Windows NT\CurrentVersion]
"ProductName"="Windows 11 Pro"

[HKEY_LOCAL_MACHINE\Software\WOW6432Node\Valve\Steam]
"InstallPath"="C:\\Program Files (x86)\\Steam"

[HKEY_LOCAL_MACHINE\Software\Clients\Mail\Example]
"ReinstallCommand"="\"X:\\Windows\\System32\\setup.exe\" -q"
"#;

fn project() -> Vec<raven::registry::Key> {
    let bytes = hive_from(SOFTWARE);
    hive::project(&bytes, r"HKLM\Software", &Rules::default())
        .expect("the projection should read a hive regf just built")
}

fn paths() -> Vec<String> {
    project().into_iter().map(|k| k.path).collect()
}

fn value(path: &str, name: &str) -> Option<raven::registry::Data> {
    project()
        .into_iter()
        .find(|k| k.path.eq_ignore_ascii_case(path))?
        .values
        .into_iter()
        .find(|v| v.name.eq_ignore_ascii_case(name))
        .map(|v| v.data)
}

#[test]
fn third_party_software_crosses() {
    assert!(
        paths().iter().any(|p| p.contains(r"Valve\Steam")),
        "got {:?}",
        paths()
    );
}

#[test]
fn a_named_microsoft_subtree_crosses() {
    assert!(paths().iter().any(|p| p.ends_with(r"Microsoft\DirectX")));
}

#[test]
fn the_os_version_key_does_not_cross() {
    assert!(
        !paths().iter().any(|p| p.contains("Windows NT")),
        "Wine's own account of the Windows version must not be overwritten"
    );
}

#[test]
fn machine_identity_does_not_cross() {
    assert!(!paths().iter().any(|p| p.contains("Cryptography")));
}

#[test]
fn the_32_bit_mirror_obeys_the_same_rules() {
    let p = paths();
    assert!(
        !p.iter()
            .any(|x| x.contains("WOW6432Node") && x.contains("Windows NT")),
        "the 32-bit view of a refused subtree must be refused too: {p:?}"
    );
    assert!(
        p.iter()
            .any(|x| x.contains("WOW6432Node") && x.contains("Valve")),
        "but a 32-bit vendor key must still cross, keeping its real path: {p:?}"
    );
}

#[test]
fn the_setup_drive_letter_is_rewritten_on_the_way_through() {
    let v = value(r"HKLM\Software\Clients\Mail\Example", "ReinstallCommand")
        .expect("the key should have crossed");
    match v {
        raven::registry::Data::Sz(s) => {
            assert!(!s.contains("X:\\"), "X: survived the projection: {s}");
            assert!(s.contains("C:\\Windows\\System32"), "{s}");
        }
        other => panic!("expected a string, got {other:?}"),
    }
}

#[test]
fn projecting_twice_gives_the_same_answer() {
    // Idempotence is what makes it safe to re-run after editing the rules, and
    // it is why the output is never corrected by hand.
    assert_eq!(paths(), paths());
}

#[test]
fn a_hive_that_is_not_a_hive_is_an_error_not_a_panic() {
    let err = hive::project(b"not a hive at all", r"HKLM\Software", &Rules::default());
    assert!(err.is_err());
}
