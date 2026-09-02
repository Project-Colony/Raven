//! The attach lifecycle, end to end, without Wine.
//!
//! Everything `attach` produces is inert configuration - symlinks and a
//! registry text edit - so the whole cycle is checkable against a fake
//! environment layout and a real block device node that is never opened.

use std::fs;
use std::path::{Path, PathBuf};

use raven::env::{Environment, Manifest};

/// Any block device will do: it is stat'ed, never opened. Skip (rather than
/// fail) on machines that expose none.
fn some_block_devices() -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir("/sys/block") else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| PathBuf::from("/dev").join(e.file_name()))
        .filter(|d| d.exists())
        .collect()
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
    let Some(dev) = some_block_devices().into_iter().next() else {
        eprintln!("skipped: no block device visible on this machine");
        return;
    };
    let env = fake_env("cycle");
    let dos = env.prefix().join("dosdevices");

    let a = env.attach(&dev, 'd').expect("attach must succeed");
    assert_eq!(a.letter, 'd');
    assert_eq!(a.device, dev);
    // Mountmgr pre-creates PhysicalDrive0, so the first disk is number 1 -
    // NOT letter minus 'a'.
    assert_eq!(a.number, 1);

    // The three symlinks point where Wine will look.
    assert_eq!(fs::read_link(dos.join("d::")).unwrap(), dev);
    assert_eq!(fs::read_link(dos.join("physicaldrive1")).unwrap(), dev);
    assert_eq!(
        fs::read_link(dos.join("d:")).unwrap(),
        env.root.join("attached/d")
    );
    assert!(env.root.join("attached/d").is_dir());

    // The registry gained exactly the "floppy" entry that makes Wine's
    // mountmgr publish the disk, atomically - no temp file left behind.
    let reg = fs::read_to_string(env.prefix().join("system.reg")).unwrap();
    assert!(reg.contains("[Software\\\\Wine\\\\Drives]"), "{reg}");
    assert!(reg.contains("\"d:\"=\"floppy\""), "{reg}");
    assert!(reg.contains("\"x\"=\"y\""), "the unrelated key survives");
    assert!(!env.prefix().join("system.reg.raven-tmp").exists());

    // Read-back agrees.
    assert_eq!(env.attachments(), vec![a]);

    // A second attach on the same letter is refused, not overwritten; the
    // same device on another letter is refused too.
    assert!(matches!(
        env.attach(&dev, 'd'),
        Err(raven::Error::AlreadyAttached('d'))
    ));
    assert!(matches!(
        env.attach(&dev, 'e'),
        Err(raven::Error::DeviceAttached(_, 'd'))
    ));

    env.detach('d').expect("detach must succeed");
    assert!(fs::symlink_metadata(dos.join("d::")).is_err());
    assert!(fs::symlink_metadata(dos.join("d:")).is_err());
    assert!(fs::symlink_metadata(dos.join("physicaldrive1")).is_err());
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
fn a_relative_device_path_is_wired_absolute() {
    let Some(dev) = some_block_devices().into_iter().next() else {
        eprintln!("skipped: no block device visible on this machine");
        return;
    };
    let env = fake_env("relative");

    // Build a path to the device that is relative to the test's cwd. If it
    // were wired verbatim, the symlink would resolve against dosdevices/
    // instead and dangle - the exact bug this test pins.
    let cwd = std::env::current_dir().unwrap();
    let ups: PathBuf = cwd.components().skip(1).map(|_| "..").collect();
    let rel = ups.join(dev.strip_prefix("/").unwrap());
    assert!(rel.is_relative());

    let a = env.attach(&rel, 'd').expect("attach must succeed");
    assert!(a.device.is_absolute(), "stored as {}", a.device.display());
    let wired = fs::read_link(env.prefix().join("dosdevices/d::")).unwrap();
    assert!(wired.is_absolute());
    assert_eq!(
        fs::canonicalize(&wired).unwrap(),
        fs::canonicalize(&dev).unwrap(),
        "the wired path must reach the device that was checked"
    );

    let _ = fs::remove_dir_all(&env.root);
}

#[test]
fn a_letter_the_user_already_mapped_is_refused_not_overwritten() {
    let Some(dev) = some_block_devices().into_iter().next() else {
        eprintln!("skipped: no block device visible on this machine");
        return;
    };

    // A winecfg-style folder mapping: a d: symlink, no d:: raw link.
    let env = fake_env("taken-link");
    let link = env.prefix().join("dosdevices/d:");
    std::os::unix::fs::symlink("/srv/shared", &link).unwrap();
    assert!(matches!(
        env.attach(&dev, 'd'),
        Err(raven::Error::LetterTaken('d'))
    ));
    assert_eq!(
        fs::read_link(&link).unwrap(),
        Path::new("/srv/shared"),
        "the user's mapping must survive"
    );
    let _ = fs::remove_dir_all(&env.root);

    // A registry-only mapping counts as taken too, and detach must not
    // delete it either - it is not an attachment.
    let env = fake_env("taken-reg");
    fs::write(
        env.prefix().join("system.reg"),
        "WINE REGISTRY Version 2\n\n[Software\\\\Wine\\\\Drives] 5\n\"d:\"=\"cdrom\"\n",
    )
    .unwrap();
    assert!(matches!(
        env.attach(&dev, 'd'),
        Err(raven::Error::LetterTaken('d'))
    ));
    let _ = fs::remove_dir_all(&env.root);
}

#[test]
fn detaching_renumbers_the_disks_that_remain() {
    let devs = some_block_devices();
    if devs.len() < 2 {
        eprintln!("skipped: needs two block devices, found {}", devs.len());
        return;
    }
    let env = fake_env("renumber");
    let dos = env.prefix().join("dosdevices");

    let a = env.attach(&devs[0], 'd').unwrap();
    let b = env.attach(&devs[1], 'e').unwrap();
    assert_eq!((a.number, b.number), (1, 2));
    assert_eq!(fs::read_link(dos.join("physicaldrive2")).unwrap(), devs[1]);

    // Dropping d: shifts e: down to number 1, exactly as mountmgr will.
    env.detach('d').unwrap();
    let left = env.attachments();
    assert_eq!(left.len(), 1);
    assert_eq!((left[0].letter, left[0].number), ('e', 1));
    assert_eq!(fs::read_link(dos.join("physicaldrive1")).unwrap(), devs[1]);
    assert!(fs::symlink_metadata(dos.join("physicaldrive2")).is_err());

    let _ = fs::remove_dir_all(&env.root);
}

#[test]
fn attach_refuses_what_would_eat_a_disk() {
    let env = fake_env("refuse");

    // A character device and a plain file are not block devices.
    assert!(matches!(
        env.attach(Path::new("/dev/null"), 'd'),
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
            env.attach(Path::new("/dev/null"), bad),
            Err(raven::Error::BadLetter(_))
        ));
    }

    let _ = fs::remove_dir_all(&env.root);
}
