//! Editing Wine's text-format registry files offline.
//!
//! Wine keeps the registry as two plain-text files - `system.reg` for HKLM and
//! `user.reg` for HKCU - in a format of its own: sections written
//! `[Software\\Wine\\Drives] <epoch>` and values written `"name"="value"`.
//!
//! Editing them without Wine is legal only while the environment is **stopped**.
//! A running `wineserver` holds the whole registry in memory and rewrites both
//! files when it exits, so an edit made underneath it is silently discarded.
//! Every caller here checks that first.
//!
//! Which file a setting belongs in is not guessable and has already cost this
//! project once: drive configuration is read from HKLM (`system.reg`), DLL
//! overrides from HKCU (`user.reg`). Verify against Wine before adding a third.

use std::path::Path;

use crate::Error;

/// Replaces a registry file atomically.
///
/// The file is an entire registry branch - the projected base plus everything
/// installers have written since - and an in-place rewrite torn by ENOSPC or a
/// crash would cost all of it. This is the same temp-file-then-rename dance
/// wineserver itself performs to save these files.
pub fn write_atomic(reg: &Path, updated: &str) -> Result<(), Error> {
    let mut name = reg.file_name().unwrap_or_default().to_os_string();
    name.push(".raven-tmp");
    let tmp = reg.with_file_name(name);
    let result = (|| {
        use std::io::Write as _;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(updated.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, reg)
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error::Layer(reg.to_path_buf(), e));
    }
    Ok(())
}

/// Adds, replaces or removes one value in one section. `None` removes it.
///
/// `section` is written exactly as it appears between the brackets in the file,
/// with the backslashes already doubled: `Software\\\\Wine\\\\DllOverrides` in
/// Rust source. A missing section is created; an emptied one is left in place,
/// because Wine wrote it and its timestamp is not ours to discard.
pub fn set_value(text: &str, section: &str, key: &str, value: Option<&str>) -> String {
    let header = format!("[{section}]");
    let entry = |v: &str| format!("\"{key}\"=\"{v}\"");
    let prefix = format!("\"{key}\"=");

    let mut out = Vec::new();
    let mut in_section = false;
    let mut section_seen = false;
    let mut written = false;

    for line in text.lines() {
        if line.starts_with('[') {
            // Leaving the section without having written the value: it belongs
            // inside, so place it before the next section opens.
            if in_section && !written {
                if let Some(v) = value {
                    out.push(entry(v));
                }
                written = true;
            }
            in_section = line.starts_with(&header);
            if in_section {
                section_seen = true;
            }
            out.push(line.to_string());
            continue;
        }
        if in_section && line.starts_with(&prefix) {
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
            out.push(format!("{header} {epoch}"));
            out.push(entry(v));
        }
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// Whether the section carries the key at all, whatever its value.
pub fn has_value(text: &str, section: &str, key: &str) -> bool {
    let header = format!("[{section}]");
    let prefix = format!("\"{key}\"=");
    let mut in_section = false;
    for line in text.lines() {
        if line.starts_with('[') {
            in_section = line.starts_with(&header);
            continue;
        }
        if in_section && line.starts_with(&prefix) {
            return true;
        }
    }
    false
}

/// Every `"name"="value"` pair in a section, in file order.
pub fn values(text: &str, section: &str) -> Vec<(String, String)> {
    let header = format!("[{section}]");
    let mut out = Vec::new();
    let mut in_section = false;
    for line in text.lines() {
        if line.starts_with('[') {
            in_section = line.starts_with(&header);
            continue;
        }
        if !in_section {
            continue;
        }
        let Some(rest) = line.trim_end().strip_prefix('"') else {
            continue;
        };
        let Some((name, val)) = rest.split_once("\"=\"") else {
            continue;
        };
        let Some(val) = val.strip_suffix('"') else {
            continue;
        };
        out.push((name.to_string(), val.to_string()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECTION: &str = "Software\\\\Wine\\\\DllOverrides";
    const REG: &str = "WINE REGISTRY Version 2\n\n[Software\\\\Fluff] 100\n\"x\"=\"y\"\n\n\
                       [Software\\\\Wine\\\\DllOverrides] 200\n\"d3d9\"=\"builtin\"\n\n\
                       [Software\\\\Zz] 300\n\"z\"=\"1\"\n";

    #[test]
    fn an_existing_value_is_replaced_in_place() {
        let out = set_value(REG, SECTION, "d3d9", Some("native"));
        assert!(out.contains("\"d3d9\"=\"native\""));
        assert!(!out.contains("builtin"));
        assert!(out.contains("\"x\"=\"y\""), "other sections are untouched");
        assert!(out.contains("\"z\"=\"1\""));
    }

    #[test]
    fn a_new_value_lands_inside_the_existing_section() {
        let out = set_value(REG, SECTION, "d3d11", Some("native"));
        let before_next = out.split("[Software\\\\Zz]").next().unwrap();
        assert!(before_next.contains("\"d3d11\"=\"native\""), "got:\n{out}");
        assert!(
            before_next.contains("\"d3d9\"=\"builtin\""),
            "the old value survives"
        );
    }

    #[test]
    fn a_missing_section_is_created() {
        let bare = "WINE REGISTRY Version 2\n\n[Software\\\\Fluff] 100\n\"x\"=\"y\"\n";
        let out = set_value(bare, SECTION, "d3d11", Some("native"));
        assert!(out.contains("[Software\\\\Wine\\\\DllOverrides]"));
        assert!(out.contains("\"d3d11\"=\"native\""));
    }

    #[test]
    fn removal_takes_the_value_and_leaves_the_section() {
        let out = set_value(REG, SECTION, "d3d9", None);
        assert!(!out.contains("\"d3d9\""));
        assert!(
            out.contains("[Software\\\\Wine\\\\DllOverrides] 200"),
            "Wine wrote that section and its timestamp; we do not discard it"
        );
    }

    #[test]
    fn removing_something_absent_changes_nothing() {
        let bare = "WINE REGISTRY Version 2\n\n[Software\\\\Fluff] 100\n\"x\"=\"y\"\n";
        assert_eq!(set_value(bare, SECTION, "d3d11", None), bare);
    }

    #[test]
    fn the_edit_is_idempotent() {
        let once = set_value(REG, SECTION, "d3d11", Some("native"));
        assert_eq!(set_value(&once, SECTION, "d3d11", Some("native")), once);
    }

    #[test]
    fn a_value_in_another_section_is_not_matched() {
        // "x" lives in Fluff, not in DllOverrides.
        assert!(!has_value(REG, SECTION, "x"));
        assert!(has_value(REG, SECTION, "d3d9"));
        assert!(!has_value(REG, SECTION, "d3d11"));
    }

    #[test]
    fn values_reads_back_only_its_own_section() {
        assert_eq!(
            values(REG, SECTION),
            vec![("d3d9".to_string(), "builtin".to_string())]
        );
        let two = set_value(REG, SECTION, "dxgi", Some("native"));
        assert_eq!(values(&two, SECTION).len(), 2);
    }

    #[test]
    fn the_write_is_a_rename_and_leaves_no_droppings() {
        let dir = std::env::temp_dir().join(format!("raven-regtext-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let reg = dir.join("user.reg");
        std::fs::write(&reg, "old").unwrap();
        write_atomic(&reg, "new\n").unwrap();
        assert_eq!(std::fs::read_to_string(&reg).unwrap(), "new\n");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
