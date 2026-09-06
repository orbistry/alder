use std::{path::PathBuf, sync::Arc};

use alder_driver::{
    BuildMode, Database, FileSystemSource, Project, build_graph_with_dependencies,
    build_with_reporter,
};
use miette::{IntoDiagnostic, Result};
use tokio::sync::Mutex;

#[derive(clap::Args)]
pub struct Args {
    /// Path to the project (defaults to current directory)
    #[arg(default_value = ".")]
    pub path: PathBuf,
}

impl Args {
    pub async fn exec(self) -> Result<()> {
        super::Cmd::Check(self).exec().await
    }

    pub(super) async fn exec_with(self, output: &crate::reporting::Output) -> Result<()> {
        let project = Project::load(&self.path).await.into_diagnostic()?;
        output.project("Checking", &project);
        let db = Arc::new(Mutex::new(Database::new(FileSystemSource::new())));
        output.detail("Discovering", "source modules");
        let mut modules = project
            .discover_modules(&*db.lock().await)
            .await
            .into_diagnostic()?;
        output.detail("Discovered", format!("{} modules", modules.len()));
        if modules.is_empty() {
            return Ok(());
        }
        output.detail("Resolving", "dependencies");
        let dependencies = project
            .build_dependencies(&mut *db.lock().await, &modules, false)
            .await
            .into_diagnostic()?;
        modules.extend(dependencies.source_modules.iter().cloned());
        modules.sort();
        modules.dedup();
        let graph = build_graph_with_dependencies(db.clone(), &modules, &dependencies)
            .await
            .into_diagnostic()?;
        let result = build_with_reporter(
            db,
            &graph,
            BuildMode::Check,
            dependencies,
            Arc::new(output.clone()),
        )
        .await;
        super::build::report_diagnostics(&project.root, &result, output)?;
        super::build::persist_semantic_artifacts(&project.root, &result)
    }
}
