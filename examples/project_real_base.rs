//! Projects the deployed Windows base and checks the refusals held.
//!
//! Not a test: it needs a real Windows, which the repository cannot carry. The
//! tested equivalents run against synthetic data in `cargo test`.
fn main() {
    let base =
        std::path::Path::new("/home/mothersphere/.local/share/Colony/Raven/bases/win11-26200-pro");
    let t = std::time::Instant::now();
    let reg = raven::registry::project_base(base, &raven::registry::Rules::default()).unwrap();
    let keys = reg.matches("\r\n[").count();
    println!(
        "  {keys} clés, {} Ko, en {:?}",
        reg.len() / 1024,
        t.elapsed()
    );
    let low = reg.to_lowercase();
    let checks: [(&str, bool); 6] = [
        (
            "HKLM\\System refusé",
            !low.contains("[hkey_local_machine\\system\\"),
        ),
        ("SAM refusé", !low.contains("\\sam\\")),
        (
            "Windows NT\\CurrentVersion refusé",
            !low.contains("windows nt\\currentversion"),
        ),
        (
            "Classes\\Installer refusé",
            !low.contains("classes\\installer"),
        ),
        (
            "Cryptography refusé",
            !low.contains("microsoft\\cryptography"),
        ),
        ("plus aucun X:\\", !reg.contains("X:\\\\")),
    ];
    for (name, ok) in checks {
        println!("  {} {name}", if ok { "✓" } else { "✗ A FUITÉ" });
    }
    println!("\n  --- ce qui a traversé ---");
    let mut roots: std::collections::BTreeMap<String, usize> = Default::default();
    for l in reg.lines().filter(|l| l.starts_with('[')) {
        let parts: Vec<&str> = l.trim_matches(['[', ']']).split('\\').collect();
        *roots
            .entry(parts.iter().take(3).cloned().collect::<Vec<_>>().join("\\"))
            .or_default() += 1;
    }
    let mut v: Vec<_> = roots.into_iter().collect();
    v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (r, n) in v.iter().take(8) {
        println!("    {n:>6}  {r}");
    }
}
