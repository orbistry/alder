use std::path::PathBuf;

use alder_bundle::EntryKind;
use alder_config::Target;
use alder_driver::BuildMode;
use miette::{Result, miette};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// Path to the standalone project
    #[arg(default_value = ".")]
    pub path: PathBuf,
    /// Arguments exposed through `Cli.args()`
    #[arg(last = true)]
    pub args: Vec<String>,
}

impl Args {
    pub async fn exec(self) -> Result<()> {
        let compiled = super::build::compile(&self.path, BuildMode::Build).await?;
        if compiled.target != Target::Standalone {
            return Err(miette!("alder run requires target: standalone"));
        }
        let bundle = super::build::bundle(&compiled.result, EntryKind::Standalone).await?;
        let code = alder_runtime::execute(bundle, self.args)
            .await
            .map_err(|error| miette!(error.to_string()))?;
        if code != 0 {
            return Err(miette!("program exited with code {code}"));
        }
        Ok(())
    }
}
