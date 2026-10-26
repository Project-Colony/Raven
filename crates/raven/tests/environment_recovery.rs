//! Recovering an environment whose namespace is still alive.
//!
//! Killing a program's window can leave `wineserver` and a dozen services
//! running inside the mount namespace, the upper layer stays busy, and the
//! next launch fails. `Environment::holders` must find those processes from
//! outside the namespace, and `Environment::stop` must actually release the
//! mount - properties only a real mount can demonstrate.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use raven::env::{Environment, Manifest};

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

#[test]
fn a_process_left_in_the_namespace_is_found_and_stopped() {
    if !userns_available() {
        eprintln!("skipped: this kernel restricts unprivileged user namespaces");
        return;
    }
    // The root deliberately contains a space, a comma, a colon and a
    // backslash: each survives a different escaping between the mount options
    // and what the kernel displays in mountinfo, and a holder must be found
    // through all of them - a miss here is `destroy` deleting layers under a
    // live mount.
    let root = std::env::temp_dir().join(format!("raven-recovery-{} a,b:c\\d", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for d in ["base", "upper", "work", "merged"] {
        fs::create_dir_all(root.join(d)).unwrap();
    }
    // Constructed by hand rather than through `create()`: holders and stop
    // depend only on the layout, and this test must not need Wine.
    let env = Environment {
        // Unique, because the session record goes to this name's runtime
        // directory and a real environment could be called anything.
        name: format!("recovery-test-{}", std::process::id()),
        manifest: Manifest {
            base: "none".into(),
        },
        root: root.clone(),
    };

    let mut child = Command::new(raven_bin())
        .args(["exec", "--lower"])
        .arg(root.join("base"))
        .arg("--upper")
        .arg(root.join("upper"))
        .arg("--work")
        .arg(root.join("work"))
        .arg("--target")
        .arg(root.join("merged"))
        .args(["--", "sleep", "30"])
        .spawn()
        .expect("spawn raven exec");

    // The mount appears when the child finishes unsharing; give it a moment.
    let mut holders = Vec::new();
    for _ in 0..50 {
        holders = env.holders();
        if !holders.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(
        holders.iter().any(|h| h.pid == child.id()),
        "the process inside the namespace must be visible from outside; saw {holders:?}"
    );

    // The launch path does not ask who holds the environment - it asks
    // whether the anchor it recorded still does, which is one process, not
    // every process on the machine. The cheap question must give the same
    // answer as the expensive one, or a warm launch either misses a live
    // session or joins a dead one.
    assert!(
        env.holds(child.id()),
        "the single-process check must agree with the scan that found it"
    );
    assert!(
        !env.holds(1),
        "a live process that holds nothing of ours is not a session"
    );
    // And the recorded pid is believed only when it is really a holder.
    let record = env.session_file().expect("XDG_RUNTIME_DIR is set");
    fs::create_dir_all(record.parent().unwrap()).unwrap();
    fs::write(&record, "1\n").unwrap();
    assert_eq!(
        env.session(),
        None,
        "a stale pid must not pass for a session"
    );
    fs::write(&record, format!("{}\n", child.id())).unwrap();
    assert_eq!(
        env.session(),
        Some(child.id()),
        "a real holder is the session"
    );
    let _ = fs::remove_file(&record);
    // A session started by a release that kept the record in the data
    // directory is still joined after an upgrade.
    fs::write(root.join("session.pid"), format!("{}\n", child.id())).unwrap();
    assert_eq!(
        env.session(),
        Some(child.id()),
        "the legacy record must still find a live session"
    );

    let stopped = env.stop().expect("stop must release the environment");
    assert!(
        stopped.iter().any(|h| h.pid == child.id()),
        "stop must report what it terminated"
    );
    assert!(
        env.holders().is_empty(),
        "nothing may hold the mount after stop"
    );
    // Reap it; stop already killed it.
    let _ = child.wait();
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(record.parent().unwrap());
}

#[test]
fn an_environment_running_nothing_reports_no_holders() {
    let root = std::env::temp_dir().join(format!("raven-idle-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("upper")).unwrap();
    let env = Environment {
        name: "idle-test".into(),
        manifest: Manifest {
            base: "none".into(),
        },
        root: root.clone(),
    };
    assert!(env.holders().is_empty());
    assert!(
        env.stop()
            .expect("stopping an idle environment is fine")
            .is_empty()
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_mount_waits_for_an_upper_layer_that_is_being_released() {
    if !userns_available() {
        eprintln!("skipped: this kernel restricts unprivileged user namespaces");
        return;
    }
    // `stop` returns once no process shows the mount any more, but the last
    // one is still taking it down, and overlayfs refuses the upper layer
    // until it has: `env start` right after `env stop` failed with EBUSY. A
    // holder that exits by itself a second later stands in for that window
    // and makes it long enough to hit every time.
    let root = std::env::temp_dir().join(format!("raven-release-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for d in ["base", "upper", "work", "merged"] {
        fs::create_dir_all(root.join(d)).unwrap();
    }
    let exec = |argv: &[&str]| {
        let mut cmd = Command::new(raven_bin());
        cmd.args(["exec", "--lower"])
            .arg(root.join("base"))
            .arg("--upper")
            .arg(root.join("upper"))
            .arg("--work")
            .arg(root.join("work"))
            .arg("--target")
            .arg(root.join("merged"))
            .arg("--")
            .args(argv);
        cmd
    };
    let env = Environment {
        name: format!("release-test-{}", std::process::id()),
        manifest: Manifest {
            base: "none".into(),
        },
        root: root.clone(),
    };

    let mut leaving = exec(&["sleep", "1"]).spawn().expect("spawn raven exec");
    for _ in 0..50 {
        if !env.holders().is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(!env.holders().is_empty(), "the first mount must be up");

    let second = exec(&["true"]).status().expect("spawn raven exec");
    assert!(
        second.success(),
        "a mount must wait for the upper layer to be let go, not fail with EBUSY"
    );
    let _ = leaving.wait();
    let _ = fs::remove_dir_all(&root);
}
