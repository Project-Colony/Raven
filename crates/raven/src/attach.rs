//! Attaching a real block device to an environment.
//!
//! This is the configuration tier of device passthrough — see
//! `docs/internals/device-passthrough.md` for why the other two tiers are a
//! Wine patch and a refusal. What this module wires up is exactly what Wine's
//! own machinery reads:
//!
//! - `dosdevices/<l>::` → the unix device: `\\.\L:` opens the real block
//!   device, genuine sector reads and writes.
//! - `dosdevices/<l>:` → a directory, because Wine expects every drive to
//!   have a mount point.
//! - `HKLM\Software\Wine\Drives "<l>:"="floppy"` — the counter-intuitive
//!   value that matters: Wine's mountmgr promotes a "floppy" on a letter ≥ 2
//!   to a real `\Device\Harddisk` with a `\\.\PhysicalDriveN` alias; the
//!   obvious `"hd"` yields a volume with no such alias at all.
//! - `dosdevices/physicaldrive<n>` → the unix device: a plain DOS device
//!   name resolves straight to the target, so `\\.\PhysicalDriveN` reaches
//!   the real device instead of mountmgr's fake one, which cannot read or
//!   write.
//!
//! The number `n` is not ours to invent: mountmgr allocates disk devices
//! first-free-from-0 in creation order, and pre-creates a stub
//! `Harddisk0/PhysicalDrive0` at startup, so the first registry-configured
//! disk is PhysicalDrive**1**. Raven mirrors that allocation — the rank of
//! the letter among the disk-producing entries of the Drives section — so
//! the number a program derives from Wine and the name Raven wired agree.
//! Detaching renumbers what remains, because mountmgr will too.
//!
//! What this deliberately does not do: make the device *enumerable*. Tools
//! that discover disks through SetupDi (Rufus among them) stay blind — no
//! configuration can register the device interface they query. And Raven
//! never touches the device node's permissions: access is the user's to
//! grant, and `attach` prints the command rather than running it.

use std::path::{Path, PathBuf};

use crate::{Error, env::Environment, registry::text};

const SECTION: &str = "Software\\\\Wine\\\\Drives";

/// A device wired into an environment.
#[derive(Debug, PartialEq, Eq)]
pub struct Attachment {
    pub letter: char,
    pub device: PathBuf,
    pub number: u32,
}

impl Environment {
    /// The devices currently attached, read back from the prefix.
    pub fn attachments(&self) -> Vec<Attachment> {
        let dos = self.prefix().join("dosdevices");
        let Ok(text) = std::fs::read_to_string(self.prefix().join("system.reg")) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for (rank, &letter) in disk_letters(&text).iter().enumerate() {
            // An entry without a raw link is not Raven's attachment, but it
            // still consumes a mountmgr number, so the rank counts it.
            let Ok(device) = std::fs::read_link(dos.join(format!("{letter}::"))) else {
                continue;
            };
            found.push(Attachment {
                letter,
                device,
                number: rank as u32 + 1,
            });
        }
        found
    }

    /// Wires a block device into the environment under a drive letter.
    ///
    /// Refuses while the environment runs: `wineserver` holds the registry
    /// in memory and would overwrite the edit on exit. Refuses a letter that
    /// already has any mapping — a drive the user set up by hand is theirs,
    /// not Raven's to overwrite.
    pub fn attach(&self, device: &Path, letter: char) -> Result<Attachment, Error> {
        self.ensure_not_running()?;
        check_letter(letter)?;
        // The symlink target is resolved by the kernel against dosdevices/,
        // not against our cwd — absolutize so the path that was checked is
        // the path that gets wired. Symlinks are kept unresolved on purpose:
        // /dev/disk/by-id names survive a reboot, /dev/sdX names do not.
        let device =
            std::path::absolute(device).map_err(|e| Error::Layer(device.to_path_buf(), e))?;
        check_block_device(&device)?;

        let dos = self.prefix().join("dosdevices");
        let raw = dos.join(format!("{letter}::"));
        if std::fs::symlink_metadata(&raw).is_ok() {
            return Err(Error::AlreadyAttached(letter));
        }
        let reg = self.prefix().join("system.reg");
        let text = std::fs::read_to_string(&reg).map_err(|e| Error::Layer(reg.clone(), e))?;
        let letter_link = dos.join(format!("{letter}:"));
        if std::fs::symlink_metadata(&letter_link).is_ok() || has_drive_entry(&text, letter) {
            return Err(Error::LetterTaken(letter));
        }
        for l in ('a'..='z').filter(|&l| l != 'c' && l != letter) {
            if let Ok(t) = std::fs::read_link(dos.join(format!("{l}::"))) {
                if same_node(&t, &device) {
                    return Err(Error::DeviceAttached(device, l));
                }
            }
        }

        let updated = edit_drives_section(&text, letter, Some("floppy"));
        let number = disk_letters(&updated)
            .iter()
            .position(|&l| l == letter)
            .map(|rank| rank as u32 + 1)
            .expect("the entry was inserted one line above");

        let mount = self.root.join("attached").join(letter.to_string());
        std::fs::create_dir_all(&mount).map_err(|e| Error::Layer(mount.clone(), e))?;
        std::os::unix::fs::symlink(&device, &raw).map_err(|e| Error::Layer(raw.clone(), e))?;
        std::os::unix::fs::symlink(&mount, &letter_link)
            .map_err(|e| Error::Layer(letter_link, e))?;
        let phys = dos.join(format!("physicaldrive{number}"));
        // Only a stale leftover can be here: live attachments hold every
        // rank below ours, and a hand-made physicaldrive link is not a
        // supported state to begin with.
        let _ = std::fs::remove_file(&phys);
        std::os::unix::fs::symlink(&device, &phys).map_err(|e| Error::Layer(phys, e))?;

        text::write_atomic(&reg, &updated)?;

        Ok(Attachment {
            letter,
            device,
            number,
        })
    }

    /// Reverses `attach`. The device itself is untouched.
    pub fn detach(&self, letter: char) -> Result<(), Error> {
        self.ensure_not_running()?;
        check_letter(letter)?;

        let dos = self.prefix().join("dosdevices");
        let raw = dos.join(format!("{letter}::"));
        let reg = self.prefix().join("system.reg");
        let text = std::fs::read_to_string(&reg).map_err(|e| Error::Layer(reg.clone(), e))?;

        // Either half alone still counts: a detach that failed midway must
        // be re-runnable until nothing is left.
        let target = std::fs::read_link(&raw).ok();
        if target.is_none() && !has_drive_entry(&text, letter) {
            return Err(Error::NotAttached(letter));
        }
        let updated = edit_drives_section(&text, letter, None);

        if target.is_some() {
            std::fs::remove_file(&raw).map_err(|e| Error::Layer(raw, e))?;
        }
        let _ = std::fs::remove_file(dos.join(format!("{letter}:")));

        // Rebuild the physicaldrive links from the ranks that remain:
        // removing an entry shifts every later number, in mountmgr and
        // therefore here. First drop every link that belongs to an
        // attachment (or to the device just detached), then re-wire.
        let mut owned: Vec<PathBuf> = target.into_iter().collect();
        let survivors: Vec<(char, PathBuf)> = disk_letters(&updated)
            .iter()
            .filter_map(|&l| {
                let t = std::fs::read_link(dos.join(format!("{l}::"))).ok()?;
                Some((l, t))
            })
            .collect();
        owned.extend(survivors.iter().map(|(_, t)| t.clone()));
        if let Ok(entries) = std::fs::read_dir(&dos) {
            for e in entries.flatten() {
                if !e.file_name().to_string_lossy().starts_with("physicaldrive") {
                    continue;
                }
                if let Ok(t) = std::fs::read_link(e.path()) {
                    if owned.contains(&t) {
                        let _ = std::fs::remove_file(e.path());
                    }
                }
            }
        }
        for (rank, (_, t)) in survivors.iter().enumerate() {
            let phys = dos.join(format!("physicaldrive{}", rank + 1));
            std::os::unix::fs::symlink(t, &phys).map_err(|e| Error::Layer(phys, e))?;
        }

        text::write_atomic(&reg, &updated)?;

        let _ = std::fs::remove_dir(self.root.join("attached").join(letter.to_string()));
        let _ = std::fs::remove_dir(self.root.join("attached"));
        Ok(())
    }
}

/// Whether the user can actually open the device, so `attach` can print the
/// missing grant instead of leaving a program to fail with a bare EACCES.
pub fn accessible(device: &Path) -> bool {
    rustix::fs::access(
        device,
        rustix::fs::Access::READ_OK | rustix::fs::Access::WRITE_OK,
    )
    .is_ok()
}

/// Path equality for the double-attachment guard, through symlinks, so
/// /dev/disk/by-id/… and the /dev/sdX it points at count as the same device.
fn same_node(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// C: is the mounted Windows; a: and b: are floppies by Wine convention, and
/// the registry trick this module relies on only works from c upward anyway.
fn check_letter(letter: char) -> Result<(), Error> {
    if !letter.is_ascii_lowercase() || matches!(letter, 'a' | 'b' | 'c') {
        return Err(Error::BadLetter(letter));
    }
    Ok(())
}

fn check_block_device(device: &Path) -> Result<(), Error> {
    use std::os::unix::fs::FileTypeExt;
    let meta = std::fs::metadata(device).map_err(|e| Error::Layer(device.to_path_buf(), e))?;
    if !meta.file_type().is_block_device() {
        return Err(Error::NotABlockDevice(device.to_path_buf()));
    }
    Ok(())
}

/// The letters whose Drives entries mountmgr turns into `\Device\Harddisk`
/// objects — `"floppy"` on a letter index ≥ 2 — in section order, which is
/// the order mountmgr creates them in and therefore numbers them by.
fn disk_letters(text: &str) -> Vec<char> {
    let mut out = Vec::new();
    let mut in_section = false;
    for line in text.lines() {
        if line.starts_with('[') {
            in_section = line.starts_with(&format!("[{SECTION}]"));
            continue;
        }
        if !in_section {
            continue;
        }
        let Some(body) = line.trim_end().strip_prefix('"') else {
            continue;
        };
        let Some(letter) = body.strip_suffix(":\"=\"floppy\"").and_then(single_char) else {
            continue;
        };
        if letter.is_ascii_lowercase() && letter > 'b' {
            out.push(letter);
        }
    }
    out
}

fn single_char(s: &str) -> Option<char> {
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

/// Whether the Drives section maps the letter at all, whatever the type.
fn has_drive_entry(t: &str, letter: char) -> bool {
    text::has_value(t, SECTION, &format!("{letter}:"))
}

/// Adds, replaces or removes the letter's entry in the Drives section.
///
/// Offline on purpose: it needs no Wine, no mount and no wineserver - and it is
/// only legal because `attach` refuses a running environment, where the server
/// holds the registry in memory and rewrites the file on exit.
fn edit_drives_section(t: &str, letter: char, value: Option<&str>) -> String {
    text::set_value(t, SECTION, &format!("{letter}:"), value)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REG: &str = "WINE REGISTRY Version 2\n;; All keys relative to \\\\Machine\n\n[Software\\\\Fluff] 100\n\"x\"=\"y\"\n\n[Software\\\\Wine\\\\Drives] 200\n\"d:\"=\"cdrom\"\n\n[Software\\\\Zz] 300\n\"z\"=\"1\"\n";

    #[test]
    fn an_existing_entry_is_replaced_in_place() {
        let out = edit_drives_section(REG, 'd', Some("floppy"));
        assert!(out.contains("\"d:\"=\"floppy\""));
        assert!(!out.contains("\"d:\"=\"cdrom\""));
        // Everything else is untouched.
        assert!(out.contains("[Software\\\\Fluff] 100"));
        assert!(out.contains("\"z\"=\"1\""));
    }

    #[test]
    fn a_new_entry_lands_inside_the_existing_section() {
        let out = edit_drives_section(REG, 'e', Some("floppy"));
        let section = out.split("[Software\\\\Zz]").next().unwrap();
        assert!(
            section.contains("\"e:\"=\"floppy\""),
            "the entry must sit before the next section, got:\n{out}"
        );
        assert!(
            section.contains("\"d:\"=\"cdrom\""),
            "the old entry survives"
        );
    }

    #[test]
    fn a_missing_section_is_created() {
        let bare = "WINE REGISTRY Version 2\n\n[Software\\\\Fluff] 100\n\"x\"=\"y\"\n";
        let out = edit_drives_section(bare, 'd', Some("floppy"));
        assert!(out.contains("[Software\\\\Wine\\\\Drives]"));
        assert!(out.contains("\"d:\"=\"floppy\""));
    }

    #[test]
    fn removal_deletes_the_entry_and_nothing_else() {
        let out = edit_drives_section(REG, 'd', None);
        assert!(!out.contains("\"d:\"="));
        assert!(
            out.contains("[Software\\\\Wine\\\\Drives] 200"),
            "the section stays"
        );
        assert!(out.contains("\"x\"=\"y\""));
    }

    #[test]
    fn removing_from_a_file_without_the_section_changes_nothing() {
        let bare = "WINE REGISTRY Version 2\n\n[Software\\\\Fluff] 100\n\"x\"=\"y\"\n";
        assert_eq!(edit_drives_section(bare, 'd', None), bare);
    }

    #[test]
    fn the_edit_is_idempotent() {
        let once = edit_drives_section(REG, 'd', Some("floppy"));
        let twice = edit_drives_section(&once, 'd', Some("floppy"));
        assert_eq!(once, twice);
    }

    #[test]
    fn reserved_and_nonsense_letters_are_refused() {
        for bad in ['a', 'b', 'c', 'D', '3', '?'] {
            assert!(check_letter(bad).is_err(), "{bad:?} must be refused");
        }
        assert!(check_letter('d').is_ok());
        assert!(check_letter('z').is_ok());
    }

    #[test]
    fn a_non_block_device_is_refused_by_name() {
        // /dev/null is a character device; a regular file is not a device at
        // all. Both must be refused - handing a program raw access to the
        // wrong path is how disks get eaten.
        assert!(matches!(
            check_block_device(Path::new("/dev/null")),
            Err(Error::NotABlockDevice(_))
        ));
        let f = std::env::temp_dir().join(format!("raven-blk-{}", std::process::id()));
        std::fs::write(&f, "x").unwrap();
        assert!(matches!(
            check_block_device(&f),
            Err(Error::NotABlockDevice(_))
        ));
        let _ = std::fs::remove_file(&f);
        assert!(check_block_device(Path::new("/definitely/not/here")).is_err());
    }

    #[test]
    fn numbering_follows_mountmgr_rank_not_the_letter() {
        // PhysicalDrive0 is a stub mountmgr pre-creates; registry disks are
        // numbered by their order in the section, from 1. Only "floppy" on a
        // letter ≥ c makes a disk: real floppies and other types do not.
        let text = "[Software\\\\Wine\\\\Drives] 1\n\"a:\"=\"floppy\"\n\"e:\"=\"cdrom\"\n\"g:\"=\"floppy\"\n\"d:\"=\"floppy\"\n";
        assert_eq!(disk_letters(text), vec!['g', 'd']);
        assert_eq!(disk_letters("no section at all\n"), Vec::<char>::new());
    }

    #[test]
    fn any_existing_mapping_counts_as_taken() {
        assert!(
            has_drive_entry(REG, 'd'),
            "a cdrom mapping is still a mapping"
        );
        assert!(!has_drive_entry(REG, 'e'));
        // The x=y value outside the Drives section must not match.
        assert!(!has_drive_entry(REG, 'x'));
    }
}
