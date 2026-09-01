use std::path::PathBuf;
use std::sync::Arc;

use alder_driver::{Database, FileSystemSource, Project, build, build_graph};
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
        let modules = project
            .discover_modules(&*db.lock().await)
            .await
            .into_diagnostic()?;

        eprintln!("Found {} modules", modules.len());

        if modules.is_empty() {
            eprintln!("No Alder source files found.");
            return Ok(());
        }

        eprintln!("Building dependency graph...");
        let graph = build_graph(db.clone(), &modules).await.into_diagnostic()?;

        eprintln!("Dependency order: {} modules", graph.order.len());

        eprintln!("Compiling...");
        let result = build(db, &graph).await;

        eprintln!();
        if !result.warnings.is_empty() {
            for warning in &result.warnings {
                eprintln!("Warning: {}", warning);
            }
            eprintln!();
        }

        if result.is_success() {
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
            eprintln!("  {} succeeded", result.success);
            eprintln!("  {} failed", result.failed);

            for (uri, module_result) in &result.modules {
                if let alder_driver::ModuleResult::Failed { message } = module_result {
                    eprintln!();
                    eprintln!("Error in {}:", uri.path());
                    eprintln!("  {}", message);
                }
            }

            std::process::exit(1);
        }
    }
}
