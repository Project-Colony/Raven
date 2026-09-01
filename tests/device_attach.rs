//! The attach lifecycle, end to end, without Wine.
//!
//! Everything `attach` produces is inert configuration - symlinks and a
//! registry text edit - so the whole cycle is checkable against a fake
//! environment layout and a real block device node that is never opened.

use std::fs;
use std::path::PathBuf;

use raven::env::{Environment, Manifest};

/// Any block device will do: it is stat'ed, never opened. Skip (rather than
/// fail) on machines that expose none.
fn some_block_device() -> Option<PathBuf> {
    let entries = fs::read_dir("/sys/block").ok()?;
    for e in entries.flatten() {
        let dev = PathBuf::from("/dev").join(e.file_name());
        if dev.exists() {
            return Some(dev);
        }
    }
    None
}

fn fake_env(name: &str) -> Environment {
    let root = std::env::temp_dir().join(format!("raven-attach-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("prefix/dosdevices")).unwrap();
    fs::create_dir_all(root.join("upper")).unwrap();
    fs::write(
        root.join("prefix/system.reg"),
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

#[test]
fn the_attach_cycle_creates_and_removes_exactly_its_own_wiring() {
    let Some(dev) = some_block_device() else {
        eprintln!("skipped: no block device visible on this machine");
        return;
    };
    let env = fake_env("cycle");
    let dos = env.prefix().join("dosdevices");

    let a = env.attach(&dev, 'd').expect("attach must succeed");
    assert_eq!(a.letter, 'd');
    assert_eq!(a.device, dev);
    assert_eq!(a.number, 3);

    // The three symlinks point where Wine will look.
    assert_eq!(fs::read_link(dos.join("d::")).unwrap(), dev);
    assert_eq!(fs::read_link(dos.join("physicaldrive3")).unwrap(), dev);
    assert_eq!(
        fs::read_link(dos.join("d:")).unwrap(),
        env.root.join("attached/d")
    );
    assert!(env.root.join("attached/d").is_dir());

    // The registry gained exactly the "floppy" entry that makes Wine's
    // mountmgr publish \\.\PhysicalDrive3.
    let reg = fs::read_to_string(env.prefix().join("system.reg")).unwrap();
    assert!(reg.contains("[Software\\\\Wine\\\\Drives]"), "{reg}");
    assert!(reg.contains("\"d:\"=\"floppy\""), "{reg}");
    assert!(reg.contains("\"x\"=\"y\""), "the unrelated key survives");

    // Read-back agrees.
    assert_eq!(env.attachments(), vec![a]);

    // A second attach on the same letter is refused, not overwritten.
    assert!(matches!(
        env.attach(&dev, 'd'),
        Err(raven::Error::AlreadyAttached('d'))
    ));

    env.detach('d').expect("detach must succeed");
    assert!(fs::symlink_metadata(dos.join("d::")).is_err());
    assert!(fs::symlink_metadata(dos.join("d:")).is_err());
    assert!(fs::symlink_metadata(dos.join("physicaldrive3")).is_err());
    assert!(!env.root.join("attached").exists());
    let reg = fs::read_to_string(env.prefix().join("system.reg")).unwrap();
    assert!(!reg.contains("\"d:\"="), "{reg}");
    assert!(env.attachments().is_empty());

    // Detaching again is an error, not a shrug.
    assert!(matches!(
        env.detach('d'),
        Err(raven::Error::NotAttached('d'))
    ));

    let _ = fs::remove_dir_all(&env.root);
}

#[test]
fn attach_refuses_what_would_eat_a_disk() {
    let env = fake_env("refuse");

    // A character device and a plain file are not block devices.
    assert!(matches!(
        env.attach(std::path::Path::new("/dev/null"), 'd'),
        Err(raven::Error::NotABlockDevice(_))
    ));
    let f = env.root.join("not-a-device");
    fs::write(&f, "x").unwrap();
    assert!(matches!(
        env.attach(&f, 'd'),
        Err(raven::Error::NotABlockDevice(_))
    ));

    // Reserved letters are refused before the device is even looked at.
    for bad in ['a', 'b', 'c'] {
        assert!(matches!(
            env.attach(std::path::Path::new("/dev/null"), bad),
            Err(raven::Error::BadLetter(_))
        ));
    }

    let _ = fs::remove_dir_all(&env.root);
}
