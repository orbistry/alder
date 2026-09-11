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
    pub(super) async fn exec(self, output: &crate::reporting::Output) -> Result<()> {
        let started = std::time::Instant::now();
        let compiled = super::build::compile(&self.path, BuildMode::Build, output).await?;
        if compiled.target != Target::Standalone {
            return Err(miette!("alder run requires target: standalone"));
        }
        output.stage("bundling");
        output.status("Bundling", crate::reporting::display_path(&compiled.root));
        let bundle = super::build::bundle(&compiled, EntryKind::Standalone).await?;
        output.finish("build", started.elapsed());
        output.stage("runtime");
        output.status("Running", crate::reporting::display_path(&compiled.root));
        let code = alder_runtime::execute(bundle, self.args)
            .await
            .map_err(|error| miette!(error.to_string()))?;
        if code != 0 {
            return Err(miette!("program exited with code {code}"));
        }
        Ok(())
    }
}
