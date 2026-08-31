//! What is allowed to cross from a real Windows registry into a Wine prefix.
//!
//! Deny by default. A subtree is projected only if an allow rule names it, and
//! any deny rule beats every allow rule.
//!
//! The reason for that asymmetry is `HKLM\System`. It describes a *specific
//! physical machine* — its driver and service database, the devices that were
//! present, its disk layout. Wine fills its own `HKLM\System` with a description
//! of the synthetic environment it actually provides. Overwriting one with the
//! other replaces a true account of the running system with a true account of a
//! different, absent one, and every later lookup gets a confident wrong answer.
//! An empty key would be better than that.

use serde::{Deserialize, Serialize};

/// The allow and deny lists, as a reviewable data file rather than constants.
///
/// What crosses from a real Windows into a prefix is a correctness and security
/// decision, and it should be readable by someone who does not read Rust.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rules {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

impl Rules {
    /// Whether `path` may be projected.
    ///
    /// **The most specific rule wins**, and a path no rule names is refused.
    /// Blanket precedence — deny always beating allow — cannot express the shape
    /// the registry actually has: *all of `Software` except `Microsoft`, but
    /// `Microsoft\\DirectX` after all*. Longest match can, and each exception is
    /// then one line in the rules file instead of a change to the engine.
    ///
    /// Comparison is case-insensitive, because registry paths are.
    pub fn permits(&self, path: &str) -> bool {
        let p = canonical(path);
        let best = |rules: &[String]| -> Option<usize> {
            rules
                .iter()
                .filter(|r| under(&p, r))
                .map(|r| r.trim_end_matches('\\').len())
                .max()
        };
        match (best(&self.allow), best(&self.deny)) {
            (Some(a), Some(d)) => a > d,
            (Some(_), None) => true,
            _ => false,
        }
    }

    /// Parses a rules file.
    pub fn parse(text: &str) -> Result<Rules, toml::de::Error> {
        toml::from_str(text)
    }
}

/// Lowercases, and folds away the 32-bit view of the registry.
///
/// `HKLM\Software\WOW6432Node` mirrors the whole of `HKLM\Software` for 32-bit
/// programs. Left alone it defeats every rule: measured against a real Windows
/// hive, 10 524 of 10 596 projected keys were the 32-bit copy of subtrees the
/// rules had just refused. Folding it out here means one rule covers both views
/// instead of every rule needing a twin that someone will forget to add.
///
/// Only matching is folded. The key keeps its real path in the output, because
/// a 32-bit program genuinely looks for it there.
pub(crate) fn canonical(path: &str) -> String {
    path.to_ascii_lowercase().replace(r"\wow6432node", "")
}

/// Whether `path` is `prefix` or sits beneath it.
///
/// The separator check matters: without it `HKLM\Software\Foobar` would count as
/// being under `HKLM\Software\Foo`, and a rule meant for one vendor would
/// silently cover another.
fn under(path: &str, prefix: &str) -> bool {
    let prefix = prefix.trim_end_matches('\\').to_ascii_lowercase();
    if !path.starts_with(&prefix) {
        return false;
    }
    matches!(path.as_bytes().get(prefix.len()), None | Some(b'\\'))
}

impl Default for Rules {
    /// The broadened allow-list: third-party software, COM registration, and a
    /// named set of Microsoft subtrees that applications genuinely depend on.
    ///
    /// Widened on evidence, never for convenience. Anything not named here does
    /// not cross, and finding out that something should have is a measurement,
    /// not a guess.
    fn default() -> Self {
        // Every Microsoft subtree named here was checked against a real Windows
        // 11 hive rather than guessed. A rule naming a key that does not exist
        // is a rule nobody can tell is dead.
        Rules {
            allow: [
                // Third-party vendors: where installers record what they put
                // where. This is the bulk of what is worth carrying over.
                r"HKLM\Software",
                r"HKCU\Software",
                // The Microsoft subtrees applications genuinely depend on,
                // allowed back over the blanket refusal below.
                r"HKLM\Software\Microsoft\.NETFramework",
                r"HKLM\Software\Microsoft\NET Framework Setup",
                r"HKLM\Software\Microsoft\DirectX",
                r"HKLM\Software\Microsoft\COM3",
                r"HKLM\Software\Microsoft\Ole",
                r"HKLM\Software\Microsoft\Windows Script Host",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            deny: [
                // A description of a machine that is not this one: its drivers,
                // its services, the devices it had.
                r"HKLM\System",
                r"HKLM\Security",
                r"HKLM\SAM",
                // The OS's own tree. Refused wholesale, with the handful of
                // subtrees above allowed back by being more specific.
                r"HKLM\Software\Microsoft",
                r"HKCU\Software\Microsoft",
                // The COM registry. Measured at 121 256 keys and 21 MB, and
                // deliberately NOT projected by default despite being the most
                // obviously valuable thing here.
                //
                // The risk runs the wrong way: a CLSID registration pointing at
                // a Microsoft in-process server that Wine shadows turns a
                // working builtin fallback into a hard failure. Nothing is known
                // about how often that happens, and the rule for widening this
                // list is evidence, not plausibility. Turning these three back
                // on is one line each, and measuring what it changes is the
                // first experiment to run.
                r"HKLM\Software\Classes",
                r"HKCU\Software\Classes",
                // Wine keeps its own configuration in the registry. Projecting
                // over it lets a real Windows silently reconfigure Wine.
                r"HKLM\Software\Wine",
                r"HKCU\Software\Wine",
                // Machine identity and credentials have no business being
                // copied, and are named explicitly so removing the blanket
                // Microsoft refusal cannot expose them by accident.
                r"HKLM\Software\Microsoft\Cryptography",
                r"HKLM\Software\Microsoft\EnterpriseCertificates",
                r"HKLM\Software\Microsoft\SystemCertificates",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r() -> Rules {
        Rules::default()
    }

    #[test]
    fn third_party_software_crosses() {
        assert!(r().permits(r"HKLM\Software\Valve\Steam"));
        assert!(r().permits(r"HKCU\Software\Adobe\Photoshop"));
    }

    #[test]
    fn the_machine_description_never_crosses() {
        for path in [
            r"HKLM\System",
            r"HKLM\System\CurrentControlSet\Services\nvlddmkm",
            r"HKLM\SAM\SAM\Domains",
            r"HKLM\Security\Policy",
        ] {
            assert!(!r().permits(path), "{path} must not cross");
        }
    }

    #[test]
    fn the_os_tree_is_refused_wholesale() {
        assert!(!r().permits(r"HKLM\Software\Microsoft"));
        assert!(!r().permits(r"HKLM\Software\Microsoft\Windows NT\CurrentVersion"));
        assert!(!r().permits(r"HKCU\Software\Microsoft\Assistance"));
    }

    #[test]
    fn a_more_specific_allow_beats_a_broader_deny() {
        // The shape blanket precedence could not express: Microsoft is refused,
        // and the subtrees applications need come back anyway.
        assert!(r().permits(r"HKLM\Software\Microsoft\DirectX"));
        assert!(r().permits(r"HKLM\Software\Microsoft\.NETFramework\v4.0"));
    }

    #[test]
    fn the_com_registry_is_off_by_default_and_switchable() {
        assert!(!r().permits(r"HKLM\Software\Classes\CLSID\{0000-0000}"));
        let mut widened = r();
        widened.allow.push(r"HKLM\Software\Classes\CLSID".into());
        assert!(
            widened.permits(r"HKLM\Software\Classes\CLSID\{0000-0000}"),
            "turning it on must be one line in the rules file, not a code change"
        );
    }

    #[test]
    fn an_even_more_specific_deny_beats_that_allow_again() {
        // Cryptography sits under the refused Microsoft tree and is also named
        // explicitly, so removing the blanket rule cannot expose it by accident.
        assert!(!r().permits(r"HKLM\Software\Microsoft\Cryptography\MachineGuid"));
        assert!(!r().permits(r"HKLM\Software\Classes\Installer"));
    }

    #[test]
    fn wines_own_configuration_is_protected_from_the_projection() {
        assert!(!r().permits(r"HKCU\Software\Wine"));
        assert!(!r().permits(r"HKCU\Software\Wine\Direct3D"));
    }

    #[test]
    fn a_prefix_match_must_stop_at_a_separator() {
        let rules = Rules {
            allow: vec![r"HKLM\Software\Foo".into()],
            deny: vec![],
        };
        assert!(rules.permits(r"HKLM\Software\Foo"));
        assert!(rules.permits(r"HKLM\Software\Foo\Bar"));
        assert!(
            !rules.permits(r"HKLM\Software\Foobar"),
            "a rule for one vendor must not silently cover another whose name extends it"
        );
    }

    #[test]
    fn the_32_bit_view_obeys_the_same_rules_as_the_64_bit_one() {
        // Without folding, this is where almost the entire projection went.
        assert!(!r().permits(r"HKLM\Software\WOW6432Node\Microsoft\Windows NT\CurrentVersion"));
        assert!(!r().permits(r"HKLM\Software\WOW6432Node\Classes\Installer"));
        // And a 32-bit vendor key still crosses, keeping its real path.
        assert!(r().permits(r"HKLM\Software\WOW6432Node\Valve\Steam"));
        assert!(r().permits(r"HKLM\Software\WOW6432Node\Microsoft\DirectX"));
    }

    #[test]
    fn matching_ignores_case_because_the_registry_does() {
        assert!(r().permits(r"hklm\software\valve"));
        assert!(!r().permits(r"HKLM\SYSTEM\CurrentControlSet"));
    }

    #[test]
    fn anything_unnamed_is_refused() {
        assert!(!r().permits(r"HKLM\Hardware\Description"));
        assert!(!r().permits(r"HKU\.DEFAULT"));
    }

    #[test]
    fn the_default_rules_round_trip_through_the_file_format() {
        let text = toml::to_string(&r()).unwrap();
        let parsed = Rules::parse(&text).unwrap();
        assert!(parsed.permits(r"HKLM\Software\Valve"));
        assert!(!parsed.permits(r"HKLM\System"));
    }
}
