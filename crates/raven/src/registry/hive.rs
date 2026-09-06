//! Reading a Windows registry hive.
//!
//! Read-only is enough, and that is what keeps the whole path free of C and of
//! FFI: Raven never writes a hive. It reads one and emits a `.reg` file, and
//! Wine writes its own registry from that.

use nt_hive::{Hive, KeyNode, KeyValue, KeyValueDataType};

use super::emit::{Data, Key, Value};
use super::rules::Rules;
use crate::Error;

/// Reads one hive and returns the keys the rules permit, with their values.
///
/// `mount` is where this hive's contents live in the registry — `HKLM\Software`
/// for the `SOFTWARE` hive, `HKCU` for a user's `NTUSER.DAT`. A hive file does
/// not record where it belongs; whoever loads it decides, which is why it has to
/// be passed in.
pub fn project(bytes: &[u8], mount: &str, rules: &Rules) -> Result<Vec<Key>, Error> {
    let hive = Hive::new(bytes).map_err(|e| Error::Hive(e.to_string()))?;
    let root = hive
        .root_key_node()
        .map_err(|e| Error::Hive(e.to_string()))?;
    let mut out = Vec::new();
    walk(&root, mount, rules, &mut out, 0);
    Ok(out)
}

/// A hive crafted to be deep would otherwise recurse until the stack runs out.
/// Real registries are nowhere near this deep.
const MAX_DEPTH: usize = 512;

fn walk(node: &KeyNode<'_, &[u8]>, path: &str, rules: &Rules, out: &mut Vec<Key>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }

    if rules.permits(path) {
        let values = read_values(node);
        if !values.is_empty() {
            out.push(Key {
                path: path.to_string(),
                values,
            });
        }
    }

    let Some(Ok(subkeys)) = node.subkeys() else {
        return;
    };
    for sub in subkeys {
        let Ok(sub) = sub else { continue };
        let Ok(name) = sub.name() else { continue };
        let child = format!("{path}\\{}", name.to_string_lossy());

        // Descend even where this node is not itself allowed: an allowed subtree
        // can sit beneath a path that is merely unnamed, and pruning here would
        // lose it. What is *denied*, though, is abandoned at its root — without
        // that, the walk visits every key in a 76 MB hive.
        if rules.permits(&child) || could_contain_allowed(rules, &child) {
            walk(&sub, &child, rules, out, depth + 1);
        }
    }
}

/// Whether an allow rule lies *beneath* `path`, so descending can still reach
/// something.
///
/// Deliberately ignores the deny list. An earlier version short-circuited on a
/// denied ancestor, which silently dropped every allow rule nested inside one —
/// `HKLM\Software\Classes` is refused wholesale and `…\Classes\CLSID` is allowed
/// back, and the whole COM registry, 6 860 keys of it, never crossed. Whether a
/// given key is projected is `permits`'s decision; this only decides whether
/// walking further could ever reach one.
///
/// It stays cheap because it matches only the ancestors of allow rules, which
/// is a handful of paths, not a subtree.
fn could_contain_allowed(rules: &Rules, path: &str) -> bool {
    let p = super::rules::canonical(path);
    rules
        .allow
        .iter()
        .any(|a| a.to_ascii_lowercase().starts_with(&p))
}

fn read_values(node: &KeyNode<'_, &[u8]>) -> Vec<Value> {
    let Some(Ok(values)) = node.values() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for v in values {
        let Ok(v) = v else { continue };
        let Ok(name) = v.name() else { continue };
        let Some(data) = convert(&v) else { continue };
        out.push(Value {
            name: name.to_string_lossy(),
            // A never-booted Windows records itself on X:; see emit.rs.
            data: super::emit::rewrite_setup_drive(&data),
        });
    }
    out
}

fn convert(v: &KeyValue<'_, &[u8]>) -> Option<Data> {
    let bytes = || v.data().ok()?.into_vec().ok();
    Some(match v.data_type().ok()? {
        KeyValueDataType::RegSZ => Data::Sz(utf16_string(&bytes()?)),
        KeyValueDataType::RegExpandSZ => Data::ExpandSz(utf16_string(&bytes()?)),
        KeyValueDataType::RegMultiSZ => Data::MultiSz(utf16_multi(&bytes()?)),
        KeyValueDataType::RegDWord | KeyValueDataType::RegDWordBigEndian => {
            Data::Dword(v.dword_data().ok()?)
        }
        KeyValueDataType::RegQWord => Data::Qword(v.qword_data().ok()?),
        KeyValueDataType::RegNone => Data::None,
        // Binary, resource lists, links: carried across as opaque bytes rather
        // than interpreted. Raven has no reason to understand them, and every
        // reason not to corrupt them.
        _ => Data::Binary(bytes()?),
    })
}

/// Decodes UTF-16LE, stopping at the terminating NUL the registry stores.
///
/// Decoded here rather than through the crate's own helper because that one
/// borrows for the hive's whole lifetime, which a value visited inside a loop
/// cannot provide.
fn utf16_string(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// A `REG_MULTI_SZ` is NUL-separated and ends with an empty string.
fn utf16_multi(bytes: &[u8]) -> Vec<String> {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    units
        .split(|&u| u == 0)
        .filter(|s| !s.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_string_stops_at_its_terminating_nul() {
        // "Hi\0" plus trailing padding the registry often leaves behind.
        let bytes = [0x48, 0x00, 0x69, 0x00, 0x00, 0x00, 0x21, 0x00];
        assert_eq!(utf16_string(&bytes), "Hi");
    }

    #[test]
    fn an_unterminated_string_is_still_decoded() {
        assert_eq!(utf16_string(&[0x48, 0x00, 0x69, 0x00]), "Hi");
    }

    #[test]
    fn an_odd_trailing_byte_does_not_panic() {
        assert_eq!(utf16_string(&[0x48, 0x00, 0x69]), "H");
    }

    #[test]
    fn multi_sz_splits_on_nuls_and_drops_the_terminator() {
        // "a\0b\0\0"
        let bytes = [0x61, 0, 0, 0, 0x62, 0, 0, 0, 0, 0];
        assert_eq!(utf16_multi(&bytes), vec!["a".to_string(), "b".to_string()]);
    }
}
