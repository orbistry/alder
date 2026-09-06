use std::path::PathBuf;
use std::sync::Arc;

use alder_driver::{
    BuildMode, Database, FileSystemSource, Project, build_graph_with_dependencies,
    build_with_dependencies,
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
        eprintln!("Loading project from {:?}...", self.path);
        let project = Project::load(&self.path).await.into_diagnostic()?;

        eprintln!("Project root: {:?}", project.root);
        eprintln!("Members: {}", project.members.len());

        let db = Arc::new(Mutex::new(Database::new(FileSystemSource::new())));

        eprintln!("Discovering modules...");
        let mut modules = project
            .discover_modules(&*db.lock().await)
            .await
            .into_diagnostic()?;

        eprintln!("Found {} modules", modules.len());

        if modules.is_empty() {
            eprintln!("No Alder source files found.");
            return Ok(());
        }

        eprintln!("Building dependency graph...");
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

        eprintln!("Dependency order: {} modules", graph.order.len());

        eprintln!("Compiling...");
        let result = build_with_dependencies(db, &graph, BuildMode::Check, dependencies).await;

        eprintln!();
        if !result.warnings.is_empty() {
            for warning in &result.warnings {
                eprintln!("{:?}", miette::Report::new(warning.clone()));
            }
            eprintln!();
        }

        if result.is_success() {
            super::build::persist_semantic_artifacts(&project.root, &result)?;
            eprintln!(
                "Success! Compiled {} modules ({} declarations)",
                result.total,
                result
                    .modules
                    .values()
                    .filter_map(|r| match r {
                        alder_driver::ModuleResult::Success { decl_count } => Some(decl_count),
                        _ => None,
                    })
                    .sum::<usize>()
            );
            Ok(())
        } else {
            eprintln!("Compilation failed.");
            if !result.diagnostics.is_empty() {
                eprintln!("  {} build errors", result.diagnostics.len());
            }
            eprintln!("  {} succeeded", result.success);
            let blocked = result
                .modules
                .values()
                .filter(|module| matches!(module, alder_driver::ModuleResult::Blocked))
                .count();
            eprintln!("  {} failed", result.failed - blocked);
            if blocked != 0 {
                eprintln!("  {blocked} blocked by build validation errors");
            }

            let mut diagnostics = result
                .modules
                .values()
                .filter_map(|module_result| match module_result {
                    alder_driver::ModuleResult::Failed { diagnostics } => Some(diagnostics.iter()),
                    alder_driver::ModuleResult::Success { .. }
                    | alder_driver::ModuleResult::Blocked => None,
                })
                .flatten()
                .chain(result.diagnostics.iter())
                .collect::<Vec<_>>();
            diagnostics.sort_by(|left, right| {
                left.source()
                    .name()
                    .cmp(right.source().name())
                    .then_with(|| left.message().cmp(right.message()))
            });
            for diagnostic in diagnostics {
                eprintln!();
                eprintln!("{:?}", miette::Report::new(diagnostic.clone()));
            }

            std::process::exit(1);
        }
    }
}
