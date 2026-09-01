//! The Raven command-line interface.
//!
//! A shell over the library, holding no logic of its own. Anything that decides
//! something belongs in `raven::`, so a graphical front end calls the same code
//! rather than reimplementing it.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Prints a line, treating a closed pipe as a normal end.
///
/// Rust ignores `SIGPIPE`, so a plain `println!` into `head` or `less` fails its
/// write and *panics* — a backtrace where every other Unix tool simply stops.
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
use clap::{Args, Parser, Subcommand};
use raven::mount::{MountBackend, OverlaySpec, UserNsOverlay};
use raven::{base, env, launch};

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
    /// Install, remove or report DXVK in an environment.
    ///
    /// Raven fetches nothing: point `--from` at a DXVK build you already have,
    /// the way `base deploy` takes an ISO you already have. With no flag, this
    /// reports what is installed.
    Dxvk {
        name: String,
        /// An extracted DXVK release, or a release archive of one.
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

fn main() -> Result<()> {
    // binfmt_misc hands its interpreter a path where clap expects a verb, and
    // offers no way to insert one. Reading the magic bytes settles which it is
    // without guessing: `raven doctor` is not a file, and `./game.exe` is not a
    // subcommand.
    let mut args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if let Some(first) = args.get(1) {
        if launch::looks_like_pe(Path::new(first)) {
            args.insert(1, std::ffi::OsString::from("launch"));
        }
    }

    match Cli::parse_from(args).command {
        Commands::Doctor => doctor(),
        Commands::Base(c) => base_cmd(c),
        Commands::Env(c) => env_cmd(c),
        Commands::Run { name, argv } => run(&name, argv, None),
        Commands::Launch { exe, args } => {
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
            let cwd = exe
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(PathBuf::from);
            run(&e.name, argv, cwd)
        }
        Commands::Binfmt => binfmt(),
        Commands::SessionAnchor { name } => session_anchor(&name),
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
            None,
        ),
    }
}

fn doctor() -> Result<()> {
    let userns = UserNsOverlay::is_available();
    out!(
        "unprivileged user namespaces : {}",
        if userns { "yes" } else { "no" }
    );
    if !userns {
        out!(
            "  This kernel restricts them, and Raven's only implemented mount\n  \
             backend needs them. linux-hardened and Ubuntu's AppArmor policy\n  \
             both do this."
        );
    }
    out!(
        "wine                         : {}",
        if raven::prefix::wine_available() {
            "found"
        } else {
            "MISSING - Raven cannot run anything without it"
        }
    );
    out!(
        "ntsync                       : {}",
        if PathBuf::from("/dev/ntsync").exists() {
            "present"
        } else {
            "absent - Wine falls back to wineserver for NT synchronization"
        }
    );
    out!(
        "bases                        : {}",
        base::Base::list()?.len()
    );
    out!(
        "environments                 : {}",
        env::Environment::list()?.len()
    );
    report_exe_handlers();
    Ok(())
}

/// Who actually gets a double-clicked `.exe`.
///
/// Wine registers a handler for the same `MZ` magic, the kernel picks the most
/// recently registered one silently, and losing that race looks like Raven
/// losing its prefix. It cost an hour once; this is the hour, written down.
fn report_exe_handlers() {
    const LABEL: &str = ".exe handler                 ";
    if !Path::new("/proc/sys/fs/binfmt_misc").exists() {
        out!("{LABEL}: binfmt_misc is not mounted - a double-clicked .exe cannot run");
        return;
    }
    let handlers = launch::exe_handlers();
    let Some(winner) = handlers.iter().find(|h| h.enabled) else {
        if handlers.is_empty() {
            out!("{LABEL}: none - a double-clicked .exe will not run; see `raven binfmt`");
        } else {
            // Disabled is not absent: the registration exists, someone turned
            // it off, and re-registering as root is the wrong fix.
            out!("{LABEL}: all disabled - a double-clicked .exe will not run");
            for h in &handlers {
                out!("    {:10} disabled  {}", h.name, h.interpreter.display());
            }
            out!("  Re-enable one: echo 1 | sudo tee /proc/sys/fs/binfmt_misc/<name>");
        }
        return;
    };

    let raven_wins = winner.name == launch::BINFMT_NAME;
    out!(
        "{LABEL}: {} -> {}{}",
        winner.name,
        winner.interpreter.display(),
        if raven_wins { "" } else { "  (NOT Raven)" }
    );

    // Every claimant, when there is more than one: the kernel's choice is
    // silent, so the losers must not be.
    if handlers.len() > 1 {
        for h in &handlers {
            out!(
                "    {:10} {:8} {}{}",
                h.name,
                if h.enabled { "enabled" } else { "disabled" },
                h.interpreter.display(),
                if std::ptr::eq(h, winner) {
                    "  <- wins"
                } else {
                    ""
                }
            );
        }
    }

    if !raven_wins {
        out!(
            "  Every double-clicked .exe runs through {}, and the failure looks\n  \
             like Raven losing its prefix. If that is Wine's registration, mask it:\n    \
             echo -n | sudo tee /etc/binfmt.d/wine.conf\n    \
             sudo systemctl restart systemd-binfmt",
            winner.name
        );
    } else if handlers.iter().any(|h| h.enabled && h.name != winner.name) {
        out!(
            "  Another handler matches, and whichever registers LAST wins - an\n  \
             update to its package can silently take every .exe back. Mask it\n  \
             (for Wine: an empty /etc/binfmt.d/wine.conf, then restart\n  \
             systemd-binfmt)."
        );
    }

    if !winner.interpreter.exists() {
        if winner.held_open {
            out!(
                "  Its interpreter no longer exists. The registration survives on the\n  \
                 kernel's open handle until reboot - then every .exe stops running.\n  \
                 Re-register: see `raven binfmt`."
            );
        } else {
            out!(
                "  Its interpreter no longer exists, so every double-clicked .exe\n  \
                 fails right now. Re-register: see `raven binfmt`."
            );
        }
    }
}

fn base_cmd(cmd: BaseCmd) -> Result<()> {
    match cmd {
        BaseCmd::List => {
            let bases = base::Base::list()?;
            if bases.is_empty() {
                out!("No bases. Deploy one with `raven base deploy`.");
            }
            for b in bases {
                let ok = if b.looks_like_windows() {
                    ""
                } else {
                    "  (does not look like Windows)"
                };
                out!("{}{ok}", b.id);
            }
            Ok(())
        }
        BaseCmd::Editions(a) => {
            for e in base::editions(&a.image)? {
                let build = e.build.as_deref().unwrap_or("-");
                out!("{:>3}  {}  (build {build})", e.index, e.name);
            }
            Ok(())
        }
        BaseCmd::Deploy {
            image,
            edition,
            name,
        } => {
            out!("Applying edition {edition}. This writes tens of gigabytes and takes a while.");
            let b = base::deploy(&image.image, edition, &name)?;
            out!("Deployed {} to {}", b.id, b.path.display());
            Ok(())
        }
    }
}

fn env_cmd(cmd: EnvCmd) -> Result<()> {
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
                    out!("  {:>7}  {}", h.pid, h.comm);
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
        EnvCmd::Dxvk { name, from, remove } => {
            let e = env::Environment::open(&name)?;
            if let Some(src) = from {
                let done = e.install_dxvk(&src)?;
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
                    "Installed {} DXVK {word} into {name}: {}",
                    done.len(),
                    dlls.join(", ")
                );
                out!("They shadow the real Windows through the overlay; the base is untouched.");
                out!("Undo it: raven env dxvk {name} --remove");
            } else if remove {
                let gone = e.remove_dxvk()?;
                let word = if gone == 1 { "library" } else { "libraries" };
                out!("Removed {gone} DXVK {word} from {name}; the real Windows is uncovered.");
            } else {
                let files = e.dxvk();
                if files.is_empty() {
                    out!("{name}: no DXVK installed");
                } else {
                    out!(
                        "{name}: {}",
                        e.dxvk_build()
                            .unwrap_or_else(|| "DXVK, build unrecorded".into())
                    );
                    for f in &files {
                        out!("  {:<10} {}", f.dll, f.arch);
                    }
                    let over = e.dxvk_overrides();
                    // The two halves are reported apart on purpose: a file with
                    // no override is a library Wine will ignore.
                    for f in &files {
                        if !over.iter().any(|(n, _)| n == &f.dll) {
                            out!("warning: {} is installed but has no DLL override", f.dll);
                        }
                    }
                }
            }
            Ok(())
        }
        EnvCmd::Detach { name, letter } => {
            let e = env::Environment::open(&name)?;
            e.detach(letter)?;
            out!("Detached {letter}: from {name}. The device itself is untouched.");
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

fn binfmt() -> Result<()> {
    let me = std::env::current_exe().context("could not find this binary")?;
    out!(
        "registration : {}",
        if launch::registered() {
            "active"
        } else {
            "not registered - .exe files will not run on their own"
        }
    );
    out!(
        "mounted      : {}",
        Path::new("/proc/sys/fs/binfmt_misc").exists()
    );
    report_exe_handlers();
    out!();
    out!("Registering needs root, once. It belongs to the package rather than");
    out!("to Raven, so that nothing has to hold privilege while Raven runs:");
    out!();
    out!(
        "  echo '{}' | sudo tee {}",
        launch::binfmt_line(&me),
        launch::conf_path().display()
    );
    out!("  sudo systemctl restart systemd-binfmt");
    out!();
    out!("The interpreter path above is this binary. A package would name its");
    out!("installed location instead.");
    Ok(())
}

fn run(name: &str, argv: Vec<String>, cwd: Option<PathBuf>) -> Result<()> {
    let e = env::Environment::open(name)?;
    // Join the environment's session rather than building a world of our own.
    // The first launch of the day starts the anchor and pays for the mount;
    // every later one lands in a namespace that already has a warm wineserver
    // in it, which is where plain Wine's thirteenfold advantage came from.
    let anchor = e.ensure_session()?;
    e.join_session(anchor)?;

    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    cmd.env("WINEPREFIX", e.prefix());
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    Err(cmd.exec()).with_context(|| format!("could not run {}", argv[0]))
}

/// Holds a namespace open so later launches can join it.
///
/// Mounts, reports itself, then does nothing for as long as it is wanted. It
/// must stay single-threaded until the mount is done - the kernel refuses
/// `CLONE_NEWUSER` to a threaded process - which is why the readiness line is
/// written only afterwards.
fn session_anchor(name: &str) -> Result<()> {
    use std::io::Write as _;
    let e = env::Environment::open(name)?;
    let spec = e.spec()?;
    std::fs::create_dir_all(&spec.target)
        .with_context(|| format!("could not create the mount point {}", spec.target.display()))?;
    if !UserNsOverlay::is_available() {
        println!("this kernel restricts unprivileged user namespaces; run `raven doctor`");
        let _ = std::io::stdout().flush();
        std::process::exit(1);
    }
    if let Err(err) = UserNsOverlay.mount(&spec) {
        println!("could not mount the overlay: {err}");
        let _ = std::io::stdout().flush();
        std::process::exit(1);
    }
    let pid = std::process::id();
    // Written only after the mount exists, so a reader of this file never sees
    // a session that cannot be joined.
    std::fs::write(e.session_file(), format!("{pid}\n")).with_context(|| {
        format!(
            "could not record the session at {}",
            e.session_file().display()
        )
    })?;
    println!("ready {pid}");
    let _ = std::io::stdout().flush();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

fn exec(
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
