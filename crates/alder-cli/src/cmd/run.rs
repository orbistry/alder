use std::path::PathBuf;

use alder_bundle::EntryKind;
use alder_config::Target;
use alder_driver::BuildMode;
use miette::{IntoDiagnostic, Result, miette};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// Path to a standalone project or a previously built .mjs artifact
    #[arg(default_value = ".")]
    pub path: PathBuf,
    /// Arguments exposed through `Cli.args()`
    #[arg(last = true)]
    pub args: Vec<String>,
}

impl Args {
    pub(super) async fn exec(self, output: &crate::reporting::Output) -> Result<()> {
        if self.path.is_file()
            && self
                .path
                .extension()
                .is_some_and(|extension| extension == "mjs")
        {
            output.stage("runtime");
            output.status("Running", crate::reporting::display_path(&self.path));
            let code = alder_runtime::execute(
                tokio::fs::read_to_string(&self.path)
                    .await
                    .into_diagnostic()?,
                self.args,
            )
            .await
            .map_err(|error| miette!(error.to_string()))?;
            return if code == 0 {
                Ok(())
            } else {
                Err(miette!("program exited with code {code}"))
            };
        }
        let started = std::time::Instant::now();
        let compiled = super::build::compile(&self.path, BuildMode::Build, output).await?;
        if compiled.target != Target::Standalone {
            return Err(miette!("alder run requires target: standalone"));
        }
        output.stage("bundling");
        output.status("Bundling", crate::reporting::display_path(&compiled.root));
        let bundle = if compiled.web.is_some() {
            super::web::bundle(&compiled).await?.server
        } else {
            super::build::bundle(&compiled, EntryKind::Standalone).await?
        };
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
