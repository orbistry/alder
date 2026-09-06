//! Error types for the alder driver.

use miette::Diagnostic;
use std::path::PathBuf;
use thiserror::Error;
use url::Url;

/// Main error type for driver operations.
#[derive(Debug, Error, Diagnostic)]
pub enum DriverError {
    #[error("file not found: {uri}")]
    FileNotFound { uri: Url },

    #[error("failed to read file {path}: {source}")]
    ReadError {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write file {path}: {source}")]
    WriteError {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid file URI: {uri}")]
    InvalidFileUri { uri: Url },

    #[error("failed to parse config: {0}")]
    ConfigError(#[from] alder_config::ConfigError),

    #[error("project root not found: no alder.jsonc in {path} or parent directories")]
    ProjectNotFound { path: PathBuf },

    #[error("workspace member not found: {pattern}")]
    MemberNotFound { pattern: String },

    #[error("workspace package {name} is declared by both {first} and {second}")]
    #[diagnostic(code(alder::driver::duplicate_workspace_package))]
    DuplicateWorkspacePackage {
        name: String,
        first: PathBuf,
        second: PathBuf,
    },

    #[error("dependency package {name} resolves to both {first} and {second}")]
    #[diagnostic(code(alder::driver::duplicate_dependency_package))]
    DuplicateDependencyPackage {
        name: String,
        first: PathBuf,
        second: PathBuf,
    },

    #[error("import cycle detected: {cycle}")]
    ImportCycle { cycle: String },

    #[error("module not found: {module}")]
    ModuleNotFound { module: String },

    #[error("missing explicit package or source-relative module path for {uri}")]
    #[diagnostic(code(alder::driver::missing_module_identity))]
    MissingModuleIdentity { uri: Url },

    #[error("failed to serialize interface: {0}")]
    SerializeError(#[from] bincode::Error),

    #[error("incompatible interface: {reason}")]
    IncompatibleInterface { reason: String },

    #[error("invalid module path: {path}")]
    InvalidModulePath { path: PathBuf },
}
