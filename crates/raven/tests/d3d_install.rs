//! What an interrupted Direct3D install leaves behind.
//!
//! Installing a runtime copies libraries into the upper layer, then edits
//! the prefix's registry, then records what it did. Everything between the
//! first copy and that record can fail, and what the record says afterwards
//! decides whether the environment can be repaired: a library Raven put
//! there but does not list is one `dxvk` cannot see, `--remove` cannot
//! remove, and the next install refuses as somebody else's.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use raven::d3d::DXVK;
use raven::env::{Environment, Manifest};

fn fake_env(name: &str) -> Environment {
    let root = std::env::temp_dir().join(format!("raven-d3d-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("prefix/dosdevices")).unwrap();
    fs::create_dir_all(root.join("upper/Windows/System32")).unwrap();
    fs::create_dir_all(root.join("upper/Windows/SysWOW64")).unwrap();
    fs::write(
        root.join("prefix/user.reg"),
        "WINE REGISTRY Version 2\n\n[Software\\\\Fluff] 100\n\"x\"=\"y\"\n",
    )
    .unwrap();
    Environment {
        name: name.into(),
        manifest: Manifest {
            base: "none".into(),
        },
        root,
    }
}

/// A directory shaped like a DXVK release, with the libraries it names.
fn fake_build(root: &std::path::Path, version: &str) -> PathBuf {
    let build = root.join(version);
    for arch in ["x64", "x32"] {
        let dir = build.join(arch);
        fs::create_dir_all(&dir).unwrap();
        for dll in ["d3d11", "dxgi"] {
            fs::write(dir.join(format!("{dll}.dll")), version.as_bytes()).unwrap();
        }
    }
    build
}

#[test]
fn a_failure_after_the_copies_leaves_libraries_raven_can_still_account_for() {
    let env = fake_env("interrupted");
    let build = fake_build(&env.root, "dxvk-2.7");

    // The registry is what fails: read it, then refuse the rewrite. The
    // prefix is made unwritable, which is exactly what a full disk or a
    // read-only home does to the same step.
    let prefix = env.prefix();
    let before = fs::metadata(&prefix).unwrap().permissions();
    fs::set_permissions(&prefix, fs::Permissions::from_mode(0o500)).unwrap();
    let outcome = env.install_d3d(&DXVK, &build);
    fs::set_permissions(&prefix, before).unwrap();

    assert!(outcome.is_err(), "the registry could not be written");

    // The libraries are on disk. What matters is that Raven says so, rather
    // than reporting an empty install and then refusing to touch its own
    // files as though a stranger had put them there.
    let listed = env.d3d(&DXVK);
    assert!(
        !listed.is_empty(),
        "the copies happened, so they must be accounted for"
    );
    for shadow in &listed {
        assert!(
            shadow.path.is_file(),
            "{} is listed and must be on disk",
            shadow.path.display()
        );
    }
    let removed = env.remove_d3d(&DXVK).expect("removal must be possible");
    assert_eq!(removed, listed.len(), "everything listed comes back out");
    assert!(
        env.d3d(&DXVK).is_empty(),
        "and nothing of Raven's is left behind"
    );

    let _ = fs::remove_dir_all(&env.root);
}

#[test]
fn a_failed_upgrade_does_not_take_the_working_build_with_it() {
    let env = fake_env("upgrade");
    let old = fake_build(&env.root, "dxvk-2.4");
    env.install_d3d(&DXVK, &old)
        .expect("the first install works");
    let installed: Vec<PathBuf> = env.d3d(&DXVK).into_iter().map(|s| s.path).collect();
    assert_eq!(installed.len(), 4, "two libraries, two architectures");

    // An upgrade whose second architecture cannot be read: x32 is there in
    // the plan and unreadable by the time it is copied.
    let new = fake_build(&env.root, "dxvk-2.7");
    fs::set_permissions(new.join("x32/dxgi.dll"), fs::Permissions::from_mode(0o000)).unwrap();
    let outcome = env.install_d3d(&DXVK, &new);
    fs::set_permissions(new.join("x32/dxgi.dll"), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(outcome.is_err(), "the upgrade could not be completed");

    // Whatever the mixture of versions now on disk, every library the
    // environment had is still there and still described. The rollback must
    // not delete files that were a working install before this call.
    for path in &installed {
        assert!(
            path.is_file(),
            "{} was working before the upgrade and must survive it",
            path.display()
        );
    }
    assert_eq!(
        env.d3d(&DXVK).len(),
        installed.len(),
        "and all of them are still accounted for"
    );

    let _ = fs::remove_dir_all(&env.root);
}
