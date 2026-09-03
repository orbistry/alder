use std::{path::PathBuf, sync::Arc};

use alder_bundle::EntryKind;
use alder_config::{Config, Target};
use alder_driver::{
    BuildMode, BuildResult, Database, FileSystemSource, InterfaceCache, Project, build_graph,
    build_with_dependencies,
};
use miette::{IntoDiagnostic, Result, miette};
use tokio::sync::Mutex;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// Path to the project
    #[arg(default_value = ".")]
    pub path: PathBuf,
    /// Output ESM file
    #[arg(short, long, default_value = "dist/main.mjs")]
    pub output: PathBuf,
}

impl Args {
    pub async fn exec(self) -> Result<()> {
        let compiled = compile(&self.path, BuildMode::Build).await?;
        let kind = match compiled.target {
            Target::Standalone => EntryKind::Standalone,
            Target::Cloudflare => EntryKind::Cloudflare,
        };
        let bundle = bundle(&compiled.result, kind).await?;
        let output = if self.output.is_absolute() {
            self.output
        } else {
            compiled.root.join(self.output)
        };
        if let Some(parent) = output.parent() {
            tokio::fs::create_dir_all(parent).await.into_diagnostic()?;
        }
        tokio::fs::write(&output, bundle).await.into_diagnostic()?;
        eprintln!("Built {}", output.display());
        Ok(())
    }
}

pub(super) struct Compiled {
    pub root: PathBuf,
    pub target: Target,
    pub result: BuildResult,
}

pub(super) async fn compile(path: &PathBuf, mode: BuildMode) -> Result<Compiled> {
    compile_inner(path, mode, true).await
}

#[cfg(test)]
pub(super) async fn compile_ephemeral(path: &PathBuf, mode: BuildMode) -> Result<Compiled> {
    compile_inner(path, mode, false).await
}

async fn compile_inner(path: &PathBuf, mode: BuildMode, persist: bool) -> Result<Compiled> {
    let project = Project::load(path).await.into_diagnostic()?;
    let target = project_target(&project.config)?;
    let db = Arc::new(Mutex::new(Database::new(FileSystemSource::new())));
    let mut modules = project
        .discover_modules(&*db.lock().await)
        .await
        .into_diagnostic()?;
    if modules.is_empty() {
        return Err(miette!("no Alder source files found"));
    }
    let dependencies = project
        .build_dependencies(&mut *db.lock().await, &modules, mode == BuildMode::Test)
        .await
        .into_diagnostic()?;
    modules.extend(dependencies.source_modules.iter().cloned());
    modules.sort();
    modules.dedup();
    let graph = build_graph(db.clone(), &modules).await.into_diagnostic()?;
    let result = build_with_dependencies(db, &graph, mode, dependencies).await;
    for warning in &result.warnings {
        eprintln!("{:?}", miette::Report::new(warning.clone()));
    }
    if !result.is_success() {
        let mut errors = result
            .modules
            .values()
            .filter_map(|result| match result {
                alder_driver::ModuleResult::Failed { diagnostics } => Some(diagnostics.clone()),
                alder_driver::ModuleResult::Success { .. } => None,
            })
            .flatten()
            .collect::<Vec<_>>();
        errors.sort_by(|left, right| {
            left.source()
                .name()
                .cmp(right.source().name())
                .then_with(|| left.message().cmp(right.message()))
        });
        let Some(primary) = errors.pop() else {
            return Err(miette!("compilation failed without a diagnostic"));
        };
        let primary = errors
            .into_iter()
            .fold(primary, |primary, related| primary.with_related(related));
        return Err(miette::Report::new(primary));
    }
    if persist {
        persist_semantic_artifacts(&project.root, &result)?;
    }
    Ok(Compiled {
        root: project.root,
        target,
        result,
    })
}

pub(super) fn persist_semantic_artifacts(
    root: &std::path::Path,
    result: &BuildResult,
) -> Result<()> {
    let cache = InterfaceCache::new(root);
    for interface in &result.interfaces {
        cache.save(interface).into_diagnostic()?;
    }
    for index in &result.package_instance_indexes {
        cache.save_package_index(index).into_diagnostic()?;
    }
    Ok(())
}

pub(super) async fn bundle(result: &BuildResult, kind: EntryKind) -> Result<String> {
    let entry = "alder://app/main.mjs";
    alder_bundle::bundle(result.artifacts.values().cloned(), entry, kind)
        .await
        .map_err(|error| miette!(error.to_string()))
}

fn project_target(config: &Config) -> Result<Target> {
    match config {
        Config::Application(application) => Ok(application.target),
        Config::Package(package) => package
            .target
            .ok_or_else(|| miette!("target-neutral packages cannot be executed directly")),
        Config::Workspace(_) => Err(miette!(
            "run/build from a workspace member with an application config"
        )),
    }
}
