//! Alder compiler driver and build system.
//!
//! This crate provides the infrastructure for building Alder projects:
//!
//! - **File abstraction**: `FileSource` trait for runtime-agnostic file I/O
//! - **Caching**: `Database` for managing source files and compilation results
//! - **Project loading**: Parse `alder.jsonc` and discover source files
//! - **Dependency graph**: Build and analyze module dependencies
//! - **Parallel compilation**: Compile modules respecting dependency order
//! - **Incremental builds**: Interface-based caching for fast rebuilds
//!
//! # Example
//!
//! ```no_run
//! use alder_driver::{Project, Database, BuildMode, build_with_dependencies, build_graph_with_dependencies};
//! use alder_driver::source::FileSystemSource;
//! use std::sync::Arc;
//! use tokio::sync::Mutex;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), alder_driver::DriverError> {
//!     // Load project
//!     let project = Project::load(".").await?;
//!
//!     // Create database with filesystem source
//!     let db = Arc::new(Mutex::new(Database::new(FileSystemSource::new())));
//!
//!     // Discover modules
//!     let mut modules = project.discover_modules(&*db.lock().await).await?;
//!     let dependencies = project
//!         .build_dependencies(&mut *db.lock().await, &modules, false)
//!         .await?;
//!     modules.extend(dependencies.source_modules.iter().cloned());
//!     modules.sort();
//!     modules.dedup();
//!
//!     // Build dependency graph
//!     let graph = build_graph_with_dependencies(db.clone(), &modules, &dependencies).await?;
//!
//!     // Compile everything
//!     let result = build_with_dependencies(db, &graph, BuildMode::Check, dependencies).await;
//!
//!     println!("Compiled {} modules ({} success, {} failed)",
//!         result.total, result.success, result.failed);
//!
//!     Ok(())
//! }
//! ```

pub mod compile;
pub mod database;
pub mod error;
pub mod graph;
pub mod interface;
pub mod progress;
pub mod project;
mod report;
pub mod source;

// Re-export main types
pub use compile::{
    BuildDependencies, BuildMode, BuildResult, ModuleResult, build_graph_with_dependencies,
    build_with_dependencies, build_with_reporter,
};
pub use database::Database;
pub use error::DriverError;
pub use graph::DepGraph;
pub use interface::{InterfaceCache, InterfaceFile, ModuleMeta, PackageInstanceIndexFile};
pub use project::{Project, ProjectMember};
pub use source::{FileSource, FileSystemSource, InMemorySource, OverlaySource};
