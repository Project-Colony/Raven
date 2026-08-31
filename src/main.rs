//! The Raven command-line interface.
//!
//! A shell over the library, holding no logic of its own. Anything that decides
//! something belongs in `raven::`, so a graphical front end calls the same code
//! rather than reimplementing it.

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use raven::mount::{MountBackend, OverlaySpec, UserNsOverlay};
use raven::{base, env};

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

    /// Deployed Windows installations, shared by every environment.
    #[command(subcommand)]
    Base(BaseCmd),

    /// Environments: a Wine layer over a base, plus somewhere to write.
    #[command(subcommand)]
    Env(EnvCmd),

    /// Run a program inside an environment.
    Run {
        /// The environment to run in.
        name: String,
        /// The command, after `--`. A bare `wine <program.exe>` is the usual one.
        #[arg(last = true, required = true)]
        argv: Vec<String>,
    },

    /// Mount a layer stack and run a command, without an environment.
    ///
    /// The primitive `run` is built on. Useful for testing a stack before
    /// committing it to an environment.
    Exec {
        /// A read-only layer. Repeat it; the FIRST wins where layers overlap.
        #[arg(long = "lower", required = true)]
        lower: Vec<PathBuf>,
        #[arg(long)]
        upper: PathBuf,
        #[arg(long)]
        work: PathBuf,
        #[arg(long)]
        target: PathBuf,
        #[arg(last = true, required = true)]
        argv: Vec<String>,
    },
}

#[derive(Subcommand)]
enum BaseCmd {
    /// List the deployed bases.
    List,
    /// List the editions inside an installation image.
    Editions(ImageArg),
    /// Apply one edition of an image into a new base.
    Deploy {
        #[command(flatten)]
        image: ImageArg,
        /// The edition index, from `raven base editions`.
        #[arg(long)]
        edition: u32,
        /// What to call the base. Environments refer to it by this name.
        #[arg(long)]
        name: String,
    },
}

#[derive(Args)]
struct ImageArg {
    /// Path to `install.wim`, extracted from an official ISO.
    #[arg(long)]
    image: PathBuf,
}

#[derive(Subcommand)]
enum EnvCmd {
    /// List the environments.
    List,
    /// Build an environment against a base.
    Create {
        name: String,
        /// The base to run against, from `raven base list`.
        #[arg(long)]
        base: String,
    },
    /// Delete an environment. The base it ran against is untouched.
    Destroy { name: String },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Commands::Doctor => doctor(),
        Commands::Base(c) => base_cmd(c),
        Commands::Env(c) => env_cmd(c),
        Commands::Run { name, argv } => run(&name, argv),
        Commands::Exec {
            lower,
            upper,
            work,
            target,
            argv,
        } => exec(
            OverlaySpec {
                lower,
                upper,
                work,
                target,
            },
            argv,
            None,
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
            "  This kernel restricts them, and Raven's only implemented mount\n  \
             backend needs them. linux-hardened and Ubuntu's AppArmor policy\n  \
             both do this."
        );
    }
    println!(
        "wine                         : {}",
        if raven::prefix::wine_available() {
            "found"
        } else {
            "MISSING - Raven cannot run anything without it"
        }
    );
    println!(
        "ntsync                       : {}",
        if PathBuf::from("/dev/ntsync").exists() {
            "present"
        } else {
            "absent - Wine falls back to wineserver for NT synchronization"
        }
    );
    println!(
        "bases                        : {}",
        base::Base::list()?.len()
    );
    println!(
        "environments                 : {}",
        env::Environment::list()?.len()
    );
    Ok(())
}

fn base_cmd(cmd: BaseCmd) -> Result<()> {
    match cmd {
        BaseCmd::List => {
            let bases = base::Base::list()?;
            if bases.is_empty() {
                println!("No bases. Deploy one with `raven base deploy`.");
            }
            for b in bases {
                let ok = if b.looks_like_windows() {
                    ""
                } else {
                    "  (does not look like Windows)"
                };
                println!("{}{ok}", b.id);
            }
            Ok(())
        }
        BaseCmd::Editions(a) => {
            for e in base::editions(&a.image)? {
                let build = e.build.as_deref().unwrap_or("-");
                println!("{:>3}  {}  (build {build})", e.index, e.name);
            }
            Ok(())
        }
        BaseCmd::Deploy {
            image,
            edition,
            name,
        } => {
            println!(
                "Applying edition {edition}. This writes tens of gigabytes and takes a while."
            );
            let b = base::deploy(&image.image, edition, &name)?;
            println!("Deployed {} to {}", b.id, b.path.display());
            Ok(())
        }
    }
}

fn env_cmd(cmd: EnvCmd) -> Result<()> {
    match cmd {
        EnvCmd::List => {
            let envs = env::Environment::list()?;
            if envs.is_empty() {
                println!("No environments. Create one with `raven env create`.");
            }
            for e in envs {
                println!("{}  (base {})", e.name, e.manifest.base);
            }
            Ok(())
        }
        EnvCmd::Create { name, base } => {
            let e = env::create(&name, &base)?;
            println!("Created {} against base {}", e.name, e.manifest.base);
            println!(
                "Run something with: raven run {} -- wine <program.exe>",
                e.name
            );
            Ok(())
        }
        EnvCmd::Destroy { name } => {
            env::Environment::destroy(&name)?;
            println!("Destroyed {name}. The base is untouched.");
            Ok(())
        }
    }
}

fn run(name: &str, argv: Vec<String>) -> Result<()> {
    let e = env::Environment::open(name)?;
    let spec = e.spec()?;
    std::fs::create_dir_all(&spec.target)
        .with_context(|| format!("could not create the mount point {}", spec.target.display()))?;
    exec(spec, argv, Some(e.prefix()))
}

fn exec(spec: OverlaySpec, argv: Vec<String>, wineprefix: Option<PathBuf>) -> Result<()> {
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
    // Replaces this process, so the mount lives exactly as long as the program.
    Err(cmd.exec()).with_context(|| format!("could not run {}", argv[0]))
}
