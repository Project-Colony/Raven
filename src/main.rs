//! The Raven command-line interface.
//!
//! This is a shell over the library and holds no logic of its own. Anything that
//! decides something belongs in `raven::`, so that a graphical front end can call
//! the same code instead of reimplementing it.

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use raven::mount::{MountBackend, OverlaySpec, UserNsOverlay};

#[derive(Parser)]
#[command(name = "raven", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Report what this system can and cannot do.
    Doctor,

    /// Mount an overlay over a base and run a command inside it.
    ///
    /// The mount lives only for this process tree, so it needs no cleanup and
    /// leaves nothing behind if the program crashes.
    Exec {
        #[arg(long)]
        base: PathBuf,
        #[arg(long)]
        upper: PathBuf,
        #[arg(long)]
        work: PathBuf,
        #[arg(long)]
        target: PathBuf,
        /// The command to run once C: is mounted.
        #[arg(last = true, required = true)]
        argv: Vec<String>,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Commands::Doctor => doctor(),
        Commands::Exec {
            base,
            upper,
            work,
            target,
            argv,
        } => exec(
            OverlaySpec {
                base,
                upper,
                work,
                target,
            },
            argv,
        ),
    }
}

fn doctor() -> Result<()> {
    let userns = UserNsOverlay::is_available();
    println!(
        "unprivileged user namespaces : {}",
        if userns { "yes" } else { "no" }
    );
    if !userns {
        println!(
            "  This kernel restricts them. Raven's only implemented mount backend\n\
               needs them. linux-hardened and Ubuntu's AppArmor policy both do this."
        );
    }
    println!(
        "ntsync                       : {}",
        if PathBuf::from("/dev/ntsync").exists() {
            "present"
        } else {
            "absent — Wine falls back to wineserver for NT synchronization"
        }
    );
    Ok(())
}

fn exec(spec: OverlaySpec, argv: Vec<String>) -> Result<()> {
    if !UserNsOverlay::is_available() {
        bail!("this kernel restricts unprivileged user namespaces; run `raven doctor`");
    }

    UserNsOverlay
        .mount(&spec)
        .context("could not mount the overlay")?;

    // Replaces this process, so the mount lives exactly as long as the program.
    Err(Command::new(&argv[0]).args(&argv[1..]).exec())
        .with_context(|| format!("could not run {}", argv[0]))
}
