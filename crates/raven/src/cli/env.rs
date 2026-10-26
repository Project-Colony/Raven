//! `raven env`: building, running, stopping and equipping environments.

use std::path::PathBuf;

use anyhow::Result;
use raven::i18n::{self, Lang, t, tf, tn};
use raven::{env, launch};

use super::EnvCmd;

pub fn env_cmd(cmd: EnvCmd) -> Result<()> {
    match cmd {
        EnvCmd::List => {
            let envs = env::Environment::list()?;
            if envs.is_empty() {
                out!("{}", t("env_none"));
            }
            for e in envs {
                out!(
                    "{}",
                    tf("env_line", &[("name", &e.name), ("base", &e.manifest.base)])
                );
            }
            Ok(())
        }
        EnvCmd::Create { name, base } => {
            let e = env::create(&name, &base)?;
            out!(
                "{}",
                tf(
                    "env_created",
                    &[("name", &e.name), ("base", &e.manifest.base)]
                )
            );
            out!("{}", tf("env_created_hint", &[("name", &e.name)]));
            Ok(())
        }
        EnvCmd::Default { name } => {
            match name {
                Some(n) => {
                    launch::set_default_environment(&n)?;
                    out!("{}", tf("env_default_set", &[("name", &n)]));
                }
                None => match launch::default_environment()? {
                    Some(n) => out!("{n}"),
                    None => out!("{}", t("env_default_none")),
                },
            }
            Ok(())
        }
        EnvCmd::Reproject { name } => {
            let e = env::Environment::open(&name)?;
            let keys = e.project_registry()?;
            out!(
                "{}",
                tn(
                    "env_reprojected",
                    keys,
                    &[("name", &name), ("file", &e.rules_file().display())]
                )
            );
            Ok(())
        }
        EnvCmd::Destroy { name } => {
            env::Environment::destroy(&name)?;
            out!("{}", tf("env_destroyed", &[("name", &name)]));
            Ok(())
        }
        EnvCmd::Status { name } => {
            let e = env::Environment::open(&name)?;
            let holders = e.holders();
            if holders.is_empty() {
                out!("{}", tf("env_not_running", &[("name", &name)]));
            } else {
                out!("{}", tn("env_running", holders.len(), &[("name", &name)]));
                for h in &holders {
                    let role = if h.anchor {
                        t("env_session_anchor")
                    } else if h.is_session() {
                        t("env_wine_service")
                    } else {
                        ""
                    };
                    out!("  {:>7}  {}{role}", h.pid, h.comm);
                }
                out!("{}", tf("env_release", &[("name", &name)]));
            }
            for a in e.attachments() {
                out!(
                    "{}",
                    tf(
                        "env_attachment",
                        &[
                            ("letter", &a.letter),
                            ("device", &a.device.display()),
                            ("number", &a.number)
                        ]
                    )
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
                "{}",
                tf(
                    "env_attached",
                    &[
                        ("device", &a.device.display()),
                        ("letter", &a.letter),
                        ("number", &a.number)
                    ]
                )
            );
            out!("{}", tf("env_attached_warning", &[("name", &name)]));
            out!(
                "{}",
                tf(
                    "env_attached_detach",
                    &[("name", &name), ("letter", &a.letter)]
                )
            );
            if !raven::attach::accessible(&a.device) {
                out!();
                out!("{}", t("env_attached_no_access"));
                out!("  sudo setfacl -m u:$USER:rw {}", a.device.display());
            }
            Ok(())
        }
        EnvCmd::Dxvk { name, from, remove } => d3d_cmd(&raven::d3d::DXVK, &name, from, remove),
        EnvCmd::Vkd3d { name, from, remove } => d3d_cmd(&raven::d3d::VKD3D, &name, from, remove),
        EnvCmd::Detach { name, letter } => {
            let e = env::Environment::open(&name)?;
            e.detach(letter)?;
            out!(
                "{}",
                tf("env_detached", &[("letter", &letter), ("name", &name)])
            );
            Ok(())
        }
        EnvCmd::Start { name } => {
            let e = env::Environment::open(&name)?;
            if e.session().is_some() {
                out!("{}", tf("env_already_up", &[("name", &name)]));
                return Ok(());
            }
            let anchor = e.ensure_session()?;
            out!(
                "{}",
                tf("env_mounted", &[("name", &name), ("anchor", &anchor)])
            );
            // The mount is the cheap half, and it lasts until `env stop`.
            // Wine's services are the two seconds a first launch pays; they
            // are brought up here by running the smallest possible program.
            // The anchor's wineserver is persistent, so they stay up with it.
            // Without one - an older anchor, or a server that would not
            // start - they exit three to six seconds after the program, and
            // the message must not promise more.
            out!("{}", t("env_starting_services"));
            let started = std::time::Instant::now();
            if e.warm_up() {
                let mut seconds = format!("{:.1}", started.elapsed().as_secs_f32());
                if i18n::lang() == Lang::Fr {
                    seconds = seconds.replace('.', ",");
                }
                let key = if e.server_persistent() {
                    "env_ready"
                } else {
                    "env_ready_plain"
                };
                out!("{}", tf(key, &[("name", &name), ("seconds", &seconds)]));
            } else {
                // The mount is up either way, so this is a warning and not a
                // failure: the next launch simply pays what this would have.
                out!("{}", tf("env_wine_did_not_start", &[("name", &name)]));
            }
            Ok(())
        }
        EnvCmd::Stop { name } => {
            let e = env::Environment::open(&name)?;
            let stopped = e.stop()?;
            if stopped.is_empty() {
                out!("{}", tf("env_was_not_running", &[("name", &name)]));
            } else {
                out!("{}", tn("env_stopped", stopped.len(), &[("name", &name)]));
            }
            Ok(())
        }
    }
}

/// Install, remove or report one Direct3D-on-Vulkan runtime.
fn d3d_cmd(
    rt: &raven::d3d::Runtime,
    name: &str,
    from: Option<PathBuf>,
    remove: bool,
) -> Result<()> {
    let e = env::Environment::open(name)?;
    let runtime = &rt.key;
    if let Some(src) = from {
        let done = e.install_d3d(rt, &src)?;
        let mut dlls: Vec<&str> = done.iter().map(|s| s.dll.as_str()).collect();
        dlls.sort();
        dlls.dedup();
        out!(
            "{}",
            tn(
                "d3d_installed",
                done.len(),
                &[
                    ("runtime", runtime),
                    ("name", &name),
                    ("dlls", &dlls.join(", "))
                ]
            )
        );
        out!("{}", t("d3d_installed_shadow"));
        out!(
            "{}",
            tf("d3d_undo", &[("runtime", runtime), ("name", &name)])
        );
    } else if remove {
        let gone = e.remove_d3d(rt)?;
        out!(
            "{}",
            tn(
                "d3d_removed",
                gone,
                &[("runtime", runtime), ("name", &name)]
            )
        );
    } else {
        let files = e.d3d(rt);
        if files.is_empty() {
            out!(
                "{}",
                tf("d3d_none", &[("name", &name), ("runtime", runtime)])
            );
        } else {
            out!(
                "{name}: {}",
                e.d3d_build(rt)
                    .unwrap_or_else(|| tf("d3d_build_unrecorded", &[("runtime", runtime)]))
            );
            for f in &files {
                out!("  {:<10} {}", f.dll, f.arch);
            }
            let over = e.d3d_overrides(rt);
            // The two halves are reported apart on purpose: a file with no
            // override is a library Wine will ignore.
            for f in &files {
                if !over.iter().any(|(n, _)| n == &f.dll) {
                    out!("{}", tf("d3d_no_override", &[("dll", &f.dll)]));
                }
            }
        }
    }
    Ok(())
}
