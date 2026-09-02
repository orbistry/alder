use std::path::PathBuf;

use alder_bundle::EntryKind;
use alder_driver::BuildMode;
use miette::{Result, miette};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// Path to the project
    #[arg(default_value = ".")]
    pub path: PathBuf,
}

impl Args {
    pub async fn exec(self) -> Result<()> {
        let compiled = super::build::compile(&self.path, BuildMode::Test).await?;
        let bundle = super::build::bundle(&compiled.result, EntryKind::Test).await?;
        let code = alder_runtime::execute(bundle, Vec::new())
            .await
            .map_err(|error| miette!(error.to_string()))?;
        if code != 0 {
            return Err(miette!("tests failed"));
        }
        Ok(())
    }
}
