//! The Raven command-line interface. The commands live in `cli/`; this only
//! parses the command line and hands it over.

mod cli;

fn main() -> anyhow::Result<()> {
    cli::dispatch(cli::parse())
}
