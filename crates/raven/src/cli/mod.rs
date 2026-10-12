//! The command line: what `raven` accepts, and which handler answers it.
//!
//! A shell over the library, holding no logic of its own. Anything that decides
//! something belongs in `raven::`, so a graphical front end calls the same code
//! rather than reimplementing it.

/// Prints a line, treating a closed pipe as a normal end.
///
/// Rust ignores `SIGPIPE`, so a plain `println!` into `head` or `less` fails its
/// write and *panics* - a backtrace where every other Unix tool simply stops.
/// A closed pipe is the reader saying "enough", which is an ordinary way for a
/// command to finish, not a failure to report.
macro_rules! out {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        if writeln!(std::io::stdout(), $($arg)*).is_err() {
            std::process::exit(0);
        }
    }};
}

mod base;
mod doctor;
mod env;
mod run;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use raven::launch;
use raven::mount::OverlaySpec;

#[derive(Parser)]
#[command(name = "raven", version, about, long_about = None)]
pub struct Cli {
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

    /// Run a Windows program, resolving its environment automatically.
    ///
    /// This is what `binfmt_misc` invokes for a `.exe`, and what makes
    /// `./program.exe` work. It can be run by hand too.
    Launch {
        /// The program. `binfmt_misc` passes this as the first argument.
        exe: PathBuf,
        /// Arguments for the program.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Show how `.exe` files are registered with the kernel.
    Binfmt,

    /// Hold an environment's namespace open. Started by Raven, not by hand.
    #[command(hide = true)]
    SessionAnchor { name: String },

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
    /// Report whether an environment is running and what holds it.
    Status { name: String },
    /// Release an environment: terminate every process holding its mount.
    Stop { name: String },
    /// Bring an environment up: mount it and start Wine's services.
    ///
    /// Mounting costs 0.04s and lasts until `env stop`. Wine's services cost
    /// about 1.7s and exit a few seconds after the last program does, so only
    /// a launch made within those seconds skips them.
    Start { name: String },
    /// Attach a block device to an environment as a raw drive.
    ///
    /// Dangerous by design: a program in the environment can then read and
    /// WRITE the device's sectors directly. Tools that discover disks through
    /// Windows enumeration still will not see it - the docs say why.
    Attach {
        name: String,
        /// The unix block device, e.g. /dev/sdc.
        device: PathBuf,
        /// The drive letter, d through z.
        #[arg(long, default_value = "d")]
        letter: char,
    },
    /// Install, remove or report DXVK - Direct3D 8 through 11 on Vulkan.
    ///
    /// Raven fetches nothing: point `--from` at a build you already have, the
    /// way `base deploy` takes an ISO you already have. With no flag, this
    /// reports what is installed.
    Dxvk {
        name: String,
        /// An extracted release, or a release archive of one.
        #[arg(long, value_name = "PATH", conflicts_with = "remove")]
        from: Option<PathBuf>,
        /// Uncover the real Windows again by deleting what was installed.
        #[arg(long)]
        remove: bool,
    },

    /// Install, remove or report vkd3d-proton - Direct3D 12 on Vulkan.
    ///
    /// A separate runtime from DXVK, not a newer one: DXVK does not implement
    /// Direct3D 12 at all. They install side by side.
    Vkd3d {
        name: String,
        /// An extracted release, or a release archive of one.
        #[arg(long, value_name = "PATH", conflicts_with = "remove")]
        from: Option<PathBuf>,
        /// Uncover the real Windows again by deleting what was installed.
        #[arg(long)]
        remove: bool,
    },

    /// Detach a previously attached device. The device itself is untouched.
    Detach {
        name: String,
        #[arg(long, default_value = "d")]
        letter: char,
    },
    /// Re-run the registry projection, after editing the environment's rules.
    Reproject { name: String },
    /// Set the environment used for programs that are not inside one.
    Default { name: Option<String> },
}

/// Parses this process's command line.
pub fn parse() -> Cli {
    // binfmt_misc hands its interpreter a path where clap expects a verb, and
    // offers no way to insert one. Reading the magic bytes settles which it is
    // without guessing: `raven doctor` is not a file, and `./game.exe` is not a
    // subcommand.
    let mut args: Vec<OsString> = std::env::args_os().collect();
    if let Some(first) = args.get(1)
        && launch::looks_like_pe(Path::new(first))
    {
        args.insert(1, OsString::from("launch"));
    }
    Cli::parse_from(args)
}

/// Runs the command `cli` names.
pub fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Doctor => doctor::doctor(),
        Commands::Base(c) => base::base_cmd(c),
        Commands::Env(c) => env::env_cmd(c),
        Commands::Run { name, argv } => run::run(&name, argv, None),
        Commands::Launch { exe, args } => run::launch(&exe, args),
        Commands::Binfmt => doctor::binfmt(),
        Commands::SessionAnchor { name } => raven::session::anchor(&name),
        Commands::Exec {
            lower,
            upper,
            work,
            target,
            argv,
        } => run::exec(
            OverlaySpec {
                lower,
                upper,
                work,
                target,
            },
            argv,
            None,
            None,
        ),
    }
}
