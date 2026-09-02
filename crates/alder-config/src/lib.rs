//! Alder project configuration.
//!
//! This crate handles parsing and validation of `alder.jsonc` configuration files.
//! Alder supports three types of configurations:
//!
//! - **Application**: A project that compiles to a JavaScript application
//! - **Package**: A publishable library with version constraints
//! - **Workspace**: A collection of related projects sharing dependencies
//!
//! ## Example alder.jsonc (Application)
//!
//! ```jsonc
//! {
//!     "type": "application",
//!     "target": "cloudflare",
//!     "dependencies": {
//!         "alder/core": "1.0.0 <= v < 2.0.0"
//!     }
//! }
//! ```
//!
//! ## Example alder.jsonc (Package)
//!
//! ```jsonc
//! {
//!     "type": "package",
//!     "name": "alice/my-package",
//!     "version": "1.0.0",
//!     "summary": "A helpful package",
//!     "license": "MIT",
//!     "dependencies": {
//!         "alder/core": "1.0.0 <= v < 2.0.0"
//!     }
//! }
//! ```
//!
//! ## Example alder.jsonc (Workspace)
//!
//! ```jsonc
//! {
//!     "type": "workspace",
//!     "members": ["packages/*", "apps/my-app"],
//!     "dependencies": {
//!         "alder/core": "1.0.0 <= v < 2.0.0"
//!     }
//! }
//! ```
//!
//! ## Dependency Types
//!
//! Dependencies can be specified as:
//! - Version constraint: `"1.0.0 <= v < 2.0.0"`
//! - Workspace reference: `{ "workspace": true }`
//! - Path reference: `{ "path": "../my-lib" }`
//! - Git reference: `{ "git": "https://...", "branch": "main" }`

mod config;
mod error;
mod name;
mod parse;

pub use config::{
    Application, CONFIG_FILE_NAME, Config, Dependency, DependencySource, GitDep, Package, PathDep,
    Target, Workspace, WorkspaceDep,
};
pub use error::{ConfigError, Position};
pub use name::{PackageName, PackageNameError};
pub use parse::{parse, parse_file};
