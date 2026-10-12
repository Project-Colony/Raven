//! The command line: what `raven` accepts, and which handler answers it.
//!
//! A shell over the library, holding no logic of its own. Anything that decides
//! something belongs in `raven::`, so a graphical front end calls the same code
//! rather than reimplementing it.
//!
//! The definitions below carry no help text. Every summary and argument line
//! comes from `raven::i18n` - `help_<command path>` and
//! `help_<command path>_<argument>` - so `--help` speaks the user's language,
//! and a test fails if a command or argument has no words in either.

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
use clap::{Args, Command, CommandFactory, FromArgMatches, Parser, Subcommand};
use raven::i18n::{self, Lang};
use raven::launch;
use raven::mount::OverlaySpec;

#[derive(Parser)]
#[command(name = "raven", version)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Doctor,
    #[command(subcommand)]
    Base(BaseCmd),
    #[command(subcommand)]
    Env(EnvCmd),
    // What `binfmt_misc` invokes for a `.exe`, and what makes `./program.exe`
    // work.
    Launch {
        exe: PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Binfmt,
    // Holds an environment's namespace open. Started by Raven, not by hand.
    #[command(hide = true)]
    SessionAnchor {
        name: String,
    },
    Run {
        name: String,
        #[arg(last = true, required = true)]
        argv: Vec<String>,
    },
    // The primitive `run` is built on.
    Exec {
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
    List,
    Editions(ImageArg),
    Deploy {
        #[command(flatten)]
        image: ImageArg,
        #[arg(long)]
        edition: u32,
        #[arg(long)]
        name: String,
    },
}

#[derive(Args)]
struct ImageArg {
    #[arg(long)]
    image: PathBuf,
}

#[derive(Subcommand)]
enum EnvCmd {
    List,
    Create {
        name: String,
        #[arg(long)]
        base: String,
    },
    Destroy {
        name: String,
    },
    Status {
        name: String,
    },
    Stop {
        name: String,
    },
    Start {
        name: String,
    },
    Attach {
        name: String,
        device: PathBuf,
        #[arg(long, default_value = "d")]
        letter: char,
    },
    Dxvk {
        name: String,
        #[arg(long, value_name = "PATH", conflicts_with = "remove")]
        from: Option<PathBuf>,
        #[arg(long)]
        remove: bool,
    },
    Vkd3d {
        name: String,
        #[arg(long, value_name = "PATH", conflicts_with = "remove")]
        from: Option<PathBuf>,
        #[arg(long)]
        remove: bool,
    },
    Detach {
        name: String,
        #[arg(long, default_value = "d")]
        letter: char,
    },
    Reproject {
        name: String,
    },
    Default {
        name: Option<String>,
    },
}

/// The whole command tree, worded in `lang`.
///
/// A command's summary is `help_<path>`, the text `--help` adds is
/// `help_<path>_long`, and an argument's line is `help_<path>_<argument>`, or
/// `help_arg_<argument>` where several commands share one meaning. Those are
/// set before the tree is built, because building is when clap decides
/// whether `--help` has more to say than `-h`; clap's own `--help`,
/// `--version` and `help` only exist after it, so they are worded then.
pub fn command(lang: Lang) -> Command {
    let mut cmd = word(Cli::command(), "help_raven", lang);
    cmd.build();
    word_clap(cmd, lang)
}

fn word(mut cmd: Command, key: &str, lang: Lang) -> Command {
    let get = |k: &str| i18n::get_in(lang, k);
    if let Some(about) = get(key) {
        cmd = cmd.about(about);
    }
    if let Some(long) = get(&format!("{key}_long")) {
        cmd = cmd.long_about(long);
    }
    cmd.mut_args(|arg| {
        let id = arg.get_id().as_str().to_owned();
        let arg = match get(&format!("{key}_{id}")).or_else(|| get(&format!("help_arg_{id}"))) {
            Some(help) => arg.help(help),
            None => arg,
        };
        let defaults: Vec<_> = arg
            .get_default_values()
            .iter()
            .map(|v| v.to_string_lossy().into_owned())
            .collect();
        if lang == Lang::En || defaults.is_empty() {
            return arg;
        }
        // clap writes "[default: d]" itself, in English; this is the same
        // note, in the help line instead.
        let help = arg.get_help().map(ToString::to_string).unwrap_or_default();
        let note = get("clap_default")
            .unwrap_or("[default: {value}]")
            .replace("{value}", &defaults.join(", "));
        arg.hide_default_value(true).help(format!("{help} {note}"))
    })
    .mut_subcommands(|sub| {
        let path = match key {
            "help_raven" => format!("help_{}", sub.get_name()),
            parent => format!("{parent}_{}", sub.get_name()),
        };
        word(sub, &path, lang)
    })
}

fn word_clap(mut cmd: Command, lang: Lang) -> Command {
    let get = |k: &str| i18n::get_in(lang, k);
    cmd = cmd
        .subcommand_help_heading(get("clap_commands"))
        .mut_args(|arg| match arg.get_id().as_str() {
            // The two wordings clap itself picks between.
            "help" if arg.get_long_help().is_some() => arg
                .help(get("clap_help_more"))
                .long_help(get("clap_help_summary")),
            "help" => arg.help(get("clap_help")),
            "version" => arg.help(get("clap_version")),
            _ => arg,
        });
    if lang != Lang::En {
        // clap hard-codes "Usage:", and its template is the only way to say it
        // differently. English keeps clap's own template.
        let style = cmd.get_styles().get_usage();
        let usage = format!(
            "{}{}{}",
            style.render(),
            get("clap_usage").unwrap_or("Usage:"),
            style.render_reset()
        );
        let has_more = cmd.get_arguments().any(|a| !a.is_hide_set()) || cmd.has_subcommands();
        cmd = cmd.help_template(if has_more {
            format!("{{before-help}}{{about-with-newline}}\n{usage} {{usage}}\n\n{{all-args}}{{after-help}}")
        } else {
            format!("{{before-help}}{{about-with-newline}}\n{usage} {{usage}}{{after-help}}")
        });
    }
    cmd.mut_subcommands(|sub| match sub.get_name() {
        "help" => sub.about(get("clap_help_command")),
        _ => word_clap(sub, lang),
    })
}

/// Parses this process's command line, in the language already set.
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
    let matches = command(i18n::lang()).get_matches_from(args);
    Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit())
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

/// A failure as the user reads it: the message, then what caused it, each in
/// the current language where it is one of Raven's own.
pub fn failure(error: &anyhow::Error) -> String {
    let mut chain = error.chain();
    let mut out = format!(
        "{} {}",
        i18n::t("cli_error"),
        chain.next().map(i18n::describe).unwrap_or_default()
    );
    let causes: Vec<String> = chain.map(i18n::describe).collect();
    if !causes.is_empty() {
        out.push_str(&format!("\n\n{}", i18n::t("cli_caused_by")));
        for cause in causes {
            out.push_str(&format!("\n    {cause}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every command and argument a user can see, with its path.
    fn visible(cmd: &Command, path: &str, out: &mut Vec<(String, Option<String>)>) {
        out.push((path.to_owned(), cmd.get_about().map(|s| s.to_string())));
        for arg in cmd.get_arguments().filter(|a| !a.is_hide_set()) {
            out.push((
                format!("{path} {}", arg.get_id()),
                arg.get_help().map(|s| s.to_string()),
            ));
        }
        for sub in cmd.get_subcommands().filter(|s| !s.is_hide_set()) {
            if sub.get_name() != "help" {
                visible(sub, &format!("{path} {}", sub.get_name()), out);
            }
        }
    }

    #[test]
    fn every_command_and_argument_has_words_in_both_languages() {
        for lang in [Lang::En, Lang::Fr] {
            let mut all = Vec::new();
            visible(&command(lang), "raven", &mut all);
            for (what, help) in all {
                assert!(
                    help.is_some_and(|h| !h.is_empty()),
                    "`{what}` has no help in {lang:?}"
                );
            }
        }
    }

    #[test]
    fn the_command_tree_is_valid_in_both_languages() {
        for lang in [Lang::En, Lang::Fr] {
            command(lang).debug_assert();
        }
    }

    #[test]
    fn a_failure_reads_in_the_current_language() {
        let e = anyhow::Error::new(raven::Error::NoSuchBase("w11".into()))
            .context(i18n::tf("could_not_run", &[("program", &"wine")]));
        let text = failure(&e);
        assert!(text.contains("w11"), "{text}");
        assert!(text.lines().count() >= 3, "the cause is shown: {text}");
    }
}
