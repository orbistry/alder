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
        super::Cmd::Test(self).exec().await
    }

    pub(super) async fn exec_with(self, output: &crate::reporting::Output) -> Result<()> {
        let started = std::time::Instant::now();
        let compiled = super::build::compile_reported(&self.path, BuildMode::Test, output).await?;
        output.stage("bundling");
        output.status("Bundling", crate::reporting::display_path(&compiled.root));
        let bundle = super::build::bundle(&compiled, EntryKind::Test).await?;
        output.finish("test build", started.elapsed());
        output.stage("test execution");
        output.status("Testing", crate::reporting::display_path(&compiled.root));
        let failures = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let failed = failures.clone();
        let reporter = output.clone();
        let test_started = std::time::Instant::now();
        let code = alder_runtime::execute_tests(bundle, move |event| {
            if reporter.test_event(event, test_started.elapsed()) {
                failed.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        })
        .await
        .map_err(|error| miette!(error.to_string()))?;
        if code != 0 {
            if failures.load(std::sync::atomic::Ordering::Relaxed) {
                output.mark_failure_reported();
            }
            return Err(miette!("tests failed"));
        }
        Ok(())
    }
}
