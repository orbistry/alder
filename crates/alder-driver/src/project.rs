//! Project loading and discovery.
//!
//! Handles loading `alder.jsonc` configuration files and discovering
//! source files within projects.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use url::Url;

use alder_config::{Config, Dependency, DependencySource, Workspace};

use crate::compile::BuildDependencies;
use crate::database::Database;
use crate::error::DriverError;
use crate::interface::{InterfaceCache, OwnedPackageId};
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
        let root = find_project_root(path)?.canonicalize().map_err(|_| {
            DriverError::InvalidModulePath {
                path: path.to_path_buf(),
            }
        })?;
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
                let mut found = db.glob(&base_uri, "**/*.ald").await?;
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

    /// Resolve each discovered source module to the package identity used by
    /// canonicalization, coherence, and persistent interface artifacts.
    pub fn module_packages(&self, modules: &[Url]) -> BTreeMap<Url, OwnedPackageId> {
        modules
            .iter()
            .filter_map(|uri| {
                let path = uri.to_file_path().ok()?;
                let member = self.members.iter().find(|member| {
                    member
                        .source_dirs
                        .iter()
                        .any(|source| path.starts_with(source))
                })?;
                Some((uri.clone(), member.package_id()))
            })
            .collect()
    }

    /// Load persistent semantic artifacts for packages actually imported by
    /// this build. Path dependencies are read from the dependency project's
    /// `.alder` cache; workspace dependencies are source modules in this build
    /// and therefore need no persistent copy.
    pub async fn build_dependencies(
        &self,
        db: &mut Database,
        modules: &[Url],
        include_test: bool,
    ) -> Result<BuildDependencies, DriverError> {
        let mut imported = std::collections::BTreeSet::new();
        for uri in modules {
            let source = db.source(uri).await?;
            let bump = bumpalo::Bump::new();
            let source = bump.alloc_str(source);
            let Ok(module) = alder_parse::parse_module(&bump, source) else {
                continue;
            };
            for import in module.imports() {
                if let alder_source::ModuleRoot::Package { author, package } =
                    import.path.value.root
                {
                    imported.insert((author.value.to_owned(), package.value.to_owned()));
                }
            }
        }

        let mut result = BuildDependencies {
            module_packages: self.module_packages(modules),
            ..BuildDependencies::default()
        };
        let mut loaded = std::collections::BTreeSet::new();
        for member in &self.members {
            let mut dependencies = match &member.config {
                Config::Application(config) => config.dependencies.clone(),
                Config::Package(config) => config.dependencies.clone(),
                Config::Workspace(_) => BTreeMap::new(),
            };
            if include_test {
                match &member.config {
                    Config::Application(config) => {
                        dependencies.extend(config.test_dependencies.clone())
                    }
                    Config::Package(config) => {
                        dependencies.extend(config.test_dependencies.clone())
                    }
                    Config::Workspace(_) => {}
                }
            }
            for (name, dependency) in dependencies {
                let key = (name.author().to_owned(), name.project().to_owned());
                if !imported.contains(&key) || !loaded.insert(key.clone()) {
                    continue;
                }
                let package = OwnedPackageId::Named {
                    author: key.0,
                    project: key.1,
                };
                let root = match dependency {
                    Dependency::Source(DependencySource::Path(path)) => member.root.join(path.path),
                    Dependency::Source(DependencySource::Workspace(_)) => continue,
                    Dependency::Constraint(_) | Dependency::Source(DependencySource::Git(_)) => {
                        self.root
                            .join(".alder")
                            .join("dependencies")
                            .join(name.author())
                            .join(name.project())
                    }
                };
                if root.join("alder.jsonc").is_file() {
                    let dependency_project = Project::load(&root).await?;
                    let dependency_modules = dependency_project.discover_modules(db).await?;
                    let declared_package = dependency_project
                        .members
                        .first()
                        .map(ProjectMember::package_id);
                    if declared_package.as_ref() != Some(&package) {
                        return Err(DriverError::IncompatibleInterface {
                            reason: "path dependency declares a different package identity"
                                .to_owned(),
                        });
                    }
                    result.module_packages.extend(
                        dependency_modules
                            .iter()
                            .cloned()
                            .map(|module| (module, package.clone())),
                    );
                    result.source_modules.extend(dependency_modules);
                }
                let cache = InterfaceCache::new(&root);
                let index = cache.load_package_index_checked(&package)?;
                if index.package != package {
                    return Err(DriverError::IncompatibleInterface {
                        reason: "dependency instance index has the wrong package identity"
                            .to_owned(),
                    });
                }
                for module in &index.modules {
                    let interface = cache.load_interface(module)?;
                    if interface.module != *module {
                        return Err(DriverError::IncompatibleInterface {
                            reason: "dependency interface has the wrong module identity".to_owned(),
                        });
                    }
                    result.interfaces.push(interface);
                }
                result.package_instance_indexes.push(index);
            }
        }
        Ok(result)
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
        Config::Application(_) | Config::Package(_) => vec![root.join("src")],
        Config::Workspace(_) => vec![], // Workspaces don't have source dirs directly
    };

    ProjectMember {
        root: root.to_path_buf(),
        config,
        source_dirs,
    }
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

    pub fn package_id(&self) -> OwnedPackageId {
        match &self.config {
            Config::Package(package) => OwnedPackageId::Named {
                author: package.name.author().to_owned(),
                project: package.name.project().to_owned(),
            },
            Config::Application(_) => OwnedPackageId::Application,
            Config::Workspace(_) => OwnedPackageId::ApplicationMember(self.name()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interface::{InterfaceFile, OwnedModuleId, PackageInstanceIndexFile};
    use crate::source::InMemorySource;

    #[test]
    fn package_modules_receive_the_declared_package_identity() {
        let root = PathBuf::from("/workspace/widgets");
        let member = ProjectMember {
            root: root.clone(),
            config: Config::Package(alder_config::Package {
                compiler: None,
                name: "vendor/widgets".parse().unwrap(),
                version: "0.1.0".to_owned(),
                summary: "Widgets".to_owned(),
                license: "MIT".to_owned(),
                target: None,
                dependencies: BTreeMap::new(),
                test_dependencies: BTreeMap::new(),
            }),
            source_dirs: vec![root.join("src")],
        };
        let project = Project {
            root,
            config: member.config.clone(),
            members: vec![member],
        };
        let module = Url::from_file_path("/workspace/widgets/src/model.ald").unwrap();

        assert_eq!(
            project.module_packages(std::slice::from_ref(&module))[&module],
            OwnedPackageId::Named {
                author: "vendor".to_owned(),
                project: "widgets".to_owned(),
            }
        );
    }

    #[tokio::test]
    async fn imported_path_dependency_loads_each_interface_and_its_package_index() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "alder-project-dependency-test-{}-{nonce}",
            std::process::id()
        ));
        let app_root = root.join("app");
        let dependency_root = root.join("widgets");
        let package = OwnedPackageId::Named {
            author: "vendor".to_owned(),
            project: "widgets".to_owned(),
        };
        let module = OwnedModuleId {
            package: package.clone(),
            path: vec!["api".to_owned()],
        };
        let interface = alder_ast::Interface {
            home: alder_ast::ModuleId {
                package: alder_ast::PackageId::Named(alder_ast::PackageName {
                    author: "vendor",
                    project: "widgets",
                }),
                path: &["api"],
            },
            values: &[],
            types: &[],
            enums: &[],
            traits: &[],
            instances: &[],
            modules: &[],
            private_names: &[],
        };
        let interface =
            InterfaceFile::dehydrate_with_source(&interface, "file:///dependency/src/api.ald")
                .unwrap();
        let index =
            PackageInstanceIndexFile::new(package.clone(), vec![module.clone()], vec![]).unwrap();
        let cache = InterfaceCache::new(&dependency_root);
        cache.save(&interface).unwrap();
        cache.save_package_index(&index).unwrap();

        let dependency_name = "vendor/widgets".parse().unwrap();
        let config = Config::Application(alder_config::Application {
            compiler: None,
            target: alder_config::Target::Standalone,
            dependencies: BTreeMap::from([(
                dependency_name,
                Dependency::Source(DependencySource::Path(alder_config::PathDep {
                    path: dependency_root.to_string_lossy().into_owned(),
                })),
            )]),
            test_dependencies: BTreeMap::new(),
        });
        let project = Project {
            root: app_root.clone(),
            config: config.clone(),
            members: vec![ProjectMember {
                root: app_root.clone(),
                config,
                source_dirs: vec![app_root.join("src")],
            }],
        };
        let source_uri = Url::from_file_path(app_root.join("src/main.ald")).unwrap();
        let source = InMemorySource::with_files([(
            source_uri.clone(),
            "import @vendor/widgets/api".to_owned(),
        )]);
        let mut database = Database::new(source);
        let dependencies = project
            .build_dependencies(&mut database, std::slice::from_ref(&source_uri), false)
            .await
            .unwrap();

        assert_eq!(dependencies.interfaces.len(), 1);
        assert_eq!(dependencies.interfaces[0].module, module);
        assert_eq!(dependencies.package_instance_indexes, vec![index]);
        assert_eq!(
            dependencies.module_packages[&source_uri],
            OwnedPackageId::Application
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
