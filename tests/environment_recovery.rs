//! Recovering an environment whose namespace is still alive.
//!
//! Killing a program's window can leave `wineserver` and a dozen services
//! running inside the mount namespace, the upper layer stays busy, and the
//! next launch fails. `Environment::holders` must find those processes from
//! outside the namespace, and `Environment::stop` must actually release the
//! mount — properties only a real mount can demonstrate.

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

fn userns_available() -> bool {
    fs::read_to_string("/proc/sys/user/max_user_namespaces")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .is_some_and(|n| n > 0)
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
        name: "recovery-test".into(),
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
