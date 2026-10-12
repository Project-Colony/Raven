//! `raven launch`, `raven run` and `raven exec`: the three ways a program
//! starts, each replacing this process with it.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use raven::mount::{MountBackend, OverlaySpec, UserNsOverlay};
use raven::{env, launch};

/// Runs a Windows program in the environment its path resolves to.
pub fn launch(exe: &Path, args: Vec<String>) -> Result<()> {
    // Both absolute before anything joins a namespace: `setns` resets
    // the working directory, so a relative path settled later is
    // resolved against the mount's root instead of the user's.
    let (exe, cwd) = launch::target(exe)?;
    let e = launch::resolve(&exe)?;
    // The kernel invokes this with no terminal of its own, so when a
    // double-clicked program misbehaves there is nothing to look at.
    // RAVEN_TRACE=1 says which environment was chosen and why it
    // mattered - the one question worth asking first.
    if std::env::var_os("RAVEN_TRACE").is_some() {
        eprintln!(
            "[raven] exe={} environment={} prefix={}",
            exe.display(),
            e.name,
            e.prefix().display()
        );
    }
    let mut argv = vec!["wine".to_string(), exe.display().to_string()];
    argv.extend(args);
    // The program's own directory, as Windows would give it.
    run(&e.name, argv, Some(cwd))
}

pub fn run(name: &str, argv: Vec<String>, cwd: Option<PathBuf>) -> Result<()> {
    let e = env::Environment::open(name)?;
    // Join the environment's session rather than building a world of our own.
    // The first launch of the day starts the anchor and pays for the mount;
    // every later one lands in that namespace, and one made within seconds of
    // the last finds a warm wineserver in it, which is where plain Wine's
    // thirteenfold advantage came from.
    let anchor = e.ensure_session()?;
    e.join_session(anchor)?;

    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    cmd.env("WINEPREFIX", e.prefix());
    // Tested inside the namespace, which is the only place the answer is
    // true, and skipped rather than fatal: a directory that is not there is
    // no reason to refuse to start the program. Setting it anyway makes
    // `exec` fail with an ENOENT that reads as "wine is missing".
    if let Some(d) = cwd.filter(|d| d.is_dir()) {
        cmd.current_dir(d);
    }
    Err(cmd.exec()).with_context(|| format!("could not run {}", argv[0]))
}

pub fn exec(
    spec: OverlaySpec,
    argv: Vec<String>,
    wineprefix: Option<PathBuf>,
    cwd: Option<PathBuf>,
) -> Result<()> {
    if !UserNsOverlay::is_available() {
        bail!("this kernel restricts unprivileged user namespaces; run `raven doctor`");
    }

    UserNsOverlay
        .mount(&spec)
        .context("could not mount the overlay")?;

    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    if let Some(p) = wineprefix {
        cmd.env("WINEPREFIX", p);
    }
    // Windows' shell starts a program in its own directory, and a great many
    // programs depend on that to find the data beside them - a game's Data/,
    // its .ini files. A double-clicked .exe reaches us with the file manager's
    // directory instead, so without this the program looks for its own files
    // in the user's home and fails for a reason that has nothing to do with
    // Raven.
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    // Replaces this process, so the mount lives exactly as long as the program.
    Err(cmd.exec()).with_context(|| format!("could not run {}", argv[0]))
}
