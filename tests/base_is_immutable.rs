//! The test that guards the whole design.
//!
//! Raven's central claim is that a program writing to C: cannot damage the
//! Windows base it runs against. That is not a property to verify by reading the
//! code — it is the one thing that must be checked by running it.
//!
//! This drives the real `raven` binary rather than calling the library directly,
//! because mounting enters namespaces the calling process can never leave, and
//! because the thing worth testing is what actually ships.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// The binary under test, next to the integration test that cargo just built.
fn raven_bin() -> PathBuf {
    let mut p = std::env::current_exe().expect("test binary has a path");
    p.pop();
    if p.ends_with("deps") {
        p.pop();
    }
    p.join("raven")
}

/// The library's own check, not a copy of it: the CI runners taught us that a
/// single-knob version passes and then fails the mount (AppArmor).
fn userns_available() -> bool {
    use raven::mount::MountBackend as _;
    raven::mount::UserNsOverlay::is_available()
}

struct Layout {
    root: PathBuf,
}

impl Layout {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("raven-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for d in ["base", "upper", "work", "merged"] {
            fs::create_dir_all(root.join(d)).expect("create layer directory");
        }
        Self { root }
    }
    fn p(&self, s: &str) -> PathBuf {
        self.root.join(s)
    }
}

impl Drop for Layout {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn run_in_overlay(l: &Layout, shell: &str) -> std::process::Output {
    run_layered(l, &[&l.p("base")], shell)
}

fn run_layered(l: &Layout, lower: &[&PathBuf], shell: &str) -> std::process::Output {
    let mut cmd = Command::new(raven_bin());
    cmd.arg("exec");
    for p in lower {
        cmd.args(["--lower", p.to_str().unwrap()]);
    }
    cmd.args(["--upper", l.p("upper").to_str().unwrap()])
        .args(["--work", l.p("work").to_str().unwrap()])
        .args(["--target", l.p("merged").to_str().unwrap()])
        .arg("--")
        .args(["/bin/sh", "-c", shell])
        .output()
        .expect("run raven exec")
}

/// The property the whole shadow-set design rests on: with two read-only layers,
/// the first one wins wherever both have a file, and the second shows through
/// wherever the first does not.
#[test]
fn the_first_layer_wins_and_the_second_fills_in() {
    if !userns_available() {
        eprintln!("skipped: this kernel restricts unprivileged user namespaces");
        return;
    }
    let l = Layout::new("layers");
    let high = l.p("high");
    fs::create_dir_all(&high).unwrap();

    // Present in both, with different contents - this is the shadow.
    fs::write(l.p("base").join("shared.txt"), "from windows").unwrap();
    fs::write(high.join("shared.txt"), "from wine").unwrap();
    // Present only in the low layer - Microsoft's own files showing through.
    fs::write(l.p("base").join("only-low.txt"), "microsoft only").unwrap();
    // Present only in the high layer.
    fs::write(high.join("only-high.txt"), "wine only").unwrap();

    let m = l.p("merged");
    let out = run_layered(
        &l,
        &[&high, &l.p("base")],
        &format!(
            "cat {m}/shared.txt; echo; cat {m}/only-low.txt; echo; cat {m}/only-high.txt",
            m = m.display()
        ),
    );
    assert!(
        out.status.success(),
        "raven exec failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let seen = String::from_utf8_lossy(&out.stdout);
    assert!(
        seen.contains("from wine") && !seen.contains("from windows"),
        "the first layer must win where both have the file, got: {seen}"
    );
    assert!(
        seen.contains("microsoft only"),
        "the second layer must show through where the first has nothing, got: {seen}"
    );
    assert!(seen.contains("wine only"), "got: {seen}");
}

#[test]
fn a_write_through_the_overlay_leaves_the_base_untouched() {
    if !userns_available() {
        eprintln!("skipped: this kernel restricts unprivileged user namespaces");
        return;
    }
    let l = Layout::new("immutable");
    fs::write(l.p("base").join("windows.txt"), "from the base").unwrap();

    let out = run_in_overlay(
        &l,
        &format!(
            "echo overwritten > {m}/windows.txt && echo new > {m}/added.txt",
            m = l.p("merged").display()
        ),
    );
    assert!(
        out.status.success(),
        "raven exec failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // The claim, checked three ways.
    assert_eq!(
        fs::read_to_string(l.p("base").join("windows.txt")).unwrap(),
        "from the base",
        "the base file was modified - the immutability guarantee is broken"
    );
    assert!(
        !l.p("base").join("added.txt").exists(),
        "a new file reached the base - writes are not being redirected"
    );
    assert!(
        l.p("upper").join("added.txt").exists(),
        "the write did not land in the overlay upper layer either"
    );
}

#[test]
fn the_mount_does_not_survive_the_process() {
    if !userns_available() {
        eprintln!("skipped: this kernel restricts unprivileged user namespaces");
        return;
    }
    let l = Layout::new("ephemeral");
    fs::write(l.p("base").join("marker"), "x").unwrap();

    let out = run_in_overlay(&l, &format!("ls {}", l.p("merged").display()));
    assert!(out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("marker"),
        "the overlay was not visible to the launched program"
    );

    // A leaked mount would leave the base's contents visible here.
    let leaked: Vec<_> = fs::read_dir(l.p("merged")).unwrap().collect();
    assert!(
        leaked.is_empty(),
        "the mount outlived the process that owned it"
    );
}

#[test]
fn a_missing_layer_is_reported_by_name() {
    if !userns_available() {
        // On a restricted kernel the earlier "user namespaces unavailable"
        // refusal wins, so the path validation this asserts never runs.
        eprintln!("skipped: this kernel restricts unprivileged user namespaces");
        return;
    }
    let l = Layout::new("missing");
    fs::remove_dir(l.p("work")).unwrap();
    let out = run_in_overlay(&l, "true");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(
        stderr.contains("work"),
        "the error should name the missing directory, got: {stderr}"
    );
}
