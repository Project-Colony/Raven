//! `raven env`: building, running, stopping and equipping environments.

use std::path::PathBuf;

use anyhow::Result;
use raven::{env, launch};

use super::EnvCmd;

pub fn env_cmd(cmd: EnvCmd) -> Result<()> {
    match cmd {
        EnvCmd::List => {
            let envs = env::Environment::list()?;
            if envs.is_empty() {
                out!("No environments. Create one with `raven env create`.");
            }
            for e in envs {
                out!("{}  (base {})", e.name, e.manifest.base);
            }
            Ok(())
        }
        EnvCmd::Create { name, base } => {
            let e = env::create(&name, &base)?;
            out!("Created {} against base {}", e.name, e.manifest.base);
            out!(
                "Run something with: raven run {} -- wine <program.exe>",
                e.name
            );
            Ok(())
        }
        EnvCmd::Default { name } => {
            match name {
                Some(n) => {
                    launch::set_default_environment(&n)?;
                    out!("Programs outside an environment will run in {n}.");
                }
                None => match launch::default_environment()? {
                    Some(n) => out!("{n}"),
                    None => {
                        out!("No default environment. Set one with `raven env default <name>`.")
                    }
                },
            }
            Ok(())
        }
        EnvCmd::Reproject { name } => {
            let e = env::Environment::open(&name)?;
            let keys = e.project_registry()?;
            out!(
                "Projected {keys} keys into {name} using {}",
                e.rules_file().display()
            );
            Ok(())
        }
        EnvCmd::Destroy { name } => {
            env::Environment::destroy(&name)?;
            out!("Destroyed {name}. The base is untouched.");
            Ok(())
        }
        EnvCmd::Status { name } => {
            let e = env::Environment::open(&name)?;
            let holders = e.holders();
            if holders.is_empty() {
                out!("{name}: not running");
            } else {
                let (n, s) = plural(holders.len());
                out!("{name}: running - {n} process{s} holding its C:");
                for h in &holders {
                    let role = if h.anchor { "  (session anchor)" } else { "" };
                    out!("  {:>7}  {}{role}", h.pid, h.comm);
                }
                out!("Release it: raven env stop {name}");
            }
            for a in e.attachments() {
                out!(
                    "attached: {}: -> {}  (\\\\.\\PhysicalDrive{}, raw access)",
                    a.letter,
                    a.device.display(),
                    a.number
                );
            }
            Ok(())
        }
        EnvCmd::Attach {
            name,
            device,
            letter,
        } => {
            let e = env::Environment::open(&name)?;
            let a = e.attach(&device, letter)?;
            out!(
                "Attached {} as {}: and \\\\.\\PhysicalDrive{}.",
                a.device.display(),
                a.letter,
                a.number
            );
            out!("A program in {name} can now read and WRITE this device's sectors.");
            out!("Detach it: raven env detach {name} --letter {}", a.letter);
            if !raven::attach::accessible(&a.device) {
                out!();
                out!("You cannot open the device yet. Grant yourself access (lasts");
                out!("until it is replugged):");
                out!("  sudo setfacl -m u:$USER:rw {}", a.device.display());
            }
            Ok(())
        }
        EnvCmd::Dxvk { name, from, remove } => d3d_cmd(&raven::d3d::DXVK, &name, from, remove),
        EnvCmd::Vkd3d { name, from, remove } => d3d_cmd(&raven::d3d::VKD3D, &name, from, remove),
        EnvCmd::Detach { name, letter } => {
            let e = env::Environment::open(&name)?;
            e.detach(letter)?;
            out!("Detached {letter}: from {name}. The device itself is untouched.");
            Ok(())
        }
        EnvCmd::Start { name } => {
            let e = env::Environment::open(&name)?;
            if e.session().is_some() {
                out!("{name} is already up.");
                return Ok(());
            }
            let anchor = e.ensure_session()?;
            out!("Mounted {name} (session {anchor}).");
            // The mount is the cheap half, and it lasts until `env stop`.
            // Wine's services are the two seconds a first launch pays; they
            // are brought up here by running the smallest possible program.
            // Nothing makes wineserver persistent, so they exit three to six
            // seconds after it, and only a launch within that window joins them.
            out!("Starting Wine's services...");
            let started = std::time::Instant::now();
            if e.warm_up() {
                out!(
                    "{name} is ready in {:.1}s. A launch within a few seconds joins Wine's services; a later one starts them again.",
                    started.elapsed().as_secs_f32()
                );
            } else {
                // The mount is up either way, so this is a warning and not a
                // failure: the next launch simply pays what this would have.
                out!("{name} is mounted, but Wine did not start; the first launch will.");
            }
            Ok(())
        }
        EnvCmd::Stop { name } => {
            let e = env::Environment::open(&name)?;
            let stopped = e.stop()?;
            if stopped.is_empty() {
                out!("{name} was not running.");
            } else {
                let (n, s) = plural(stopped.len());
                out!("Stopped {name}: {n} process{s} terminated.");
            }
            Ok(())
        }
    }
}

fn plural(n: usize) -> (usize, &'static str) {
    (n, if n == 1 { "" } else { "es" })
}

/// Install, remove or report one Direct3D-on-Vulkan runtime.
fn d3d_cmd(
    rt: &raven::d3d::Runtime,
    name: &str,
    from: Option<PathBuf>,
    remove: bool,
) -> Result<()> {
    let e = env::Environment::open(name)?;
    if let Some(src) = from {
        let done = e.install_d3d(rt, &src)?;
        let mut dlls: Vec<&str> = done.iter().map(|s| s.dll.as_str()).collect();
        dlls.sort();
        dlls.dedup();
        // plural() appends an "s"; "library" does not take one.
        let word = if done.len() == 1 {
            "library"
        } else {
            "libraries"
        };
        out!(
            "Installed {} {} {word} into {name}: {}",
            done.len(),
            rt.key,
            dlls.join(", ")
        );
        out!("They shadow the real Windows through the overlay; the base is untouched.");
        out!("Undo it: raven env {} {name} --remove", rt.key);
    } else if remove {
        let gone = e.remove_d3d(rt)?;
        let word = if gone == 1 { "library" } else { "libraries" };
        out!(
            "Removed {gone} {} {word} from {name}; the real Windows is uncovered.",
            rt.key
        );
    } else {
        let files = e.d3d(rt);
        if files.is_empty() {
            out!("{name}: no {} installed", rt.key);
        } else {
            out!(
                "{name}: {}",
                e.d3d_build(rt)
                    .unwrap_or_else(|| format!("{}, build unrecorded", rt.key))
            );
            for f in &files {
                out!("  {:<10} {}", f.dll, f.arch);
            }
            let over = e.d3d_overrides(rt);
            // The two halves are reported apart on purpose: a file with no
            // override is a library Wine will ignore.
            for f in &files {
                if !over.iter().any(|(n, _)| n == &f.dll) {
                    out!("warning: {} is installed but has no DLL override", f.dll);
                }
            }
        }
    }
    Ok(())
}
