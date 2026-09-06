//! The window can be its own session anchor.
//!
//! `Environment::ensure_session` starts the anchor as `current_exe()
//! session-anchor <name>`, so whichever binary asked for a session has to
//! answer to that argument - the window included, since Start asks.
//!
//! The environment is cleared so that `DISPLAY`, `WAYLAND_DISPLAY` and
//! `WAYLAND_SOCKET` are all unset: whatever this binary does with the
//! argument, it cannot open a window on the machine running the tests.

use std::process::{Command, Stdio};

#[test]
fn session_anchor_is_answered_before_any_window_is_attempted() {
    let home = std::env::temp_dir().join(format!("raven-gui-anchor-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_raven-gui"))
        .args(["session-anchor", "nope"])
        .env_clear()
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .output()
        .expect("raven-gui starts");
    let _ = std::fs::remove_dir_all(&home);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    // The anchor protocol: every failure leaves through stdout with status
    // 1, and an environment that does not exist is the first it can hit.
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout: {stdout:?}\nstderr: {stderr:?}"
    );
    assert!(
        stdout.contains("no environment called \"nope\""),
        "the anchor protocol was not answered on stdout.\nstdout: {stdout:?}\nstderr: {stderr:?}"
    );
}
