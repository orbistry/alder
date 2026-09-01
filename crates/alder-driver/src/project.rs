//! Project loading and discovery.
//!
//! Handles loading `alder.jsonc` configuration files and discovering
//! source files within projects.

use std::path::{Path, PathBuf};
use url::Url;

use alder_config::{Config, Workspace};

use crate::database::Database;
use crate::error::DriverError;
use crate::source::path_to_uri;

/// A loaded Alder project.
#[derive(Debug)]
pub struct Project {
    /// Root directory of the project.
    pub root: PathBuf,

    /// Parsed configuration.
    pub config: Config,

    /// Workspace members (for workspace configs).
    pub members: Vec<ProjectMember>,
}

/// A member of a workspace, or a standalone project.
#[derive(Debug)]
pub struct ProjectMember {
    /// Root directory of the member.
    pub root: PathBuf,

    /// Parsed configuration.
    pub config: Config,

    /// Resolved source directories.
    pub source_dirs: Vec<PathBuf>,
}

impl Project {
    /// Load a project from a directory.
    ///
    /// Searches for `alder.jsonc` in the given directory and parent directories.
    pub async fn load(path: impl AsRef<Path>) -> Result<Self, DriverError> {
        let path = path.as_ref();

        // Find project root (directory containing alder.jsonc)
        let root = find_project_root(path)?;
        let config_path = root.join("alder.jsonc");

        // Parse the config
        let config = alder_config::parse_file(&config_path)?;

        // Load members if this is a workspace
        let members = match &config {
            Config::Workspace(ws) => load_workspace_members(&root, ws).await?,
            Config::Application(app) => vec![make_member(&root, Config::Application(app.clone()))],
            Config::Package(pkg) => vec![make_member(&root, Config::Package(pkg.clone()))],
        };

        Ok(Project {
            root,
            config,
            members,
        })
    }

    /// Discover all Alder source files in the project.
    pub async fn discover_modules(&self, db: &Database) -> Result<Vec<Url>, DriverError> {
        let mut modules = Vec::new();

        for member in &self.members {
            for source_dir in &member.source_dirs {
                let base_uri = path_to_uri(source_dir)?;
                let mut found = db.glob(&base_uri, "**/*.alder").await?;
                modules.append(&mut found);
            }
        }

        Ok(modules)
    }

    /// Get source directories from config.
    pub fn source_directories(&self) -> Vec<PathBuf> {
        self.members
            .iter()
            .flat_map(|m| m.source_dirs.clone())
            .collect()
    }
}

/// Find the project root by searching for alder.jsonc.
fn find_project_root(start: &Path) -> Result<PathBuf, DriverError> {
    let start = if start.is_file() {
        start.parent().unwrap_or(start)
    } else {
        start
    };

    let mut current = start.to_path_buf();

    loop {
        let config_path = current.join("alder.jsonc");
        if config_path.exists() {
            return Ok(current);
        }

        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => {
                return Err(DriverError::ProjectNotFound {
                    path: start.to_path_buf(),
                });
            }
        }
    }
}

/// Load all workspace members.
async fn load_workspace_members(
    workspace_root: &Path,
    workspace: &Workspace,
) -> Result<Vec<ProjectMember>, DriverError> {
    let mut members = Vec::new();

    for pattern in &workspace.members {
        let full_pattern = workspace_root.join(pattern);
        let pattern_str = full_pattern.to_string_lossy();

        let matches: Vec<_> = glob::glob(&pattern_str)
            .map_err(|e| DriverError::InvalidModulePath {
                path: PathBuf::from(e.msg),
            })?
            .filter_map(|r| r.ok())
            .collect();

        if matches.is_empty() {
            return Err(DriverError::MemberNotFound {
                pattern: pattern.clone(),
            });
        }

        for member_path in matches {
            // member_path is the glob match - we need to find alder.jsonc
            let member_root = if member_path.is_file() {
                member_path.parent().unwrap().to_path_buf()
            } else {
                member_path
            };

            let config_path = member_root.join("alder.jsonc");
            if !config_path.exists() {
                continue;
            }

            let config = alder_config::parse_file(&config_path)?;
            members.push(make_member(&member_root, config));
        }
    }

    Ok(members)
}

/// Create a ProjectMember from config.
fn make_member(root: &Path, config: Config) -> ProjectMember {
    let source_dirs = match &config {
        Config::Application(app) => resolve_source_dirs(root, &app.source_directories),
        Config::Package(_) => vec![root.join("src")],
        Config::Workspace(_) => vec![], // Workspaces don't have source dirs directly
    };

    ProjectMember {
        root: root.to_path_buf(),
        config,
        source_dirs,
    }
}

/// Resolve source directory paths relative to project root.
fn resolve_source_dirs(root: &Path, dirs: &[String]) -> Vec<PathBuf> {
    dirs.iter().map(|d| root.join(d)).collect()
}

impl ProjectMember {
    /// Get the project name (for packages) or a generated name (for applications).
    pub fn name(&self) -> String {
        match &self.config {
            Config::Package(pkg) => pkg.name.to_string(),
            Config::Application(_) => "application".to_string(),
            Config::Workspace(_) => "workspace".to_string(),
        }
    }
}
