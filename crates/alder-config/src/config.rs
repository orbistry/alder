//! Project configuration types.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::PackageName;

/// The default config file name.
pub const CONFIG_FILE_NAME: &str = "alder.jsonc";

/// A Alder project configuration, parsed from `alder.jsonc`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Config {
    Application(Application),
    Package(Package),
    Workspace(Workspace),
}

/// An application project configuration.
///
/// Applications are executables that compile to a JavaScript application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Application {
    /// Required compiler version (semver).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiler: Option<String>,

    /// Platform the application is built for.
    pub target: Target,

    /// Direct dependencies with version constraints or source references.
    #[serde(default)]
    pub dependencies: BTreeMap<PackageName, Dependency>,

    /// Test dependencies with version constraints or source references.
    #[serde(default)]
    pub test_dependencies: BTreeMap<PackageName, Dependency>,
}

/// A package (library) configuration.
///
/// Packages can be published and used as dependencies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Package {
    /// Required compiler version (semver).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiler: Option<String>,

    /// Package name in `author/project` format.
    pub name: PackageName,

    /// Package version (semver).
    pub version: String,

    /// Short description (should be under 80 characters).
    pub summary: String,

    /// SPDX license identifier.
    pub license: String,

    /// Platform this package is specific to. Absent means target-neutral:
    /// the package may only reach target-neutral code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,

    /// Dependencies with version constraints or source references.
    #[serde(default)]
    pub dependencies: BTreeMap<PackageName, Dependency>,

    /// Test dependencies with version constraints or source references.
    #[serde(default)]
    pub test_dependencies: BTreeMap<PackageName, Dependency>,
}

/// The platform a project is built for.
///
/// Selects the target-gated part of the standard library and the runtime
/// `alder run` / `alder dev` / `alder deploy` use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    /// Cloudflare Workers and the surrounding platform.
    Cloudflare,
    /// A long-running HTTP server on the embedded runtime.
    Server,
    /// A command-line program on the embedded runtime.
    Cli,
    /// A terminal UI on the embedded runtime.
    Tui,
    /// Client-side only, shipped to a browser.
    Browser,
}

impl Target {
    /// Every target, in the order used for messages.
    pub const ALL: [Target; 5] = [
        Target::Cloudflare,
        Target::Server,
        Target::Cli,
        Target::Tui,
        Target::Browser,
    ];

    /// The name used in `alder.jsonc`.
    pub fn as_str(self) -> &'static str {
        match self {
            Target::Cloudflare => "cloudflare",
            Target::Server => "server",
            Target::Cli => "cli",
            Target::Tui => "tui",
            Target::Browser => "browser",
        }
    }

    /// Parse the `alder.jsonc` spelling.
    pub fn from_name(name: &str) -> Option<Target> {
        Target::ALL.into_iter().find(|t| t.as_str() == name)
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Workspace configuration.
///
/// A workspace is a collection of related projects that share dependencies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    /// Required compiler version (semver).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiler: Option<String>,

    /// Glob patterns for workspace members.
    pub members: Vec<String>,

    /// Dependencies available for members to inherit via `{ "workspace": true }`.
    #[serde(default)]
    pub dependencies: BTreeMap<PackageName, Dependency>,
}

impl Config {
    /// Returns the `compiler` version requirement, if specified.
    pub fn compiler(&self) -> Option<&str> {
        match self {
            Config::Application(app) => app.compiler.as_deref(),
            Config::Package(pkg) => pkg.compiler.as_deref(),
            Config::Workspace(ws) => ws.compiler.as_deref(),
        }
    }
}

// ============================================================================
// Dependencies
// ============================================================================

/// A dependency specification.
///
/// Can be either a version constraint string or a structured source reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Dependency {
    /// Version constraint string: "1.0.0 <= v < 2.0.0"
    Constraint(String),
    /// Structured dependency source (workspace, path, git)
    Source(DependencySource),
}

/// Structured dependency source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DependencySource {
    /// Inherit from workspace root: `{ "workspace": true }`
    Workspace(WorkspaceDep),
    /// Path to local package: `{ "path": "../my-lib" }`
    Path(PathDep),
    /// Git repository: `{ "git": "https://...", "branch": "main" }`
    Git(GitDep),
}

/// Workspace dependency marker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceDep {
    /// Must be `true` to inherit from workspace.
    pub workspace: bool,
}

/// Path-based dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathDep {
    /// Relative path to the package.
    pub path: String,
}

/// Git-based dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitDep {
    /// Git repository URL.
    pub git: String,
    /// Branch to use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Tag to use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Specific revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
}

impl Dependency {
    /// Returns `true` if this is a workspace dependency.
    pub fn is_workspace(&self) -> bool {
        matches!(self, Dependency::Source(DependencySource::Workspace(_)))
    }

    /// Returns `true` if this is a path dependency.
    pub fn is_path(&self) -> bool {
        matches!(self, Dependency::Source(DependencySource::Path(_)))
    }

    /// Returns `true` if this is a git dependency.
    pub fn is_git(&self) -> bool {
        matches!(self, Dependency::Source(DependencySource::Git(_)))
    }

    /// Returns the version constraint if this is a constraint dependency.
    pub fn as_constraint(&self) -> Option<&str> {
        match self {
            Dependency::Constraint(s) => Some(s),
            _ => None,
        }
    }
}
