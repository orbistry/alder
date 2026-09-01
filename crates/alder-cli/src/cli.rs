use clap::Parser;

use crate::cmd;

#[derive(Parser)]
#[command(name = "alder", version, about = "The Alder programming language compiler", long_about = Some(crate::BANNER))]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: cmd::Cmd,
}

impl Default for Cli {
    fn default() -> Self {
        Self::parse()
    }
}

impl Cli {
    pub async fn exec(self) -> miette::Result<()> {
        self.cmd.exec().await
    }
}
