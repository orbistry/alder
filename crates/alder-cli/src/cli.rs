use clap::{CommandFactory, FromArgMatches, Parser};

use crate::cmd;

#[derive(Parser)]
#[command(name = "alder", version, about = "The Alder programming language compiler", long_about = Some(crate::BANNER))]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(flatten)]
    pub reporting: crate::reporting::Options,
    #[command(subcommand)]
    pub cmd: cmd::Cmd,
}

impl Default for Cli {
    fn default() -> Self {
        let options = crate::reporting::Options::from_startup(std::env::args_os().skip(1));

        let color = match options.color {
            crate::reporting::Color::Auto => clap::ColorChoice::Auto,
            crate::reporting::Color::Always => clap::ColorChoice::Always,
            crate::reporting::Color::Never => clap::ColorChoice::Never,
        };

        Self::from_arg_matches(&Self::command().color(color).get_matches())
            .unwrap_or_else(|error| error.exit())
    }
}

impl Cli {
    pub async fn exec(self) -> miette::Result<()> {
        self.cmd
            .exec(crate::reporting::Output::stderr(self.reporting))
            .await
    }
}
