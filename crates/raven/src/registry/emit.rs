//! Writing what crossed as a `.reg` file for Wine to import.
//!
//! Raven emits the documented `regedit` format and lets Wine's own `regedit`
//! import it, rather than writing the prefix's `system.reg` directly. Wine's
//! files are Wine's internal state and it is free to change their shape; the
//! `.reg` format is an interface with a stable definition. Reaching into another
//! project's private files to save one subprocess is how a tool breaks on
//! somebody else's release.

use std::fmt::Write as _;

/// A registry value's data, in the types the `.reg` format can express.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Data {
    Sz(String),
    ExpandSz(String),
    MultiSz(Vec<String>),
    Dword(u32),
    Qword(u64),
    Binary(Vec<u8>),
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    /// Empty means the key's default value, written as `@=`.
    pub name: String,
    pub data: Data,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub path: String,
    pub values: Vec<Value>,
}

/// Renders keys as a `.reg` file.
pub fn to_reg(keys: &[Key]) -> String {
    let mut out = String::from("Windows Registry Editor Version 5.00\r\n");
    for key in keys {
        let _ = write!(out, "\r\n[{}]\r\n", expand_root(&key.path));
        for value in &key.values {
            let name = if value.name.is_empty() {
                "@".to_string()
            } else {
                format!("\"{}\"", escape(&value.name))
            };
            let _ = write!(out, "{name}={}\r\n", render(&value.data, name.len() + 1));
        }
    }
    out
}

/// `HKLM` and `HKCU` are how Raven names roots internally; `regedit` wants them
/// spelled out.
fn expand_root(path: &str) -> String {
    for (short, long) in [
        ("HKLM\\", "HKEY_LOCAL_MACHINE\\"),
        ("HKCU\\", "HKEY_CURRENT_USER\\"),
        ("HKCR\\", "HKEY_CLASSES_ROOT\\"),
    ] {
        if let Some(rest) = path.strip_prefix(short) {
            return format!("{long}{rest}");
        }
    }
    path.to_string()
}

fn render(data: &Data, prefix_len: usize) -> String {
    match data {
        Data::Sz(s) => format!("\"{}\"", escape(s)),
        Data::Dword(v) => format!("dword:{v:08x}"),
        Data::Binary(b) => format!("hex:{}", hex_list(b, prefix_len + 4)),
        Data::ExpandSz(s) => format!("hex(2):{}", hex_list(&utf16(s), prefix_len + 7)),
        Data::MultiSz(parts) => {
            // REG_MULTI_SZ is NUL-separated and NUL-terminated, so the list ends
            // with two NULs: one for the last string, one for the list.
            let mut bytes = Vec::new();
            for p in parts {
                bytes.extend(utf16(p));
            }
            bytes.extend([0, 0]);
            format!("hex(7):{}", hex_list(&bytes, prefix_len + 7))
        }
        Data::Qword(v) => format!("hex(b):{}", hex_list(&v.to_le_bytes(), prefix_len + 7)),
        Data::None => "hex(0):".to_string(),
    }
}

/// UTF-16LE with the terminating NUL the registry stores.
fn utf16(s: &str) -> Vec<u8> {
    let mut out: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
    out.extend([0, 0]);
    out
}

/// `.reg` wraps hex payloads at 80 columns with a trailing backslash, and
/// regedit rejects a line that runs past it.
fn hex_list(bytes: &[u8], prefix_len: usize) -> String {
    // Each cell renders as "ab," - three columns. The first line also carries
    // the value name and type, which is why the caller passes its width: a
    // wrap computed from the payload alone overflows on a long value name.
    const LIMIT: usize = 76;
    let mut out = String::new();
    let mut column = prefix_len;
    for (i, byte) in bytes.iter().enumerate() {
        if column + 3 > LIMIT {
            out.push_str("\\\r\n  ");
            column = 2;
        }
        let _ = write!(out, "{byte:02x}");
        column += 2;
        if i + 1 != bytes.len() {
            out.push(',');
            column += 1;
        }
    }
    out
}

fn escape(s: &str) -> String {
    s.replace('\\', r"\\").replace('"', "\\\"")
}

/// Rewrites the drive letter a never-booted Windows records for itself.
///
/// A Windows applied from a WIM has never run `specialize`, so its hive still
/// describes the *setup* environment: `SystemRoot` reads `X:\Windows`, the
/// letter Windows Setup runs from. Under Raven the installation is C:, and every
/// path that says otherwise sends a program somewhere that does not exist.
pub fn rewrite_setup_drive(data: &Data) -> Data {
    /// Replaces every `X:\` that is really a drive letter.
    ///
    /// Not only at the start: real values wrap the path in a resource reference
    /// or a quote, as in `@X:\Program Files\...` and `"X:\Windows\..."`, and a
    /// check anchored to position zero misses all of them.
    ///
    /// A letter before the `X` means it is part of a longer word, not a drive.
    fn fix(s: &str) -> String {
        let b = s.as_bytes();
        let mut out = String::with_capacity(s.len());
        let mut i = 0;
        while i < s.len() {
            let starts_drive = b[i].eq_ignore_ascii_case(&b'X')
                && b.get(i + 1) == Some(&b':')
                && b.get(i + 2) == Some(&b'\\')
                && !i
                    .checked_sub(1)
                    .is_some_and(|j| b[j].is_ascii_alphanumeric());
            if starts_drive {
                out.push('C');
                i += 1;
            } else {
                // Walk by character so a multi-byte value is not split.
                let ch = s[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
            }
        }
        out
    }
    match data {
        Data::Sz(s) => Data::Sz(fix(s)),
        Data::ExpandSz(s) => Data::ExpandSz(fix(s)),
        Data::MultiSz(v) => Data::MultiSz(v.iter().map(|s| fix(s)).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(path: &str, values: Vec<Value>) -> Key {
        Key {
            path: path.into(),
            values,
        }
    }
    fn v(name: &str, data: Data) -> Value {
        Value {
            name: name.into(),
            data,
        }
    }

    #[test]
    fn the_file_opens_with_the_header_regedit_requires() {
        assert!(to_reg(&[]).starts_with("Windows Registry Editor Version 5.00\r\n"));
    }

    #[test]
    fn roots_are_spelled_out_for_regedit() {
        let out = to_reg(&[key(r"HKLM\Software\Valve", vec![])]);
        assert!(
            out.contains(r"[HKEY_LOCAL_MACHINE\Software\Valve]"),
            "{out}"
        );
    }

    #[test]
    fn a_path_inside_a_string_keeps_its_backslashes() {
        let out = to_reg(&[key(
            r"HKLM\Software\Valve",
            vec![v("InstallPath", Data::Sz(r"C:\Program Files\Steam".into()))],
        )]);
        assert!(
            out.contains(r#""InstallPath"="C:\\Program Files\\Steam""#),
            "an unescaped backslash makes regedit read the next character as an escape: {out}"
        );
    }

    #[test]
    fn a_quote_inside_a_value_is_escaped() {
        let out = to_reg(&[key(
            "HKLM\\Software\\X",
            vec![v("Cmd", Data::Sz("say \"hi\"".into()))],
        )]);
        assert!(out.contains(r#""Cmd"="say \"hi\"""#), "{out}");
    }

    #[test]
    fn the_default_value_is_written_as_an_at_sign() {
        let out = to_reg(&[key(
            r"HKLM\Software\X",
            vec![v("", Data::Sz("default".into()))],
        )]);
        assert!(out.contains("@=\"default\""), "{out}");
    }

    #[test]
    fn a_dword_is_eight_hex_digits() {
        let out = to_reg(&[key(r"HKLM\Software\X", vec![v("N", Data::Dword(42))])]);
        assert!(out.contains("\"N\"=dword:0000002a"), "{out}");
    }

    #[test]
    fn multi_sz_ends_with_two_nuls() {
        // One terminates the final string, one terminates the list. A single NUL
        // makes the reader run past the end of the value.
        let out = to_reg(&[key(
            r"HKLM\Software\X",
            vec![v("L", Data::MultiSz(vec!["a".into()]))],
        )]);
        let hex: String = out
            .lines()
            .find(|l| l.starts_with("\"L\""))
            .unwrap()
            .to_string();
        assert!(hex.ends_with("61,00,00,00,00,00"), "{hex}");
    }

    #[test]
    fn long_binary_values_wrap_the_way_regedit_expects() {
        let out = to_reg(&[key(
            r"HKLM\Software\X",
            vec![v("B", Data::Binary(vec![0xAB; 60]))],
        )]);
        assert!(
            out.contains("\\\r\n  "),
            "a long hex payload must be wrapped: {out}"
        );
        assert!(
            out.lines().all(|l| l.len() < 80),
            "regedit rejects a line running past 80 columns"
        );
    }

    #[test]
    fn the_setup_drive_letter_is_rewritten_to_c() {
        assert_eq!(
            rewrite_setup_drive(&Data::Sz(r"X:\Windows".into())),
            Data::Sz(r"C:\Windows".into())
        );
        assert_eq!(
            rewrite_setup_drive(&Data::ExpandSz(r"x:\Windows\System32".into())),
            Data::ExpandSz(r"C:\Windows\System32".into())
        );
    }

    #[test]
    fn the_drive_letter_is_rewritten_wherever_it_appears() {
        // The shapes that actually occur in a real hive.
        assert_eq!(
            rewrite_setup_drive(&Data::Sz(r"@X:\Program Files\IE\x.exe,-702".into())),
            Data::Sz(r"@C:\Program Files\IE\x.exe,-702".into())
        );
        assert_eq!(
            rewrite_setup_drive(&Data::Sz(
                "\"X:\\Windows\\System32\\ie4uinit.exe\" -r".into()
            )),
            Data::Sz("\"C:\\Windows\\System32\\ie4uinit.exe\" -r".into())
        );
    }

    #[test]
    fn a_letter_before_the_x_means_it_is_not_a_drive() {
        assert_eq!(
            rewrite_setup_drive(&Data::Sz(r"MAX:\nope".into())),
            Data::Sz(r"MAX:\nope".into())
        );
    }

    #[test]
    fn a_multi_byte_value_survives_the_rewrite() {
        assert_eq!(
            rewrite_setup_drive(&Data::Sz("café X:\\Windows".into())),
            Data::Sz("café C:\\Windows".into())
        );
    }

    #[test]
    fn rewriting_leaves_other_drives_and_other_types_alone() {
        assert_eq!(
            rewrite_setup_drive(&Data::Sz(r"D:\Games".into())),
            Data::Sz(r"D:\Games".into())
        );
        // A value that merely starts with an x is not a drive letter.
        assert_eq!(
            rewrite_setup_drive(&Data::Sz("xterm".into())),
            Data::Sz("xterm".into())
        );
        assert_eq!(rewrite_setup_drive(&Data::Dword(7)), Data::Dword(7));
    }
}
