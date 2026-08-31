//! Projects a deployed Windows base and checks that the refusals held.
//!
//! Not a test: it needs a real Windows, which the repository cannot carry. The
//! tested equivalents run against synthetic hives in `cargo test`.
//!
//! ```text
//! cargo run --example project_real_base -- <path to a deployed base>
//! ```

fn main() {
    let Some(arg) = std::env::args().nth(1) else {
        eprintln!("usage: project_real_base <path to a deployed base>");
        eprintln!("       (see `raven base list` for what is deployed)");
        std::process::exit(2);
    };
    let base = std::path::Path::new(&arg);

    let started = std::time::Instant::now();
    let reg = match raven::registry::project_base(base, &raven::registry::Rules::default()) {
        Ok(reg) => reg,
        Err(e) => {
            eprintln!("could not project {}: {e}", base.display());
            std::process::exit(1);
        }
    };

    println!(
        "{} keys, {} KiB, in {:?}",
        reg.matches("\r\n[").count(),
        reg.len() / 1024,
        started.elapsed()
    );

    let low = reg.to_lowercase();
    let checks: [(&str, bool); 6] = [
        ("HKLM\\System refused", !low.contains("[hkey_local_machine\\system\\")),
        ("SAM refused", !low.contains("\\sam\\")),
        ("Windows NT\\CurrentVersion refused", !low.contains("windows nt\\currentversion")),
        ("Classes\\Installer refused", !low.contains("classes\\installer")),
        ("Cryptography refused", !low.contains("microsoft\\cryptography")),
        ("no X: left", !reg.contains("X:\\\\")),
    ];
    let mut failed = false;
    for (name, ok) in checks {
        println!("  {} {name}", if ok { "ok" } else { "LEAKED" });
        failed |= !ok;
    }
    if failed {
        std::process::exit(1);
    }
}
