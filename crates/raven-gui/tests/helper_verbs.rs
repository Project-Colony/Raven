//! The window must never open a window when it was not asked for one.
//!
//! The library re-runs its own binary for two things - `session-anchor` and
//! the `exec` behind a registry import - so whichever binary asked has to
//! answer, or answer plainly that it cannot. Falling through to iced would
//! put a second, unexplained Raven window on the user's screen and hand the
//! caller a success it did not earn.
//!
//! Every case runs with the environment cleared, so no display exists for a
//! window to open on even if one were attempted.

use std::process::{Command, Stdio};

fn run(args: &[&str]) -> (Option<i32>, String, String) {
    let home = std::env::temp_dir().join(format!(
        "raven-gui-verbs-{}-{}",
        std::process::id(),
        args.join("_").replace('/', "_")
    ));
    std::fs::create_dir_all(&home).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_raven-gui"))
        .args(args)
        .env_clear()
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .output()
        .expect("raven-gui starts");
    let _ = std::fs::remove_dir_all(&home);
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn the_exec_verb_is_refused_in_words_rather_than_with_a_window() {
    // `registry::import` re-runs `current_exe() exec --lower ... -- <argv>`.
    // The window cannot mount and exec - it would have to replace itself -
    // so it must say so on stdout, which is the only stream the caller reads.
    let (code, stdout, stderr) = run(&[
        "exec",
        "--lower",
        "/nonexistent/lower",
        "--upper",
        "/nonexistent/upper",
        "--work",
        "/nonexistent/work",
        "--target",
        "/nonexistent/target",
        "--",
        "/bin/true",
    ]);
    assert_eq!(
        code,
        Some(1),
        "must fail, and visibly.\nstdout: {stdout:?}\nstderr: {stderr:?}"
    );
    assert!(
        !stderr.contains("Create event loop"),
        "a window was attempted for a verb that is not about windows: {stderr:?}"
    );
    assert!(
        stdout.contains("raven"),
        "the caller reads stdout and must be told which binary to run: {stdout:?}"
    );
}

#[test]
fn an_argument_the_window_does_not_know_never_reaches_iced() {
    let (code, stdout, stderr) = run(&["--frobnicate"]);
    assert_eq!(code, Some(1), "stdout: {stdout:?}\nstderr: {stderr:?}");
    assert!(
        !stderr.contains("Create event loop"),
        "an unknown argument must not open a window: {stderr:?}"
    );
}
