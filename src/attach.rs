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
//! What this deliberately does not do: make the device *enumerable*. Tools
//! that discover disks through SetupDi (Rufus among them) stay blind — no
//! configuration can register the device interface they query. And Raven
//! never touches the device node's permissions: access is the user's to
//! grant, and `attach` prints the command rather than running it.

use std::path::{Path, PathBuf};

use crate::{Error, env::Environment};

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
        let mut found = Vec::new();
        for l in 'a'..='z' {
            let raw = dos.join(format!("{l}::"));
            let Ok(device) = std::fs::read_link(&raw) else {
                continue;
            };
            // The C: drive's own device link, if any, is not an attachment.
            if l == 'c' {
                continue;
            }
            found.push(Attachment {
                letter: l,
                device,
                number: letter_number(l),
            });
        }
        found
    }

    /// Wires a block device into the environment under a drive letter.
    ///
    /// Refuses while the environment runs: `wineserver` holds the registry
    /// in memory and would overwrite the edit on exit.
    pub fn attach(&self, device: &Path, letter: char) -> Result<Attachment, Error> {
        self.ensure_not_running()?;
        check_letter(letter)?;
        check_block_device(device)?;

        let dos = self.prefix().join("dosdevices");
        let raw = dos.join(format!("{letter}::"));
        if raw.exists() || std::fs::symlink_metadata(&raw).is_ok() {
            return Err(Error::AlreadyAttached(letter));
        }

        let number = letter_number(letter);
        let mount = self.root.join("attached").join(letter.to_string());
        std::fs::create_dir_all(&mount).map_err(|e| Error::Layer(mount.clone(), e))?;

        std::os::unix::fs::symlink(device, &raw).map_err(|e| Error::Layer(raw.clone(), e))?;
        let letter_link = dos.join(format!("{letter}:"));
        let _ = std::fs::remove_file(&letter_link);
        std::os::unix::fs::symlink(&mount, &letter_link)
            .map_err(|e| Error::Layer(letter_link, e))?;
        let phys = dos.join(format!("physicaldrive{number}"));
        let _ = std::fs::remove_file(&phys);
        std::os::unix::fs::symlink(device, &phys).map_err(|e| Error::Layer(phys, e))?;

        set_drive_type(&self.prefix().join("system.reg"), letter, Some("floppy"))?;

        Ok(Attachment {
            letter,
            device: device.to_path_buf(),
            number,
        })
    }

    /// Reverses `attach`. The device itself is untouched.
    pub fn detach(&self, letter: char) -> Result<(), Error> {
        self.ensure_not_running()?;
        check_letter(letter)?;

        let dos = self.prefix().join("dosdevices");
        let raw = dos.join(format!("{letter}::"));
        if std::fs::symlink_metadata(&raw).is_err() {
            return Err(Error::NotAttached(letter));
        }
        std::fs::remove_file(&raw).map_err(|e| Error::Layer(raw, e))?;
        let _ = std::fs::remove_file(dos.join(format!("{letter}:")));
        let _ = std::fs::remove_file(dos.join(format!("physicaldrive{}", letter_number(letter))));
        let _ = std::fs::remove_dir(self.root.join("attached").join(letter.to_string()));
        let _ = std::fs::remove_dir(self.root.join("attached"));

        set_drive_type(&self.prefix().join("system.reg"), letter, None)?;
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

fn letter_number(letter: char) -> u32 {
    (letter as u32) - ('a' as u32)
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

/// Adds, replaces or removes the letter's entry in `[Software\\Wine\\Drives]`
/// inside Wine's text-format `system.reg`, offline.
///
/// Offline on purpose: it needs no Wine, no mount, and no wineserver — and it
/// is only legal because `attach` refuses a running environment, where the
/// server holds the registry in memory and rewrites the file on exit.
fn set_drive_type(reg: &Path, letter: char, value: Option<&str>) -> Result<(), Error> {
    let text = std::fs::read_to_string(reg).map_err(|e| Error::Layer(reg.to_path_buf(), e))?;
    let updated = edit_drives_section(&text, letter, value);
    std::fs::write(reg, updated).map_err(|e| Error::Layer(reg.to_path_buf(), e))
}

/// The pure edit, separated so the format handling is testable against
/// fixture files rather than a live prefix.
fn edit_drives_section(text: &str, letter: char, value: Option<&str>) -> String {
    const HEADER: &str = "[Software\\\\Wine\\\\Drives]";
    let entry = |v: &str| format!("\"{letter}:\"=\"{v}\"");
    let key = format!("\"{letter}:\"=");

    let mut out = Vec::new();
    let mut in_section = false;
    let mut section_seen = false;
    let mut written = false;

    for line in text.lines() {
        if line.starts_with('[') {
            // Leaving the Drives section without having written the value:
            // insert it before the next section starts.
            if in_section && !written {
                if let Some(v) = value {
                    out.push(entry(v));
                }
                written = true;
            }
            in_section = line.starts_with(HEADER);
            if in_section {
                section_seen = true;
            }
            out.push(line.to_string());
            continue;
        }
        if in_section && line.starts_with(&key) {
            // Replace or drop the existing entry.
            if let Some(v) = value {
                out.push(entry(v));
            }
            written = true;
            continue;
        }
        out.push(line.to_string());
    }
    if in_section && !written {
        if let Some(v) = value {
            out.push(entry(v));
        }
        written = true;
    }
    if !section_seen && !written {
        if let Some(v) = value {
            let epoch = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            out.push(String::new());
            out.push(format!("{HEADER} {epoch}"));
            out.push(entry(v));
        }
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
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
    fn the_physicaldrive_number_follows_the_letter() {
        assert_eq!(letter_number('d'), 3);
        assert_eq!(letter_number('z'), 25);
    }
}
