//! The Raven command-line interface. The commands live in `cli/`; this sets
//! the language, parses the command line and hands it over.

mod cli;

use std::process::ExitCode;

fn main() -> ExitCode {
    raven::i18n::set(raven::i18n::Lang::from_env());
    match cli::dispatch(cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}", cli::failure(&e));
            ExitCode::FAILURE
        }
    }
}
