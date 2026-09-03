//! Module compilation orchestration.
//!
//! Each module runs Elm's full pipeline: parse -> canonicalize ->
//! constrain -> solve -> `Interface::from_module` with the solver's
//! annotations. A discovery pass collects canonical type/trait/impl headers
//! package-wide and provisionally solves inferred value interfaces needed by
//! dependents. Every body is then compiled against the same frozen closure.
//! Headers and solved interfaces are deep-copied into a build-wide arena and
//! copied back into each module arena before use, so no phase borrows another
//! module's allocation.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use alder_ast::{
    Interface, ModuleId, PackageId, PackageName, ResolvedImport, ResolvedImportKind,
    ResolvedImportName, Visibility,
};
use alder_report::{Diagnostic, Source};
use bumpalo::Bump;
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use url::Url;

use crate::database::Database;
use crate::error::DriverError;
use crate::graph::DepGraph;
use crate::interface::{
    InterfaceFile, OwnedImplHeader, OwnedModuleId, OwnedPackageId, PackageInstanceIndexFile,
};

/// Result of compiling a single module.
#[derive(Debug)]
pub enum ModuleResult {
    /// Module compiled successfully.
    Success {
        /// Number of declarations in the module.
        decl_count: usize,
    },
    /// Module failed to compile.
    Failed {
        /// Structured diagnostics retaining their named source text.
        diagnostics: Vec<Diagnostic>,
    },
}

/// Result of a full build.
#[derive(Debug)]
pub struct BuildResult {
    /// Results for each module.
    pub modules: HashMap<Url, ModuleResult>,

    /// Total number of modules processed.
    pub total: usize,

    /// Number of successful compilations.
    pub success: usize,

    /// Number of failed compilations.
    pub failed: usize,

    /// Warnings collected during canonicalization.
    pub warnings: Vec<Diagnostic>,

    /// ESM modules produced in build or test mode, keyed by source URI.
    pub artifacts: HashMap<Url, alder_codegen::EmittedModule>,

    /// Solved semantic interfaces ready for persistent caching.
    pub interfaces: Vec<InterfaceFile>,

    /// Complete exported instance indexes, grouped by package.
    pub package_instance_indexes: Vec<PackageInstanceIndexFile>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BuildMode {
    #[default]
    Check,
    Build,
    Test,
}

#[derive(Clone, Debug, Default)]
pub struct BuildDependencies {
    /// Dependency source modules that must participate in this compilation so
    /// generated evidence imports retain in-memory Oxc ASTs through bundling.
    pub source_modules: Vec<Url>,
    pub module_packages: BTreeMap<Url, OwnedPackageId>,
    pub interfaces: Vec<InterfaceFile>,
    pub package_instance_indexes: Vec<PackageInstanceIndexFile>,
}

impl BuildResult {
    /// Check if the build was completely successful.
    pub fn is_success(&self) -> bool {
        self.failed == 0
    }
}

/// Holds the output of compiling a single module.
struct CompileOutput {
    uri: Url,
    result: ModuleResult,
    warnings: Vec<Diagnostic>,
    artifact: Option<alder_codegen::EmittedModule>,
}

struct InterfaceOutput<'a> {
    interface: Option<Interface<'a>>,
    solved: bool,
}

/// Compile all modules through the full pipeline, in dependency order.
///
/// The async part only fetches sources; the CPU-bound compilation runs on
/// tokio's blocking pool (`spawn_blocking`) so no executor worker is ever
/// stalled.
pub async fn build(db: Arc<Mutex<Database>>, graph: &DepGraph) -> BuildResult {
    build_with_mode(db, graph, BuildMode::Check).await
}

pub async fn build_with_mode(
    db: Arc<Mutex<Database>>,
    graph: &DepGraph,
    mode: BuildMode,
) -> BuildResult {
    build_with_dependencies(db, graph, mode, BuildDependencies::default()).await
}

pub async fn build_with_dependencies(
    db: Arc<Mutex<Database>>,
    graph: &DepGraph,
    mode: BuildMode,
    dependencies: BuildDependencies,
) -> BuildResult {
    let modules: Vec<&Url> = graph.levels().into_iter().flatten().collect();
    let sources = fetch_sources(&db, &modules).await;

    tokio::task::spawn_blocking(move || build_sync(sources, mode, dependencies))
        .await
        .expect("compile task panicked")
}

/// Discover every canonical package header, then compile all bodies against
/// that frozen header/interface closure. Provisional solving supplies inferred
/// value interfaces needed by downstream canonicalization; a body failure does
/// not prevent its valid type, trait, and impl headers from entering the
/// package-wide coherence check.
///
/// Type checking is inherently dependency-ordered, so within-build
/// parallelism is limited to source fetching for now.
fn build_sync(
    sources: Vec<(Url, Result<String, String>)>,
    mode: BuildMode,
    dependencies: BuildDependencies,
) -> BuildResult {
    // This arena owns canonical package headers and solved public interfaces.
    // Every source module and all phase-local ASTs use a separate arena in
    // `compile_module`.
    let store = Bump::new();
    let mut interfaces = dependencies
        .interfaces
        .iter()
        .map(|interface| interface.hydrate(&store))
        .collect::<Vec<_>>();
    let package_instances = dependencies
        .package_instance_indexes
        .iter()
        .flat_map(|index| index.hydrate_instances(&store).iter().copied())
        .collect::<Vec<_>>();
    let package_instances = store.alloc_slice_copy(&package_instances);
    let default_package = OwnedPackageId::Application;

    let total = sources.len();
    let mut solved_interfaces = vec![false; total];

    loop {
        let mut progress = false;
        for index in 0..total {
            if solved_interfaces[index] {
                continue;
            }
            let (uri, source) = &sources[index];
            let package = dependencies
                .module_packages
                .get(uri)
                .unwrap_or(&default_package);
            let (_, discovered) = compile_module(
                uri,
                source,
                package,
                &store,
                &interfaces,
                package_instances,
                BuildMode::Check,
            );
            if let Some(interface) = discovered.interface {
                let existing = interfaces
                    .iter()
                    .position(|candidate| candidate.home == interface.home);
                if let Some(existing) = existing {
                    if discovered.solved && !solved_interfaces[index] {
                        interfaces[existing] = interface;
                        progress = true;
                    }
                } else {
                    interfaces.push(interface);
                    progress = true;
                }
                solved_interfaces[index] = discovered.solved;
            }
        }
        if !progress {
            break;
        }
    }

    let mut results: HashMap<Url, ModuleResult> = HashMap::new();
    let mut all_warnings: Vec<Diagnostic> = Vec::new();
    let mut artifacts = HashMap::new();
    let mut interface_files = Vec::new();
    for (uri, source) in &sources {
        let package = dependencies
            .module_packages
            .get(uri)
            .unwrap_or(&default_package);
        let (output, discovered) = compile_module(
            uri,
            source,
            package,
            &store,
            &interfaces,
            package_instances,
            mode,
        );
        if discovered.solved
            && let Some(interface) = discovered.interface
        {
            interface_files.push(
                InterfaceFile::dehydrate_with_source(&interface, uri.as_str())
                    .expect("canonical interfaces always serialize"),
            );
        }
        all_warnings.extend(output.warnings);
        if let Some(artifact) = output.artifact {
            artifacts.insert(output.uri.clone(), artifact);
        }
        results.insert(output.uri, output.result);
    }

    let success = results
        .values()
        .filter(|r| matches!(r, ModuleResult::Success { .. }))
        .count();
    interface_files.sort_by(|left, right| left.module.cmp(&right.module));
    let package_instance_indexes = package_indexes(&interface_files);

    BuildResult {
        modules: results,
        total,
        success,
        failed: total - success,
        warnings: all_warnings,
        artifacts,
        interfaces: interface_files,
        package_instance_indexes,
    }
}

fn package_indexes(interfaces: &[InterfaceFile]) -> Vec<PackageInstanceIndexFile> {
    let mut packages: BTreeMap<OwnedPackageId, (Vec<OwnedModuleId>, Vec<OwnedImplHeader>)> =
        BTreeMap::new();
    for interface in interfaces {
        let (modules, instances) = packages
            .entry(interface.module.package.clone())
            .or_default();
        modules.push(interface.module.clone());
        instances.extend(interface.instances.iter().cloned());
    }
    packages
        .into_iter()
        .map(|(package, (modules, instances))| {
            PackageInstanceIndexFile::new(package, modules, instances)
                .expect("solved interfaces form a valid package index")
        })
        .collect()
}

/// Fetch source content for all modules, in parallel.
async fn fetch_sources(
    db: &Arc<Mutex<Database>>,
    uris: &[&Url],
) -> Vec<(Url, Result<String, String>)> {
    let mut set = JoinSet::new();

    for &uri in uris {
        let uri = uri.clone();
        let db = db.clone();
        set.spawn(async move {
            let source = {
                let mut db = db.lock().await;
                db.source(&uri).await.map(|s| s.to_string())
            };
            (uri, source.map_err(|e| e.to_string()))
        });
    }

    let mut fetched = HashMap::with_capacity(uris.len());
    while let Some(res) = set.join_next().await {
        if let Ok((uri, source)) = res {
            fetched.insert(uri, source);
        }
    }
    uris.iter()
        .map(|uri| {
            let uri = (*uri).clone();
            let source = fetched
                .remove(&uri)
                .unwrap_or_else(|| Err("source fetch task failed".to_owned()));
            (uri, source)
        })
        .collect()
}

/// Run one module through the full pipeline in its own arena:
/// parse -> canonicalize -> constrain -> solve -> interface.
///
/// On success the module's interface is deep-copied into the build-wide
/// `store` arena so it outlives this module's arena.
fn compile_module<'s>(
    uri: &Url,
    source: &Result<String, String>,
    package: &OwnedPackageId,
    store: &'s Bump,
    interfaces: &[Interface<'s>],
    package_instances: &'s [alder_ast::InterfaceImpl<'s>],
    mode: BuildMode,
) -> (CompileOutput, InterfaceOutput<'s>) {
    let report_source = Source::new(
        uri.path(),
        source.as_ref().map_or("", String::as_str).to_owned(),
    );
    let failed = |diagnostics: Vec<Diagnostic>| {
        (
            CompileOutput {
                uri: uri.clone(),
                result: ModuleResult::Failed { diagnostics },
                warnings: vec![],
                artifact: None,
            },
            InterfaceOutput {
                interface: None,
                solved: false,
            },
        )
    };

    let source = match source {
        Ok(s) => s,
        Err(e) => return failed(vec![crate::report::source_failure(report_source, e)]),
    };

    let module_arena = Bump::new();
    let src = module_arena.alloc_str(source);
    let mut parser = alder_parse::Parser::new(&module_arena, src.as_bytes());

    let module = match parser.module() {
        Ok(module) => module,
        Err(e) => return failed(vec![crate::report::parse(report_source, &e)]),
    };

    let home = module_id_from_uri(&module_arena, uri, package);
    let imports = resolve_imports(&module_arena, &module, home.package);
    let interfaces = interfaces
        .iter()
        .filter(|interface| interface.home != home)
        .map(|interface| alder_ast::copy_interface(&module_arena, interface))
        .collect::<Vec<_>>();
    let interfaces = module_arena.alloc_slice_copy(&interfaces);
    let package_instances = alder_ast::copy_interface(
        &module_arena,
        &Interface {
            home,
            values: &[],
            types: &[],
            enums: &[],
            traits: &[],
            instances: package_instances,
            modules: &[],
            private_names: &[],
        },
    )
    .instances;
    let context = alder_can::Context {
        home,
        imports,
        interfaces,
    };
    let header_result = alder_can::canonicalize_headers(&module_arena, context, &module).ok();
    let header_interface = header_result
        .as_ref()
        .map(|result| alder_can::headers_from_module(&module_arena, result.module))
        .map(|interface| alder_ast::copy_interface(store, &interface));
    if let Some(header) = &header_result {
        let database = alder_solve::TraitDatabase::build_with_package_instances(
            &module_arena,
            header.module,
            interfaces,
            package_instances,
        );
        let coherence = database
            .validate(&module_arena)
            .into_iter()
            .filter(|error| coherence_belongs_to(error, home))
            .collect::<Vec<_>>();
        if !coherence.is_empty() {
            let diagnostics = coherence
                .iter()
                .map(|error| {
                    crate::report::solve(
                        report_source.clone(),
                        header.module,
                        &alder_solve::SolveError::Coherence(error.clone()),
                    )
                })
                .collect();
            let (output, _) = failed(diagnostics);
            return (
                output,
                InterfaceOutput {
                    interface: header_interface,
                    solved: false,
                },
            );
        }
    }
    let can_result = match alder_can::canonicalize(&module_arena, context, &module) {
        Ok(can_result) => can_result,
        Err(errors) => {
            let (output, _) = failed(
                errors
                    .iter()
                    .map(|error| crate::report::canonicalize(report_source.clone(), error))
                    .collect(),
            );
            return (
                output,
                InterfaceOutput {
                    interface: header_interface,
                    solved: false,
                },
            );
        }
    };
    let warnings: Vec<Diagnostic> = can_result
        .warnings
        .iter()
        .map(|warning| crate::report::warning(report_source.clone(), warning))
        .collect();

    let header_interface = header_interface.unwrap_or_else(|| {
        let interface = alder_can::headers_from_module(&module_arena, can_result.module);
        alder_ast::copy_interface(store, &interface)
    });

    let constraint = alder_constrain::constrain(&module_arena, can_result.module);
    let trait_database = alder_solve::TraitDatabase::build_with_package_instances(
        &module_arena,
        can_result.module,
        interfaces,
        package_instances,
    );
    let solved = match alder_solve::solve(&module_arena, &constraint, &trait_database) {
        Ok(solved) => solved,
        Err(errors) => {
            let (output, _) = failed(
                errors
                    .iter()
                    .map(|error| {
                        crate::report::solve(report_source.clone(), can_result.module, error)
                    })
                    .collect(),
            );
            return (
                output,
                InterfaceOutput {
                    interface: Some(header_interface),
                    solved: false,
                },
            );
        }
    };

    let module_interface =
        alder_can::from_module(&module_arena, can_result.module, &solved.annotations);
    let artifact = match mode {
        BuildMode::Check => None,
        BuildMode::Build | BuildMode::Test => {
            let options = alder_codegen::EmitOptions {
                mode: if mode == BuildMode::Test {
                    alder_codegen::EmitMode::Test
                } else {
                    alder_codegen::EmitMode::Build
                },
            };
            match alder_codegen::emit_solved_module(can_result.module, &solved, options) {
                Ok(artifact) => Some(artifact),
                Err(error) => {
                    let (output, _) = failed(vec![crate::report::codegen(report_source, &error)]);
                    return (
                        output,
                        InterfaceOutput {
                            interface: Some(header_interface),
                            solved: false,
                        },
                    );
                }
            }
        }
    };

    (
        CompileOutput {
            uri: uri.clone(),
            result: ModuleResult::Success {
                decl_count: can_result.module.items.len(),
            },
            warnings,
            artifact,
        },
        InterfaceOutput {
            interface: Some(alder_ast::copy_interface(store, &module_interface)),
            solved: true,
        },
    )
}

fn coherence_belongs_to(error: &alder_solve::CoherenceError<'_>, home: ModuleId<'_>) -> bool {
    match error {
        alder_solve::CoherenceError::SuperclassCycle { traits } => {
            traits.iter().any(|trait_| trait_.0.module == home)
        }
        alder_solve::CoherenceError::OrphanImpl { implementation, .. }
        | alder_solve::CoherenceError::InvalidTermination { implementation, .. }
        | alder_solve::CoherenceError::KindMismatch { implementation, .. }
        | alder_solve::CoherenceError::ProjectionCycle { implementation, .. } => {
            implementation.module == home
        }
        alder_solve::CoherenceError::OverlappingImpl { first, second, .. } => {
            first.module == home || second.module == home
        }
    }
}

fn module_id_from_uri<'a>(bump: &'a Bump, uri: &Url, package: &OwnedPackageId) -> ModuleId<'a> {
    let path = uri.path();
    let relative = path
        .split("/src/")
        .nth(1)
        .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path));
    let without_extension = relative.strip_suffix(".ald").unwrap_or(relative);
    let mut segments: Vec<_> = without_extension.split('/').collect();
    if segments.last() == Some(&"mod") {
        segments.pop();
    }
    ModuleId {
        package: hydrate_package_id(bump, package),
        path: bump.alloc_slice_fill_iter(
            segments
                .into_iter()
                .map(|part| bump.alloc_str(part) as &str),
        ),
    }
}

fn hydrate_package_id<'a>(bump: &'a Bump, package: &OwnedPackageId) -> PackageId<'a> {
    match package {
        OwnedPackageId::Named { author, project } => PackageId::Named(PackageName {
            author: bump.alloc_str(author),
            project: bump.alloc_str(project),
        }),
        OwnedPackageId::Application => PackageId::Application,
        OwnedPackageId::ApplicationMember(member) => {
            PackageId::ApplicationMember(bump.alloc_str(member))
        }
        OwnedPackageId::Builtin => PackageId::Builtin,
    }
}

fn resolve_imports<'a>(
    bump: &'a Bump,
    module: &alder_source::Module<'a>,
    home_package: PackageId<'a>,
) -> &'a [ResolvedImport<'a>] {
    let imports: Vec<_> = module
        .items
        .iter()
        .filter_map(|item| {
            let alder_source::ItemKind::Import(import) = item.value.kind else {
                return None;
            };
            let path = import.path.value;
            let (package, root_name) = match path.root {
                alder_source::ModuleRoot::Local(_) => (home_package, None),
                alder_source::ModuleRoot::Package { author, package } => (
                    PackageId::Named(PackageName {
                        author: author.value,
                        project: package.value,
                    }),
                    Some(package),
                ),
            };
            let mut parts: Vec<_> = path.segments.iter().map(|segment| segment.value).collect();
            if parts.is_empty()
                && let Some(root_name) = root_name
            {
                parts.push(root_name.value);
            }
            let module_id = ModuleId {
                package,
                path: bump.alloc_slice_copy(&parts),
            };
            let kind = match import.tail {
                alder_source::ImportTail::Module => {
                    let binding = path
                        .segments
                        .last()
                        .copied()
                        .or(root_name)
                        .expect("the parser rejects imports with no bindable segment");
                    ResolvedImportKind::Module { binding }
                }
                alder_source::ImportTail::Alias(binding) => ResolvedImportKind::Module { binding },
                alder_source::ImportTail::Names(names) => ResolvedImportKind::Names(
                    bump.alloc_slice_fill_iter(names.iter().map(|name| ResolvedImportName {
                        source: name.name,
                        binding: name.alias.unwrap_or(name.name),
                    })),
                ),
                alder_source::ImportTail::All(_) => ResolvedImportKind::All,
            };
            Some(ResolvedImport {
                module: module_id,
                region: item.region,
                visibility: match item.value.visibility {
                    alder_source::Visibility::Private => Visibility::Private,
                    alder_source::Visibility::Pub(region) => Visibility::Public(region),
                },
                kind,
            })
        })
        .collect();
    bump.alloc_slice_copy(&imports)
}

/// Build a dependency graph from parsed modules.
///
/// This is a simplified implementation that parses modules to extract imports.
/// For a full implementation, we would parse just the header/imports.
pub async fn build_graph(
    db: Arc<Mutex<Database>>,
    modules: &[Url],
) -> Result<DepGraph, DriverError> {
    let mut graph = DepGraph::new();

    for uri in modules {
        // Parse module to get imports
        let source = {
            let mut db = db.lock().await;
            db.source(uri).await?.to_string()
        };

        let imports = extract_imports(&source, uri, modules);
        graph.add_module(uri.clone(), imports);
    }

    graph.compute_order()?;
    Ok(graph)
}

/// Extract import URIs from source code.
///
/// This is a simplified implementation - in production we'd use the parser.
fn extract_imports(source: &str, current: &Url, known_modules: &[Url]) -> Vec<Url> {
    let mut imports = Vec::new();

    // Parse to get imports
    let bump = Bump::new();
    let src = bump.alloc_str(source);
    let mut parser = alder_parse::Parser::new(&bump, src.as_bytes());

    if let Ok(module) = parser.module() {
        for import in module.imports() {
            if let Some(uri) = resolve_source_import(import, current, known_modules) {
                imports.push(uri);
            }
        }
    }

    imports
}

/// Resolve an import name to a module URI.
///
/// This is a simplified implementation. Full resolution would handle:
/// - Package dependencies
/// - Source directory structure
/// - Module naming conventions
fn resolve_source_import(
    import: &alder_source::Import<'_>,
    _current: &Url,
    known_modules: &[Url],
) -> Option<Url> {
    if !matches!(import.path.value.root, alder_source::ModuleRoot::Local(_)) {
        return None;
    }
    let path = import
        .path
        .value
        .segments
        .iter()
        .map(|segment| segment.value)
        .collect::<Vec<_>>()
        .join("/");
    let file = format!("/src/{path}.ald");
    let index = format!("/src/{path}/mod.ald");
    known_modules
        .iter()
        .find(|uri| uri.path().ends_with(&file) || uri.path().ends_with(&index))
        .cloned()
}

#[cfg(test)]
macro_rules! assert_rendered_diagnostic_snapshot {
    ($source:expr, $diagnostic:expr) => {{
        let source = $source;
        let diagnostic = $diagnostic;
        let mut rendered = String::new();
        miette::GraphicalReportHandler::new_themed(miette::GraphicalTheme::unicode_nocolor())
            .with_width(80)
            .render_report(&mut rendered, &diagnostic)
            .expect("diagnostic renders");
        insta::with_settings!({
            description => source,
            omit_expression => true,
        }, {
            insta::assert_snapshot!(rendered);
        });
    }};
}

#[cfg(test)]
macro_rules! assert_diagnostic_snapshot {
    ($source:expr) => {{
        let source = indoc::indoc!($source);
        let diagnostic = compile_failure(source).await;
        assert_rendered_diagnostic_snapshot!(source, diagnostic);
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::InMemorySource;

    fn url(path: &str) -> Url {
        Url::parse(&format!("file:///{}", path)).unwrap()
    }

    fn dependency_interface(
        source: &str,
        path: &'static [&'static str],
        dependencies: &[InterfaceFile],
    ) -> InterfaceFile {
        let bump = Bump::new();
        let source = bump.alloc_str(source);
        let parsed = alder_parse::parse_module(&bump, source).expect("dependency source parses");
        let interfaces = dependencies
            .iter()
            .map(|interface| interface.hydrate(&bump))
            .collect::<Vec<_>>();
        let interfaces = bump.alloc_slice_copy(&interfaces);
        let home = ModuleId {
            package: PackageId::Named(PackageName {
                author: "vendor",
                project: "widgets",
            }),
            path,
        };
        let canonical = alder_can::canonicalize(
            &bump,
            alder_can::Context {
                home,
                imports: resolve_imports(&bump, &parsed, home.package),
                interfaces,
            },
            &parsed,
        )
        .expect("dependency source canonicalizes");
        let constraints = alder_constrain::constrain(&bump, canonical.module);
        let database = alder_solve::TraitDatabase::build(&bump, canonical.module, interfaces);
        let solved =
            alder_solve::solve(&bump, &constraints, &database).expect("dependency source solves");
        let interface = alder_can::from_module(&bump, canonical.module, &solved.annotations);
        InterfaceFile::dehydrate_with_source(
            &interface,
            &format!("file:///dependency/src/{}.ald", path.join("/")),
        )
        .expect("dependency interface serializes")
    }

    async fn compile_failure(source: &str) -> Diagnostic {
        let diagnostics = compile_failures(source).await;
        assert_eq!(diagnostics.len(), 1, "expected one diagnostic");
        diagnostics.into_iter().next().expect("length checked")
    }

    async fn compile_failures(source: &str) -> Vec<Diagnostic> {
        let mem = InMemorySource::new();
        let uri = url("project/src/main.ald");
        mem.insert(uri.clone(), source.to_owned());
        let db = Arc::new(Mutex::new(Database::new(mem)));
        let graph = build_graph(db.clone(), std::slice::from_ref(&uri))
            .await
            .unwrap();
        let result = build(db, &graph).await;
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("source unexpectedly compiled");
        };
        diagnostics.clone()
    }

    fn ambiguous_failure(source: &str) -> Diagnostic {
        let bump = Bump::new();
        let source = bump.alloc_str(source);
        let parsed = alder_parse::parse_module(&bump, source).expect("source parses");
        let canonical = alder_can::canonicalize(
            &bump,
            alder_can::Context {
                home: ModuleId {
                    package: PackageId::Application,
                    path: &["main"],
                },
                imports: &[],
                interfaces: &[],
            },
            &parsed,
        )
        .expect("source canonicalizes before coherence checking");
        let mut candidates = canonical
            .module
            .items
            .iter()
            .filter_map(|item| match item.value.kind {
                alder_ast::ItemKind::Impl(implementation) => Some(implementation.id),
                _ => None,
            })
            .collect::<Vec<_>>();
        let trait_ = candidates
            .first()
            .and_then(|id| {
                canonical.module.items.iter().find_map(|item| {
                    let alder_ast::ItemKind::Impl(implementation) = item.value.kind else {
                        return None;
                    };
                    (implementation.id == *id).then_some(implementation.trait_ref.trait_)
                })
            })
            .expect("fixture has a trait implementation");
        candidates.push(alder_ast::ImplId {
            module: ModuleId {
                package: PackageId::Named(alder_ast::PackageName {
                    author: "example",
                    project: "dependency",
                }),
                path: &["display"],
            },
            origin: alder_ast::ImplOrigin::Source { item_ordinal: 4 },
        });
        let error =
            alder_solve::SolveError::Trait(alder_solve::SolveTraitError::AmbiguousInstance {
                trait_,
                subject: "Number",
                origin: canonical
                    .module
                    .items
                    .last()
                    .expect("fixture has a call")
                    .region,
                details: bump.alloc(alder_solve::AmbiguousInstanceDetails {
                    candidates: bump.alloc_slice_copy(&candidates),
                    chain: &[],
                }),
            });
        crate::report::solve(
            Source::new("/project/src/main.ald", source.to_owned()),
            canonical.module,
            &error,
        )
    }

    fn instance_cycle_failure(source: &str) -> Diagnostic {
        let bump = Bump::new();
        let source = bump.alloc_str(source);
        let parsed = alder_parse::parse_module(&bump, source).expect("source parses");
        let canonical = alder_can::canonicalize(
            &bump,
            alder_can::Context {
                home: ModuleId {
                    package: PackageId::Application,
                    path: &["main"],
                },
                imports: &[],
                interfaces: &[],
            },
            &parsed,
        )
        .expect("source canonicalizes");
        let trait_ = canonical
            .module
            .items
            .iter()
            .find_map(|item| match item.value.kind {
                alder_ast::ItemKind::Trait(trait_) => Some(trait_.id),
                _ => None,
            })
            .expect("fixture has a trait");
        let origin = canonical
            .module
            .items
            .last()
            .expect("fixture has a use site")
            .region;
        let frame = alder_solve::ObligationFrame {
            trait_,
            subject: "Number",
            required_by: None,
        };
        let error = alder_solve::SolveError::Trait(alder_solve::SolveTraitError::InstanceCycle {
            trait_,
            subject: "Number",
            origin,
            chain: bump.alloc_slice_copy(&[frame, frame]),
        });
        crate::report::solve(
            Source::new("/project/src/main.ald", source.to_owned()),
            canonical.module,
            &error,
        )
    }

    #[tokio::test]
    async fn renders_trait_parser_error_with_source() {
        assert_diagnostic_snapshot! {r#"
            trait Show[a] where a Show {
                fn show(value: a) String
            }
        "#};
    }

    #[tokio::test]
    async fn renders_nested_trait_signature_parser_error_with_source() {
        assert_diagnostic_snapshot! {r#"
            trait Show[a] {
                fn show(value: Array[]) String
            }
        "#};
    }

    #[tokio::test]
    async fn renders_missing_trait_method_annotation_with_source() {
        assert_diagnostic_snapshot! {r#"
            trait Display[a] {
                fn display(value) String
            }
        "#};
    }

    #[tokio::test]
    async fn renders_duplicate_trait_parameter_with_source() {
        assert_diagnostic_snapshot! {r#"
            trait Convert[a, a] {
                fn convert(value: a) a
            }
        "#};
    }

    #[tokio::test]
    async fn renders_unknown_associated_type_with_source() {
        assert_diagnostic_snapshot! {r#"
            trait Source[a] { type Item }
            fn take(value: a) Number
                where a: Source, a.Missing == Number
            {
                0
            }
        "#};
    }

    #[tokio::test]
    async fn renders_ambiguous_associated_type_with_source() {
        assert_diagnostic_snapshot! {r#"
            trait First[a] { type Item }
            trait Second[a] { type Item }
            fn take(value: a) Number
                where a: First + Second, a.Item == Number
            {
                0
            }
        "#};
    }

    #[tokio::test]
    async fn renders_unknown_impl_method_with_source() {
        assert_diagnostic_snapshot! {r#"
            trait Display[a] { fn display(value: a) String }
            impl Display[Number] {
                fn render(value: Number) String { "number" }
            }
        "#};
    }

    #[tokio::test]
    async fn renders_missing_impl_method_with_source() {
        assert_diagnostic_snapshot! {r#"
            trait Display[a] { fn display(value: a) String }
            impl Display[Number] {}
        "#};
    }

    #[tokio::test]
    async fn renders_missing_associated_binding_with_source() {
        assert_diagnostic_snapshot! {r#"
            trait Source[a] {
                type Item
                fn next(value: a) Item
            }
            impl Source[Number] {
                fn next(value: Number) Number { value }
            }
        "#};
    }

    #[tokio::test]
    async fn renders_impl_method_type_mismatch_with_source() {
        assert_diagnostic_snapshot! {r#"
            trait Display[a] { fn display(value: a) String }
            impl Display[Number] {
                fn display(value: Number) Number { value }
            }
        "#};
    }

    #[tokio::test]
    async fn renders_associated_type_mismatch_with_source() {
        assert_diagnostic_snapshot! {r#"
            trait Source[a] {
                type Item
                fn next(value: a) Item
            }
            impl Source[Number] {
                type Item = String
                fn next(value: Number) Number { value }
            }
        "#};
    }

    #[tokio::test]
    async fn renders_duplicate_trait_method_with_source() {
        assert_diagnostic_snapshot! {r#"
            trait Display[a] {
                fn display(value: a) String
                fn display(value: a) String
            }
        "#};
    }

    #[tokio::test]
    async fn renders_duplicate_associated_type_with_source() {
        assert_diagnostic_snapshot! {r#"
            trait Source[a] {
                type Item
                type Item
            }
        "#};
    }

    #[tokio::test]
    async fn renders_duplicate_impl_method_with_source() {
        assert_diagnostic_snapshot! {r#"
            trait Display[a] { fn display(value: a) String }
            impl Display[Number] {
                fn display(value: Number) String { "first" }
                fn display(value: Number) String { "second" }
            }
        "#};
    }

    #[tokio::test]
    async fn renders_duplicate_associated_binding_with_source() {
        assert_diagnostic_snapshot! {r#"
            trait Source[a] {
                type Item
                fn next(value: a) Item
            }
            impl Source[Number] {
                type Item = Number
                type Item = Number
                fn next(value: Number) Number { value }
            }
        "#};
    }

    #[tokio::test]
    async fn renders_invalid_derive_with_source() {
        assert_diagnostic_snapshot! {r#"
            #[derive(Show)]
            type Label = String
        "#};
    }

    #[tokio::test]
    async fn renders_invalid_nested_impl_hole_with_source() {
        assert_diagnostic_snapshot! {r#"
            trait Convert[a] { fn convert(value: a) a }
            impl Convert[Result[Array[_], String]] {
                fn convert(value: Result[Array[Number], String]) Result[Array[Number], String] {
                    value
                }
            }
        "#};
    }

    #[tokio::test]
    async fn renders_missing_instance_with_source() {
        assert_diagnostic_snapshot! {r#"
            trait Display[a] {
                fn display(value: a) String
            }

            fn main() {
                display(1)
            }
        "#};
    }

    #[tokio::test]
    async fn renders_unsatisfied_generic_bound_with_source() {
        assert_diagnostic_snapshot! {r#"
            trait Display[a] {
                fn display(value: a) String
            }

            fn render(value: a) String {
                display(value)
            }
        "#};
    }

    #[tokio::test]
    async fn renders_ambiguous_local_trait_variable_with_source() {
        assert_diagnostic_snapshot! {r#"
            fn ambiguous() {
                let equal = (left, right) -> { left == right }
                ()
            }
        "#};
    }

    #[test]
    fn renders_ambiguous_instance_candidates_with_source() {
        let source = indoc::indoc! {r#"
            trait Display[a] {
                fn display(value: a) String
            }

            impl Display[a] {
                fn display(value: a) String { "generic" }
            }

            impl Display[Number] {
                fn display(value: Number) String { "number" }
            }

            fn main() { display(1) }
        "#};
        assert_rendered_diagnostic_snapshot!(source, ambiguous_failure(source));
    }

    #[tokio::test]
    async fn renders_nested_instance_obligation_chain_with_source() {
        assert_diagnostic_snapshot! {r#"
            fn main() String {
                show([(value: Number) Number -> value])
            }
        "#};
    }

    #[tokio::test]
    async fn renders_orphan_impl_with_source() {
        let source = indoc::indoc! {r#"
            impl Show[Number] {
                fn show(value: Number) String { "number" }
            }
        "#};
        let diagnostic = compile_failures(source)
            .await
            .into_iter()
            .find(|diagnostic| diagnostic.message().starts_with("orphan implementation"))
            .expect("the orphan diagnostic is present");
        assert_rendered_diagnostic_snapshot!(source, diagnostic);
    }

    #[tokio::test]
    async fn renders_overlapping_impls_with_both_source_sites() {
        assert_diagnostic_snapshot! {r#"
            trait Display[a] {
                fn display(value: a) String
            }

            impl Display[a] {
                fn display(value: a) String { "any" }
            }

            impl Display[Number] {
                fn display(value: Number) String { "number" }
            }
        "#};
    }

    #[tokio::test]
    async fn renders_higher_kinded_impl_mismatch_with_source() {
        assert_diagnostic_snapshot! {r#"
            trait Mapper[f] {
                fn map(value: f[a], transform: fn(a) b) f[b]
            }

            impl Mapper[Number] {
                fn map(value: Number, transform: fn(a) b) Number { value }
            }
        "#};
    }

    #[tokio::test]
    async fn renders_associated_type_cycle_with_source() {
        assert_diagnostic_snapshot! {r#"
            enum Counter { Counter }
            trait Pair[i] {
                type Left
                type Right
            }
            impl Pair[Counter] {
                type Left = Right
                type Right = Left
            }
        "#};
    }

    #[tokio::test]
    async fn renders_superclass_cycle_with_source() {
        assert_diagnostic_snapshot! {r#"
            trait First[a] where a: Second {
                fn first(value: a) a
            }
            trait Second[a] where a: First {
                fn second(value: a) a
            }
        "#};
    }

    #[tokio::test]
    async fn renders_invalid_instance_termination_with_source() {
        assert_diagnostic_snapshot! {r#"
            trait Display[a] {
                fn display(value: a) String
            }
            impl Display[a] where a: Display {
                fn display(value: a) String { "recursive" }
            }
        "#};
    }

    #[test]
    fn renders_instance_search_cycle_with_source() {
        let source = indoc::indoc! {r#"
            trait Display[a] {
                fn display(value: a) String
            }
            fn main() { 42 }
        "#};
        assert_rendered_diagnostic_snapshot!(source, instance_cycle_failure(source));
    }

    #[test]
    fn renders_warning_with_source() {
        let source = indoc::indoc! {r#"
            fn main() {
                let unused = 1
                2
            }
        "#};
        let warning = alder_can::Warning {
            region: alder_region::Region::new(
                alder_region::Position::new(2, 9),
                alder_region::Position::new(2, 15),
            ),
            kind: alder_can::WarningKind::UnusedBinding { name: "unused" },
        };
        let diagnostic =
            crate::report::warning(Source::new("/project/src/main.ald", source), &warning);
        assert_rendered_diagnostic_snapshot!(source, diagnostic);
    }

    #[tokio::test]
    async fn test_compile_single_module() {
        let mem = InMemorySource::new();
        let uri = url("project/src/main.ald");
        mem.insert(uri.clone(), "pub fn main() { 42 }".to_string());

        let db = Arc::new(Mutex::new(Database::new(mem)));
        let modules = vec![uri];
        let graph = build_graph(db.clone(), &modules).await.unwrap();
        let result = build(db, &graph).await;

        assert_eq!(result.total, 1);
        assert_eq!(result.success, 1);
        assert!(result.is_success());
    }

    #[tokio::test]
    async fn build_mode_emits_an_artifact_for_each_successful_module() {
        let mem = InMemorySource::new();
        let uri = url("project/src/main.ald");
        mem.insert(
            uri.clone(),
            indoc::indoc! {r#"
                pub trait Inspect[a] {}
                impl Inspect[Number] {}
                pub fn main() { 42 }
            "#}
            .to_owned(),
        );

        let db = Arc::new(Mutex::new(Database::new(mem)));
        let modules = vec![uri.clone()];
        let graph = build_graph(db.clone(), &modules).await.unwrap();
        let result = build_with_mode(db, &graph, BuildMode::Build).await;

        assert!(result.is_success());
        assert_eq!(result.artifacts.len(), 1);
        assert_eq!(result.artifacts[&uri].module_id, "alder://app/main.mjs");
        assert_eq!(result.interfaces.len(), 1);
        assert_eq!(result.interfaces[0].instances.len(), 1);
        assert_eq!(
            result.interfaces[0].instances[0].source_uri.as_deref(),
            Some(uri.as_str())
        );
        assert_eq!(result.package_instance_indexes.len(), 1);
        assert_eq!(result.package_instance_indexes[0].modules.len(), 1);
        assert_eq!(result.package_instance_indexes[0].instances.len(), 1);
    }

    #[tokio::test]
    async fn test_compile_invalid_module() {
        let mem = InMemorySource::new();
        let uri = url("project/src/bad.ald");
        mem.insert(
            uri.clone(),
            "this is not valid alder syntax {{{{".to_string(),
        );

        let db = Arc::new(Mutex::new(Database::new(mem)));
        let modules = vec![uri];
        let graph = build_graph(db.clone(), &modules).await.unwrap();
        let result = build(db, &graph).await;

        assert_eq!(result.total, 1);
        assert_eq!(result.failed, 1);
    }

    #[tokio::test]
    async fn test_import_compiles_against_solved_interface() {
        let mem = InMemorySource::new();

        mem.insert(
            url("project/src/utils.ald"),
            "pub let helper = 1".to_string(),
        );

        mem.insert(
            url("project/src/main.ald"),
            "import ~/utils\npub fn main() { utils.helper }".to_string(),
        );

        let db = Arc::new(Mutex::new(Database::new(mem)));
        let modules = vec![url("project/src/utils.ald"), url("project/src/main.ald")];

        let graph = build_graph(db.clone(), &modules).await.unwrap();
        let result = build(db, &graph).await;

        // Utils is solved first; Main canonicalizes and type checks
        // against its interface.
        assert_eq!(result.total, 2);
        assert_eq!(result.success, 2);
        assert!(result.is_success());
    }

    #[test]
    fn trait_methods_support_every_import_form_across_interfaces() {
        let traits = url("project/src/traits.ald");
        let qualified = url("project/src/qualified.ald");
        let named = url("project/src/named.ald");
        let open = url("project/src/open.ald");
        let trait_qualified = url("project/src/trait_qualified.ald");
        let result = build_sync(
            vec![
                (
                    traits,
                    Ok(indoc::indoc! {r#"
                        pub trait Display[a] { fn display(value: a) String }
                        impl Display[Number] {
                            fn display(value: Number) String { "number" }
                        }
                    "#}.to_owned()),
                ),
                (
                    qualified.clone(),
                    Ok("import ~/traits\npub fn render() String { traits.display(1) }".to_owned()),
                ),
                (
                    named.clone(),
                    Ok("import ~/traits.{ display }\npub fn render() String { display(1) }".to_owned()),
                ),
                (
                    open.clone(),
                    Ok("import ~/traits.*\npub fn render() String { display(1) }".to_owned()),
                ),
                (
                    trait_qualified.clone(),
                    Ok("import ~/traits.{ Display }\npub fn render() String { Display::display(1) }".to_owned()),
                ),
            ],
            BuildMode::Check,
            BuildDependencies::default(),
        );

        for module in [qualified, named, open, trait_qualified] {
            assert!(
                matches!(result.modules[&module], ModuleResult::Success { .. }),
                "{module} did not compile: {:?}",
                result.modules[&module]
            );
        }
    }

    #[test]
    fn colliding_open_imported_trait_methods_render_their_source() {
        let source = indoc::indoc! {r#"
            import ~/first.*
            import ~/second.*
            pub fn main() String { render(1) }
        "#};
        let consumer = url("project/src/main.ald");
        let result = build_sync(
            vec![
                (
                    url("project/src/first.ald"),
                    Ok(indoc::indoc! {r#"
                        pub trait First[a] { fn render(value: a) String }
                        impl First[Number] {
                            fn render(value: Number) String { "first" }
                        }
                    "#}
                    .to_owned()),
                ),
                (
                    url("project/src/second.ald"),
                    Ok(indoc::indoc! {r#"
                        pub trait Second[a] { fn render(value: a) String }
                        impl Second[Number] {
                            fn render(value: Number) String { "second" }
                        }
                    "#}
                    .to_owned()),
                ),
                (consumer.clone(), Ok(source.to_owned())),
            ],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&consumer] else {
            panic!("colliding trait methods must fail the importing module")
        };
        assert_eq!(diagnostics.len(), 1);
        assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
    }

    #[tokio::test]
    async fn named_package_identity_applies_to_modules_local_imports_and_indexes() {
        let mem = InMemorySource::new();
        let model = url("package/src/model.ald");
        let implementation = url("package/src/instances.ald");
        mem.insert(
            model.clone(),
            "pub enum Token { Token }\npub trait Display[a] { fn display(value: a) String }"
                .to_owned(),
        );
        mem.insert(
            implementation.clone(),
            indoc::indoc! {r#"
                import ~/model.{ Token, Display }
                impl Display[Token] {
                    fn display(value: Token) String { "token" }
                }
            "#}
            .to_owned(),
        );
        let db = Arc::new(Mutex::new(Database::new(mem)));
        let modules = vec![model.clone(), implementation.clone()];
        let graph = build_graph(db.clone(), &modules).await.unwrap();
        let package = OwnedPackageId::Named {
            author: "vendor".to_owned(),
            project: "widgets".to_owned(),
        };
        let result = build_with_dependencies(
            db,
            &graph,
            BuildMode::Check,
            BuildDependencies {
                module_packages: BTreeMap::from([
                    (model, package.clone()),
                    (implementation, package.clone()),
                ]),
                ..BuildDependencies::default()
            },
        )
        .await;

        assert!(result.is_success(), "{:?}", result.modules);
        assert!(
            result
                .interfaces
                .iter()
                .all(|interface| interface.module.package == package)
        );
        assert_eq!(result.package_instance_indexes.len(), 1);
        assert_eq!(result.package_instance_indexes[0].package, package);
        assert_eq!(result.package_instance_indexes[0].instances.len(), 2);
    }

    #[tokio::test]
    async fn package_index_supplies_instances_from_an_unimported_dependency_module() {
        let api = dependency_interface(
            "pub enum Token { Token }\npub trait Display[a] { fn display(value: a) String }",
            &["api"],
            &[],
        );
        let instances = dependency_interface(
            indoc::indoc! {r#"
                import @vendor/widgets/api.{ Token, Display }
                impl Display[Token] {
                    fn display(value: Token) String { "token" }
                }
            "#},
            &["instances"],
            std::slice::from_ref(&api),
        );
        let index = PackageInstanceIndexFile::new(
            api.module.package.clone(),
            vec![api.module.clone(), instances.module.clone()],
            instances.instances.clone(),
        )
        .unwrap();

        let mem = InMemorySource::new();
        let consumer = url("project/src/main.ald");
        mem.insert(
            consumer.clone(),
            indoc::indoc! {r#"
                import @vendor/widgets/api.{ Token, display }
                pub fn render(value: Token) String { display(value) }
            "#}
            .to_owned(),
        );
        let db = Arc::new(Mutex::new(Database::new(mem)));
        let graph = build_graph(db.clone(), std::slice::from_ref(&consumer))
            .await
            .unwrap();
        let result = build_with_dependencies(
            db,
            &graph,
            BuildMode::Check,
            BuildDependencies {
                module_packages: BTreeMap::new(),
                interfaces: vec![api],
                package_instance_indexes: vec![index],
                ..BuildDependencies::default()
            },
        )
        .await;

        assert!(result.is_success(), "{:?}", result.modules[&consumer]);
    }

    #[tokio::test]
    async fn test_cross_module_type_error() {
        let mem = InMemorySource::new();

        mem.insert(
            url("project/src/utils.ald"),
            "pub let helper = 1".to_string(),
        );

        mem.insert(
            url("project/src/main.ald"),
            "import ~/utils\npub fn main() { utils.helper(\"not a function argument\") }"
                .to_string(),
        );

        let db = Arc::new(Mutex::new(Database::new(mem)));
        let modules = vec![url("project/src/utils.ald"), url("project/src/main.ald")];

        let graph = build_graph(db.clone(), &modules).await.unwrap();
        let result = build(db, &graph).await;

        // Utils.helper is a number, not a function: Main gets a type
        // error against the imported annotation.
        assert_eq!(result.total, 2);
        assert_eq!(result.success, 1);
        assert_eq!(result.failed, 1);
        assert!(matches!(
            result.modules[&url("project/src/main.ald")],
            ModuleResult::Failed { .. }
        ));
    }

    #[tokio::test]
    async fn trait_failures_are_rendered_without_internal_debug_names() {
        let mem = InMemorySource::new();
        let uri = url("project/src/main.ald");
        mem.insert(
            uri.clone(),
            "trait Display[a] { fn display(value: a) String }\nfn main() { display(1) }"
                .to_string(),
        );

        let db = Arc::new(Mutex::new(Database::new(mem)));
        let graph = build_graph(db.clone(), std::slice::from_ref(&uri))
            .await
            .unwrap();
        let result = build(db, &graph).await;
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("missing trait evidence must fail compilation");
        };
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].message(),
            "no implementation of `Display[Number]` was found"
        );
    }

    #[test]
    fn sibling_instances_are_visible_independent_of_input_order() {
        let model = url("project/src/model.ald");
        let consumer = url("project/src/a_consumer.ald");
        let implementation = url("project/src/z_impl.ald");
        let result = build_sync(
            vec![
                (
                    model,
                    Ok("pub enum Token { Token }\npub trait Display[a] { fn display(value: a) String }".to_owned()),
                ),
                (
                    consumer.clone(),
                    Ok("import ~/model.{ Token, display }\npub fn render(value: Token) String { display(value) }".to_owned()),
                ),
                (
                    implementation,
                    Ok("import ~/model.{ Token, Display }\nimpl Display[Token] { fn display(value: Token) String { \"token\" } }".to_owned()),
                ),
            ],
            BuildMode::Check,
            BuildDependencies::default(),
        );

        assert!(result.is_success(), "{:?}", result.modules[&consumer]);
    }

    #[test]
    fn package_coherence_uses_headers_from_modules_with_broken_bodies() {
        let model = url("project/src/model.ald");
        let first = url("project/src/first.ald");
        let second = url("project/src/second.ald");
        let result = build_sync(
            vec![
                (
                    model,
                    Ok("pub enum Token { Token }\npub trait Display[a] { fn display(value: a) String }".to_owned()),
                ),
                (
                    first.clone(),
                    Ok(indoc::indoc! {r#"
                        import ~/model.{ Token, Display }
                        impl Display[Token] { fn display(value: Token) String { "first" } }
                        fn broken() { missing_first }
                    "#}.to_owned()),
                ),
                (
                    second.clone(),
                    Ok(indoc::indoc! {r#"
                        import ~/model.{ Token, Display }
                        impl Display[Token] { fn display(value: Token) String { "second" } }
                        fn broken() { missing_second }
                    "#}.to_owned()),
                ),
            ],
            BuildMode::Check,
            BuildDependencies::default(),
        );

        for module in [first, second] {
            let ModuleResult::Failed { diagnostics } = &result.modules[&module] else {
                panic!("overlapping package instances must fail every defining module");
            };
            assert!(diagnostics.iter().any(|diagnostic| {
                diagnostic.message() == "overlapping implementations of `Display` are not allowed"
            }));
        }
    }

    /// Unannotated mutually recursive exports used from another module:
    /// Elm 0.19.1 crashes on this exact shape ("Map.!: given key is not an
    /// element in the map") because `getVarNames`' visit marks persist
    /// across `toAnnotation` calls, leaving `pong`'s `Forall` empty. Alder
    /// deliberately fixes that (see `alder-solve/src/annotation.rs`).
    #[tokio::test]
    async fn test_cross_module_mutual_recursion() {
        let mem = InMemorySource::new();

        mem.insert(
            url("project/src/utils.ald"),
            r#"
pub fn ping(x) { pong(x) }
pub fn pong(x) { ping(x) }
"#
            .to_string(),
        );

        mem.insert(
            url("project/src/main.ald"),
            r#"
import ~/utils
pub fn main() { utils.pong(1) }
"#
            .to_string(),
        );

        let db = Arc::new(Mutex::new(Database::new(mem)));
        let modules = vec![url("project/src/utils.ald"), url("project/src/main.ald")];

        let graph = build_graph(db.clone(), &modules).await.unwrap();
        let result = build(db, &graph).await;

        assert_eq!(result.total, 2);
        assert_eq!(result.success, 2);
        assert!(result.is_success());
    }
}
