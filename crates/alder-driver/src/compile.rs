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
    /// Build or package validation prevented body compilation. The cause is
    /// reported at build level or on another module; no artifact is produced.
    Blocked,
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
    /// Build-level errors without an owning source module, such as invalid
    /// stored dependency trait indexes or missing identity metadata. These do
    /// not carry fabricated spans.
    pub diagnostics: Vec<Diagnostic>,
    /// Results for each module.
    pub modules: HashMap<Url, ModuleResult>,

    /// Total number of modules processed.
    pub total: usize,

    /// Number of successful compilations.
    pub success: usize,

    /// Number of modules that did not compile, including blocked modules.
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
    /// Explicit owning package for every source module in this build.
    pub module_packages: BTreeMap<Url, OwnedPackageId>,
    /// Explicit paths relative to each module's actual package source root.
    /// Every source module needs both entries, including application modules.
    pub module_paths: BTreeMap<Url, Vec<String>>,
    pub interfaces: Vec<InterfaceFile>,
    pub package_instance_indexes: Vec<PackageInstanceIndexFile>,
}

impl BuildResult {
    /// Check if the build was completely successful.
    pub fn is_success(&self) -> bool {
        self.failed == 0 && self.diagnostics.is_empty()
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

fn duplicate_sources(
    sources: &[(Url, Result<String, String>)],
    identities: &BTreeMap<Url, OwnedModuleId>,
) -> Option<BuildResult> {
    let mut origins = BTreeMap::<OwnedModuleId, Vec<&Url>>::new();
    for (uri, identity) in identities {
        origins.entry(identity.clone()).or_default().push(uri);
    }
    let mut duplicates = HashMap::new();
    for (identity, uris) in origins.iter().filter(|(_, uris)| uris.len() > 1) {
        let source_for = |uri: &Url| {
            Source::new(
                uri.path(),
                sources
                    .iter()
                    .find(|(candidate, _)| candidate == uri)
                    .and_then(|(_, source)| source.as_ref().ok())
                    .cloned()
                    .unwrap_or_default(),
            )
        };
        let mut diagnostic = Diagnostic::error(source_for(uris[0]),
            format!("multiple files define module `{}`", identity.path.join("/")))
            .with_code("alder::driver::duplicate_module")
            .with_primary_label(alder_region::Region::one(), "one definition is in this file")
            .with_help("Keep only one source file for this module: a name.ald file and name/mod.ald define the same module.");
        for uri in &uris[1..] {
            diagnostic = diagnostic.with_related(
                Diagnostic::error(source_for(uri), "this file defines the same module")
                    .with_primary_label(alder_region::Region::one(), "conflicting module source"),
            );
        }
        duplicates.insert(
            uris[0].clone(),
            ModuleResult::Failed {
                diagnostics: vec![diagnostic],
            },
        );
    }
    if !duplicates.is_empty() {
        return Some(BuildResult {
            diagnostics: vec![],
            total: duplicates.len(),
            failed: duplicates.len(),
            success: 0,
            modules: duplicates,
            warnings: vec![],
            artifacts: HashMap::new(),
            interfaces: vec![],
            package_instance_indexes: vec![],
        });
    }
    None
}

/// Compile graph sources with explicit package and source-relative identity
/// metadata. Source fetching is async; compilation runs on the blocking pool.
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
    let identities = match source_identities(sources.iter().map(|(uri, _)| uri), &dependencies) {
        Ok(identities) => identities,
        Err(error) => {
            return BuildResult {
                diagnostics: vec![
                    Diagnostic::error(Source::new("build", ""), error.to_string())
                        .with_code("alder::driver::missing_module_identity"),
                ],
                modules: sources
                    .iter()
                    .map(|(uri, _)| (uri.clone(), ModuleResult::Blocked))
                    .collect(),
                total: sources.len(),
                success: 0,
                failed: sources.len(),
                warnings: vec![],
                artifacts: HashMap::new(),
                interfaces: vec![],
                package_instance_indexes: vec![],
            };
        }
    };
    if let Some(failure) = duplicate_sources(&sources, &identities) {
        return failure;
    }
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

    let total = sources.len();
    let mut solved_interfaces = vec![false; total];

    loop {
        let mut progress = false;
        for index in 0..total {
            if solved_interfaces[index] {
                continue;
            }
            let (uri, source) = &sources[index];
            let (_, discovered) = compile_module(
                uri,
                source,
                &identities[uri],
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
        let (output, discovered) = compile_module(
            uri,
            source,
            &identities[uri],
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
        diagnostics: vec![],
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
    identity: &OwnedModuleId,
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

    let home = ModuleId {
        package: hydrate_package_id(&module_arena, &identity.package),
        path: module_arena.alloc_slice_fill_iter(
            identity
                .path
                .iter()
                .map(|part| module_arena.alloc_str(part) as &str),
        ),
    };
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
        .map(|result| alder_can::headers_from_module(&module_arena, result.module, interfaces))
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
        let interface =
            alder_can::headers_from_module(&module_arena, can_result.module, interfaces);
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

    let module_interface = alder_can::from_module(
        &module_arena,
        can_result.module,
        &solved.annotations,
        interfaces,
    );
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
                Ok(mut artifact) => {
                    artifact.source_path = uri.to_file_path().ok();
                    artifact.source_text = Some(source.clone());
                    artifact.extern_regions = can_result
                        .module
                        .items
                        .iter()
                        .filter_map(|item| {
                            if let alder_ast::ItemKind::Extern(alder_ast::ExternDecl::Fn {
                                module,
                                ..
                            }) = item.value.kind
                            {
                                Some(((*module).to_owned(), item.region))
                            } else {
                                None
                            }
                        })
                        .collect();
                    Some(artifact)
                }
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

fn source_identities<'u>(
    uris: impl IntoIterator<Item = &'u Url>,
    dependencies: &BuildDependencies,
) -> Result<BTreeMap<Url, OwnedModuleId>, DriverError> {
    uris.into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|uri| {
            let (Some(package), Some(path)) = (
                dependencies.module_packages.get(uri),
                dependencies.module_paths.get(uri),
            ) else {
                return Err(DriverError::MissingModuleIdentity { uri: uri.clone() });
            };
            Ok((
                uri.clone(),
                OwnedModuleId {
                    package: package.clone(),
                    path: path.clone(),
                },
            ))
        })
        .collect()
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
            let parts: Vec<_> = path.segments.iter().map(|segment| segment.value).collect();
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

/// Resolve graph edges with the same package identities and source-relative
/// paths used by canonicalization. Ambiguous identities are diagnosed by build
/// preflight before any interfaces or code are produced.
pub async fn build_graph_with_dependencies(
    db: Arc<Mutex<Database>>,
    modules: &[Url],
    dependencies: &BuildDependencies,
) -> Result<DepGraph, DriverError> {
    let mut graph = DepGraph::new();
    let identities = source_identities(modules, dependencies)?;
    let mut origins = BTreeMap::<OwnedModuleId, Vec<Url>>::new();
    for (uri, identity) in &identities {
        origins
            .entry(identity.clone())
            .or_default()
            .push(uri.clone());
    }

    for uri in modules {
        // Parse module to get imports
        let source = {
            let mut db = db.lock().await;
            db.source(uri).await?.to_string()
        };

        let imports = extract_imports(&source, &identities[uri].package, &origins);
        graph.add_module(uri.clone(), imports);
    }

    graph.compute_order()?;
    Ok(graph)
}

/// Extract import URIs from source code.
///
/// Parse imports and look up their exact package-qualified identities.
fn extract_imports(
    source: &str,
    package: &OwnedPackageId,
    known_modules: &BTreeMap<OwnedModuleId, Vec<Url>>,
) -> Vec<Url> {
    let mut imports = Vec::new();

    // Parse to get imports
    let bump = Bump::new();
    let src = bump.alloc_str(source);
    let mut parser = alder_parse::Parser::new(&bump, src.as_bytes());

    if let Ok(module) = parser.module() {
        for import in module.imports() {
            if let Some(uri) = resolve_source_import(import, package, known_modules) {
                imports.push(uri);
            }
        }
    }

    imports
}

/// Resolve an import name to a module URI.
///
fn resolve_source_import(
    import: &alder_source::Import<'_>,
    current_package: &OwnedPackageId,
    known_modules: &BTreeMap<OwnedModuleId, Vec<Url>>,
) -> Option<Url> {
    let package = match import.path.value.root {
        alder_source::ModuleRoot::Local(_) => current_package.clone(),
        alder_source::ModuleRoot::Package { author, package } => OwnedPackageId::Named {
            author: author.value.to_owned(),
            project: package.value.to_owned(),
        },
    };
    let path = import
        .path
        .value
        .segments
        .iter()
        .map(|segment| segment.value.to_owned())
        .collect::<Vec<_>>();
    let candidates = known_modules.get(&OwnedModuleId { package, path })?;
    (candidates.len() == 1).then(|| candidates[0].clone())
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

    /// Source-only fixtures have one explicit `src` boundary. Tests of other
    /// layouts supply their own paths. This convenience is not a driver API:
    /// production callers must provide both identity maps for every module.
    fn fixture_dependencies<'u>(
        uris: impl IntoIterator<Item = &'u Url>,
        mut dependencies: BuildDependencies,
    ) -> BuildDependencies {
        for uri in uris {
            dependencies
                .module_packages
                .entry(uri.clone())
                .or_insert(OwnedPackageId::Application);
            dependencies
                .module_paths
                .entry(uri.clone())
                .or_insert_with(|| {
                    let (_, relative) = uri
                        .path()
                        .split_once("/src/")
                        .expect("source-only fixture needs a src root or an explicit module path");
                    assert!(
                        !relative.contains("/src/"),
                        "ambiguous fixture needs an explicit path"
                    );
                    let mut path = relative
                        .strip_suffix(".ald")
                        .unwrap()
                        .split('/')
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    if path.last().is_some_and(|part| part == "mod") {
                        path.pop();
                    }
                    path
                });
        }
        dependencies
    }

    fn build_fixture_sync(
        sources: Vec<(Url, Result<String, String>)>,
        mode: BuildMode,
        dependencies: BuildDependencies,
    ) -> BuildResult {
        let dependencies = fixture_dependencies(sources.iter().map(|(uri, _)| uri), dependencies);
        super::build_sync(sources, mode, dependencies)
    }

    async fn fixture_graph_with_dependencies(
        db: Arc<Mutex<Database>>,
        modules: &[Url],
        dependencies: &BuildDependencies,
    ) -> Result<DepGraph, DriverError> {
        let dependencies = fixture_dependencies(modules, dependencies.clone());
        super::build_graph_with_dependencies(db, modules, &dependencies).await
    }

    async fn fixture_graph(
        db: Arc<Mutex<Database>>,
        modules: &[Url],
    ) -> Result<DepGraph, DriverError> {
        fixture_graph_with_dependencies(db, modules, &BuildDependencies::default()).await
    }

    async fn build_fixture_with_dependencies(
        db: Arc<Mutex<Database>>,
        graph: &DepGraph,
        mode: BuildMode,
        dependencies: BuildDependencies,
    ) -> BuildResult {
        let dependencies = fixture_dependencies(&graph.order, dependencies);
        super::build_with_dependencies(db, graph, mode, dependencies).await
    }

    async fn build_fixture_with_mode(
        db: Arc<Mutex<Database>>,
        graph: &DepGraph,
        mode: BuildMode,
    ) -> BuildResult {
        build_fixture_with_dependencies(db, graph, mode, BuildDependencies::default()).await
    }

    async fn build_fixture(db: Arc<Mutex<Database>>, graph: &DepGraph) -> BuildResult {
        build_fixture_with_mode(db, graph, BuildMode::Check).await
    }

    #[tokio::test]
    async fn graph_requires_explicit_module_identity() {
        let uri = url("checkout/src/nested/src/main.ald");
        let db = Arc::new(Mutex::new(Database::new(InMemorySource::with_files([(
            uri.clone(),
            "pub fn answer() Number { 42 }".to_owned(),
        )]))));
        let result = super::build_graph_with_dependencies(
            db,
            std::slice::from_ref(&uri),
            &BuildDependencies::default(),
        )
        .await;
        assert!(
            matches!(result, Err(DriverError::MissingModuleIdentity { uri: missing }) if missing == uri),
            "URI spelling cannot supply a module identity"
        );
    }

    #[test]
    fn incomplete_module_identity_cannot_publish_artifacts() {
        let uri = url("checkout/src/nested/src/main.ald");
        for dependencies in [
            BuildDependencies::default(),
            BuildDependencies {
                module_packages: BTreeMap::from([(uri.clone(), OwnedPackageId::Application)]),
                ..BuildDependencies::default()
            },
            BuildDependencies {
                module_paths: BTreeMap::from([(uri.clone(), vec!["main".to_owned()])]),
                ..BuildDependencies::default()
            },
        ] {
            let result = super::build_sync(
                vec![(uri.clone(), Ok("pub fn answer() Number { 42 }".to_owned()))],
                BuildMode::Build,
                dependencies,
            );
            assert!(
                !result.is_success(),
                "both package and source-relative path are required"
            );
            assert!(result.artifacts.is_empty());
            assert!(result.interfaces.is_empty());
            assert!(result.package_instance_indexes.is_empty());
            assert_eq!(result.diagnostics.len(), 1);
            assert!(result.diagnostics[0].message().contains(uri.as_str()));
            assert!(miette::Diagnostic::labels(&result.diagnostics[0]).is_none());
            assert!(matches!(result.modules[&uri], ModuleResult::Blocked));
        }
    }

    #[test]
    fn missing_identity_diagnostic_does_not_depend_on_discovery_order() {
        let first = url("first/input.ald");
        let second = url("second/input.ald");
        for uris in [
            [first.clone(), second.clone()],
            [second.clone(), first.clone()],
        ] {
            let result = super::build_sync(
                uris.into_iter()
                    .map(|uri| (uri, Ok("pub fn answer() Number { 42 }".to_owned())))
                    .collect(),
                BuildMode::Build,
                BuildDependencies::default(),
            );
            assert!(!result.is_success());
            assert_eq!(result.diagnostics.len(), 1);
            assert!(result.diagnostics[0].message().contains(first.as_str()));
        }
    }

    #[test]
    fn explicit_module_identity_is_independent_of_source_uri_spelling() {
        let identity = OwnedModuleId {
            package: OwnedPackageId::Application,
            path: vec!["logical".to_owned(), "entry".to_owned()],
        };
        let mut previous = None;
        for uri in [
            url("checkout/src/nested/src/input.ald"),
            Url::parse("memory:///embedded/input").unwrap(),
        ] {
            let result = super::build_sync(
                vec![(uri.clone(), Ok("pub fn answer() Number { 42 }".to_owned()))],
                BuildMode::Build,
                BuildDependencies {
                    module_packages: BTreeMap::from([(uri.clone(), identity.package.clone())]),
                    module_paths: BTreeMap::from([(uri.clone(), identity.path.clone())]),
                    ..BuildDependencies::default()
                },
            );
            assert!(result.is_success(), "{:?}", result.modules);
            assert_eq!(result.interfaces.len(), 1);
            assert_eq!(result.interfaces[0].module, identity);
            let artifact = &result.artifacts[&uri];
            assert_eq!(artifact.module_id, "alder://app/logical/entry.mjs");
            if let Some(previous) = &previous {
                assert_eq!(&artifact.code(), previous);
            }
            previous = Some(artifact.code());
        }
    }

    fn url(path: &str) -> Url {
        Url::parse(&format!("file:///{}", path)).unwrap()
    }

    #[test]
    fn stored_async_contracts_preserve_layers_and_captured_state() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                pub async fn identity(value: a) a { value }
                pub async fn nested() Task[Number] { async { 42 } }
                let operation = value -> value
                pub fn deferred(value) { async { operation(value) } }
                pub fn specialize() { async { operation = (value: Number) -> value + 1 } }
            "#},
            &[],
            &[],
        );
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ identity, nested, deferred, specialize }
            pub async fn main() Number {
                let number = identity(42).await
                let text = identity("hello").await
                let inner: Task[Number] = nested().await
                specialize().await
                deferred(number).await + inner.await + String.length(text)
            }
        "#};
        let result = build_fixture_sync(
            vec![(url("project/src/main.ald"), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies {
                interfaces: vec![stored.clone()],
                ..BuildDependencies::default()
            },
        );
        assert!(result.is_success(), "{:#?}", result.modules);
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ deferred, specialize }
            pub async fn main() String {
                specialize().await
                deferred("wrong").await
            }
        "#};
        let result = build_fixture_sync(
            vec![(url("project/src/main.ald"), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies {
                interfaces: vec![stored],
                ..BuildDependencies::default()
            },
        );
        assert!(!result.is_success());
        assert!(result.artifacts.is_empty());
        assert!(result.interfaces.is_empty());
        let ModuleResult::Failed { diagnostics } = &result.modules[&url("project/src/main.ald")]
        else {
            panic!("async capture cannot publish a polymorphic replaceable binding")
        };
        assert_eq!(diagnostics.len(), 1);
        assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
    }

    #[test]
    fn stored_assignment_contracts_preserve_safe_and_restricted_functions() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                pub let identity = value -> value
                let operation = value -> value
                pub fn forward(value) { operation(value) }
                pub fn writer() { () -> { operation = (value: Number) -> value + 1 } }
            "#},
            &[],
            &[],
        );
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ identity, forward, writer }
            pub fn main() Number {
                let number = identity(42)
                let text = identity("hello")
                writer()()
                forward(number) + String.length(text)
            }
        "#};
        let result = build_fixture_sync(
            vec![(url("project/src/main.ald"), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies {
                interfaces: vec![stored.clone()],
                ..BuildDependencies::default()
            },
        );
        assert!(result.is_success(), "{:#?}", result.modules);
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ forward, writer }
            pub fn main() String {
                writer()()
                forward("wrong")
            }
        "#};
        let result = build_fixture_sync(
            vec![(url("project/src/main.ald"), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies {
                interfaces: vec![stored],
                ..BuildDependencies::default()
            },
        );
        assert!(!result.is_success());
        assert!(result.artifacts.is_empty());
        assert!(result.interfaces.is_empty());
        let ModuleResult::Failed { diagnostics } = &result.modules[&url("project/src/main.ald")]
        else {
            panic!("consumer cannot generalize the producer's replaceable state")
        };
        assert_eq!(diagnostics.len(), 1);
        assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
    }

    #[test]
    fn stored_tag_functions_check_consumer_contracts() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                pub fn identity(parts: Array[String], value: a) a { value }
                pub fn shown(parts: Array[String], value: a) String where a: Show { show(value) }
                pub fn number(parts: Array[String], value: Number) Number { value }
            "#},
            &[],
            &[],
        );
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ identity, shown }
            pub fn main() Number {
                let number = identity`${42}`
                let text = identity`${"hello"}`
                let rendered = shown`${number}`
                number + String.length(text) + String.length(rendered)
            }
        "#};
        let result = build_fixture_sync(
            vec![(url("project/src/main.ald"), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies {
                interfaces: vec![stored.clone()],
                ..BuildDependencies::default()
            },
        );
        assert!(result.is_success(), "{:#?}", result.modules);
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ number }
            pub fn main() Number {
                number`value: ${"wrong"}`
            }
        "#};
        let result = build_fixture_sync(
            vec![(url("project/src/main.ald"), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies {
                interfaces: vec![stored],
                ..BuildDependencies::default()
            },
        );
        assert!(!result.is_success());
        assert!(result.artifacts.is_empty());
        assert!(result.interfaces.is_empty());
        let ModuleResult::Failed { diagnostics } = &result.modules[&url("project/src/main.ald")]
        else {
            panic!("invalid tag consumer must fail")
        };
        assert_eq!(diagnostics.len(), 1);
        assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
    }

    #[test]
    fn stored_generic_overlays_check_independent_consumer_calls() {
        let producer = dependency_interface(
            "pub fn merge(left, right) { { ..left, ..right } }",
            &[],
            &[],
        );
        assert!(!producer.values[0].scheme.record_overlays.is_empty());
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let copied = {
            let destination = Bump::new();
            let interface = {
                let source = Bump::new();
                alder_ast::copy_interface(&destination, &stored.hydrate(&source))
            };
            InterfaceFile::dehydrate(&interface).unwrap()
        };
        drop(stored);
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ merge }
            pub fn main() Number {
                let first = merge({ x: 20 }, { y: 22 })
                let second = merge({ value: false }, { value: "text" })
                first.x + first.y + String.length(second.value)
            }
        "#};
        let result = build_fixture_sync(
            vec![(url("project/src/main.ald"), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies {
                interfaces: vec![copied.clone()],
                ..BuildDependencies::default()
            },
        );
        assert!(result.is_success(), "{:#?}", result.modules);
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ merge }
            pub fn main() Number {
                merge({ value: 42 }, { value: "wrong" }).value
            }
        "#};
        let result = build_fixture_sync(
            vec![(url("project/src/main.ald"), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies {
                interfaces: vec![copied],
                ..BuildDependencies::default()
            },
        );
        assert!(!result.is_success(), "invalid stored overlay was accepted");
        assert!(result.artifacts.is_empty());
        assert!(result.interfaces.is_empty());
        let ModuleResult::Failed { diagnostics } = &result.modules[&url("project/src/main.ald")]
        else {
            panic!("invalid consumer must fail")
        };
        assert_eq!(diagnostics.len(), 1);
        assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
    }

    #[test]
    fn package_root_reexports_survive_owned_storage_without_leaf_interfaces() {
        let leaf = dependency_interface(
            indoc::indoc! {r#"
            pub type Envelope[a] = { value: a }
            pub fn wrap(value: a) Envelope[a] { { value } }
        "#},
            &["leaf"],
            &[],
        );
        let root = dependency_interface("pub import ~/leaf.*", &[], &[leaf]);
        let bytes = bincode::serialize(&root).expect("serialize root interface");
        let stored: InterfaceFile =
            bincode::deserialize(&bytes).expect("deserialize root interface");
        assert_eq!(stored, root);
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ Envelope, wrap }
            pub fn main() Number {
                let wrapped: Envelope[Number] = wrap(42)
                wrapped.value
            }
        "#};
        let result = build_fixture_sync(
            vec![(url("project/src/main.ald"), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies {
                interfaces: vec![stored],
                ..BuildDependencies::default()
            },
        );
        assert!(
            result.is_success(),
            "stored re-export failed: {:#?}",
            result.modules
        );
    }

    #[test]
    fn wildcard_reexport_publishes_only_public_declarations() {
        let leaf = indoc::indoc! {r#"
            fn secret() Number { 42 }
            type HiddenAlias = Number
            enum HiddenEnum { Hidden }
            trait HiddenTrait[a] { fn hidden_method(value: a) Number }
            pub fn answer() Number { secret() }
            pub type PublicAlias = Number
            pub enum PublicEnum { Visible }
            pub trait PublicTrait[a] { fn public_method(value: a) Number }
        "#};
        let leaf = dependency_interface(leaf, &["leaf"], &[]);
        let facade = dependency_interface("pub import ~/leaf.*", &["facade"], &[leaf]);
        assert_eq!(
            facade
                .values
                .iter()
                .map(|value| value.exported_as.as_str())
                .collect::<Vec<_>>(),
            ["answer", "public_method"]
        );
        assert_eq!(
            facade
                .types
                .iter()
                .map(|typ| typ.exported_as.as_str())
                .collect::<Vec<_>>(),
            ["PublicAlias", "PublicEnum"]
        );
        assert_eq!(
            facade
                .traits
                .iter()
                .map(|trait_| trait_.exported_as.as_str())
                .collect::<Vec<_>>(),
            ["PublicTrait"]
        );
        assert!(facade.private_names.is_empty());
        assert!(
            facade.instances.is_empty(),
            "instances must not become facade-owned"
        );
        let source = indoc::indoc! {r#"
            import @vendor/widgets/facade.{ answer, PublicAlias, PublicEnum, PublicTrait }
            pub fn read(value: a) Number where a: PublicTrait { PublicTrait::public_method(value) }
            pub fn main() PublicAlias {
                let visible = PublicEnum::Visible
                answer()
            }
        "#};
        let checked = build_fixture_sync(
            vec![(url("project/src/main.ald"), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies {
                interfaces: vec![facade.clone()],
                ..BuildDependencies::default()
            },
        );
        assert!(
            checked.is_success(),
            "public declarations must remain usable: {checked:?}"
        );
        for source in [
            "import @vendor/widgets/facade.{ secret }",
            "import @vendor/widgets/facade.{ HiddenAlias }",
            "import @vendor/widgets/facade.{ HiddenEnum }",
            "import @vendor/widgets/facade.{ HiddenTrait }",
            "import @vendor/widgets/facade.{ hidden_method }",
        ] {
            let consumer = url("project/src/main.ald");
            let result = build_fixture_sync(
                vec![(consumer.clone(), Ok(source.to_owned()))],
                BuildMode::Build,
                BuildDependencies {
                    interfaces: vec![facade.clone()],
                    ..BuildDependencies::default()
                },
            );
            assert!(!result.is_success(), "private import accepted: {source}");
            assert!(result.artifacts.is_empty());
            assert!(
                !result
                    .interfaces
                    .iter()
                    .any(|interface| interface.module.path == ["main"])
            );
            let ModuleResult::Failed { diagnostics } = &result.modules[&consumer] else {
                panic!("private import must fail")
            };
            assert_eq!(diagnostics.len(), 1);
            assert!(
                diagnostics[0].to_string().contains("does not export"),
                "unexpected failure: {diagnostics:?}"
            );
        }
    }

    #[test]
    fn wildcard_reexport_collision_rejects_publication() {
        let source = indoc::indoc! {r#"
            pub import ~/left.*
            pub import ~/right.*
        "#};
        let facade = url("project/src/facade.ald");
        let result = build_fixture_sync(
            vec![
                (
                    url("project/src/left.ald"),
                    Ok("pub fn answer() Number { 1 }".to_owned()),
                ),
                (
                    url("project/src/right.ald"),
                    Ok("pub fn answer() String { \"two\" }".to_owned()),
                ),
                (facade.clone(), Ok(source.to_owned())),
            ],
            BuildMode::Build,
            BuildDependencies::default(),
        );
        assert!(!result.is_success());
        assert!(!result.artifacts.contains_key(&facade));
        assert!(
            !result
                .interfaces
                .iter()
                .any(|interface| interface.module.path == ["facade"])
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&facade] else {
            panic!("ambiguous facade must fail")
        };
        assert_eq!(diagnostics.len(), 1);
        assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
    }

    #[test]
    fn named_reexport_cannot_publish_a_private_value() {
        let source = "pub import ~/leaf.{ secret }";
        let facade = url("project/src/facade.ald");
        let result = build_fixture_sync(
            vec![
                (
                    url("project/src/leaf.ald"),
                    Ok("fn secret() Number { 42 }".to_owned()),
                ),
                (facade.clone(), Ok(source.to_owned())),
            ],
            BuildMode::Build,
            BuildDependencies::default(),
        );
        assert!(!result.is_success());
        assert!(!result.artifacts.contains_key(&facade));
        assert!(
            !result
                .interfaces
                .iter()
                .any(|interface| interface.module.path == ["facade"])
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&facade] else {
            panic!("private re-export must fail")
        };
        assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
    }

    #[test]
    fn renamed_trait_method_does_not_bind_its_original_name() {
        let source = indoc::indoc! {r#"
            import ~/leaf.{ obtain as read }
            pub fn main() Number { obtain(42) }
        "#};
        let diagnostic = trait_import_failure(source);
        assert_rendered_diagnostic_snapshot!(source, diagnostic);
    }

    #[test]
    fn renamed_trait_method_collides_at_its_local_binding() {
        let source = "pub import ~/leaf.{ plain as read, obtain as read }";
        let diagnostic = trait_import_failure(source);
        assert_rendered_diagnostic_snapshot!(source, diagnostic);
    }

    fn trait_import_failure(source: &str) -> Diagnostic {
        let consumer = url("project/src/main.ald");
        let leaf = indoc::indoc! {r#"
            pub trait Probe[a] { fn obtain(value: a) Number }
            impl Probe[Number] { fn obtain(value: Number) Number { value } }
            pub fn plain(value: Number) Number { value }
        "#};
        let result = build_fixture_sync(
            vec![
                (url("project/src/leaf.ald"), Ok(leaf.to_owned())),
                (consumer.clone(), Ok(source.to_owned())),
            ],
            BuildMode::Build,
            BuildDependencies::default(),
        );
        assert!(!result.is_success());
        assert!(!result.artifacts.contains_key(&consumer));
        assert!(
            !result
                .interfaces
                .iter()
                .any(|interface| interface.module.path == ["main"])
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&consumer] else {
            panic!("invalid method import must fail")
        };
        assert_eq!(diagnostics.len(), 1);
        diagnostics[0].clone()
    }

    #[test]
    fn wildcard_reexport_publishes_values_to_consumers() {
        assert_value_reexport("pub import ~/leaf.*");
    }

    #[test]
    fn aliased_reexport_publishes_values_to_consumers() {
        assert_value_reexport("pub import ~/leaf.{ answer as renamed, answer }");
    }

    #[test]
    fn named_reexport_publishes_values_to_consumers() {
        assert_value_reexport("pub import ~/leaf.{ answer }");
    }

    fn assert_value_reexport(facade: &str) {
        let leaf = url("project/src/leaf.ald");
        let facade_uri = url("project/src/facade.ald");
        let consumer = url("project/src/main.ald");
        let source = indoc::indoc! {r#"
            import ~/facade
            pub fn main() Number { facade.answer() }
        "#};
        let result = build_fixture_sync(
            vec![
                (leaf, Ok("pub fn answer() Number { 42 }".to_owned())),
                (facade_uri.clone(), Ok(facade.to_owned())),
                (consumer.clone(), Ok(source.to_owned())),
            ],
            BuildMode::Build,
            BuildDependencies::default(),
        );
        assert!(
            result.is_success(),
            "public re-export failed: {:#?}",
            result.modules
        );
        assert_eq!(result.artifacts.len(), 3);
        assert_eq!(
            result.artifacts[&facade_uri].dependencies,
            ["alder://app/leaf.mjs"],
            "a facade retains its initialization dependency without local value uses"
        );
        assert_eq!(
            result.artifacts[&consumer].dependencies,
            ["alder://app/facade.mjs", "alder://app/leaf.mjs"],
            "direct owner references must not erase the imported facade"
        );
        let leaf_interface = result
            .interfaces
            .iter()
            .find(|interface| interface.module.path == ["leaf"])
            .expect("leaf interface");
        let facade_interface = result
            .interfaces
            .iter()
            .find(|interface| interface.module.path == ["facade"])
            .expect("facade interface");
        assert_eq!(
            facade_interface.values.len(),
            if facade.contains("renamed") { 2 } else { 1 }
        );
        for forwarded in &facade_interface.values {
            assert_eq!(forwarded.identity, leaf_interface.values[0].identity);
            assert_eq!(forwarded.scheme, leaf_interface.values[0].scheme);
        }
    }

    #[test]
    fn unimplemented_markup_cannot_produce_executable_artifact() {
        let source = indoc::indoc! {r#"
            pub fn view() {
                <div>@if true { <span>visible</span> }</div>
            }
        "#};
        assert_rendered_diagnostic_snapshot!(source, unavailable_codegen(source));
    }

    #[test]
    fn unimplemented_style_cannot_produce_executable_artifact() {
        let source = "pub fn card() { style { padding: 16px } }";
        assert_rendered_diagnostic_snapshot!(source, unavailable_codegen(source));
    }

    #[test]
    fn unimplemented_state_cannot_produce_executable_artifact() {
        let source = indoc::indoc! {r#"
            pub fn main() {
                let count = state(0)
                count += 1
                count
            }
        "#};
        assert_rendered_diagnostic_snapshot!(source, unavailable_codegen(source));
    }

    #[test]
    fn unimplemented_component_cannot_produce_executable_artifact() {
        let source = "pub component Counter() { <div /> }";
        assert_rendered_diagnostic_snapshot!(source, unavailable_codegen(source));
    }

    fn unavailable_codegen(source: &str) -> Diagnostic {
        let uri = url("project/src/main.ald");
        let checked = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        assert!(
            checked.is_success(),
            "provisional checking must still work: {checked:?}"
        );
        let mut diagnostic = None;
        for mode in [BuildMode::Build, BuildMode::Test] {
            let result = build_fixture_sync(
                vec![(uri.clone(), Ok(source.to_owned()))],
                mode,
                BuildDependencies::default(),
            );
            assert!(
                !result.is_success(),
                "unavailable runtime form emitted executable code"
            );
            assert!(result.artifacts.is_empty());
            let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
                panic!("expected a diagnostic")
            };
            diagnostic = Some(diagnostics[0].clone());
        }
        diagnostic.unwrap()
    }

    #[test]
    fn unimplemented_query_cannot_produce_executable_artifact() {
        let source = indoc::indoc! {r#"
            table users {}
            pub fn load() { query { select * from users } }
        "#};
        let uri = url("project/src/main.ald");
        let checked = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        assert!(
            checked.is_success(),
            "query syntax remains available for checking"
        );
        assert!(checked.artifacts.is_empty());
        let tested = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Test,
            BuildDependencies::default(),
        );
        assert!(!tested.is_success());
        assert!(tested.artifacts.is_empty());
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Build,
            BuildDependencies::default(),
        );
        assert!(
            !result.is_success(),
            "an unavailable query must not emit a runtime stub"
        );
        assert!(result.artifacts.is_empty());
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("expected an unavailable-query diagnostic")
        };
        assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
    }

    #[test]
    fn duplicate_module_identity_cannot_publish_interfaces_or_code() {
        let source = "pub fn answer() Number { 42 }";
        for paths in [
            ["project/src/util.ald", "project/src/util/mod.ald"],
            ["project/src/util/mod.ald", "project/src/util.ald"],
        ] {
            let result = build_fixture_sync(
                paths
                    .into_iter()
                    .map(|path| (url(path), Ok(source.to_owned())))
                    .collect(),
                BuildMode::Build,
                BuildDependencies::default(),
            );
            assert!(!result.is_success());
            assert!(result.interfaces.is_empty());
            assert!(result.artifacts.is_empty());
        }
    }

    #[test]
    fn duplicate_module_diagnostic_labels_both_sources() {
        let source = "pub fn answer() Number { 42 }";
        let result = build_fixture_sync(
            vec![
                (url("project/src/util/mod.ald"), Ok(source.to_owned())),
                (url("project/src/util.ald"), Ok(source.to_owned())),
            ],
            BuildMode::Build,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&url("project/src/util.ald")]
        else {
            panic!("duplicate must fail");
        };
        assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
    }

    #[tokio::test]
    async fn graph_and_compilation_share_package_qualified_source_paths() {
        let app = url("checkout/src/application/src/main.ald");
        let local = url("checkout/src/application/src/src/util.ald");
        let foreign = url("checkout/src/widgets/src/src/util.ald");
        let root = url("checkout/src/widgets/src/mod.ald");
        let package = OwnedPackageId::Named {
            author: "vendor".to_owned(),
            project: "widgets".to_owned(),
        };
        let dependencies = BuildDependencies {
            module_packages: BTreeMap::from([
                (foreign.clone(), package.clone()),
                (root.clone(), package),
            ]),
            module_paths: BTreeMap::from([
                (app.clone(), vec!["main".to_owned()]),
                (local.clone(), vec!["src".to_owned(), "util".to_owned()]),
                (foreign.clone(), vec!["src".to_owned(), "util".to_owned()]),
                (root.clone(), vec![]),
            ]),
            ..BuildDependencies::default()
        };
        let sources = InMemorySource::with_files([
            (
                app.clone(),
                indoc::indoc! {r#"
                import ~/src/util as local
                import @vendor/widgets
                pub fn main() {
                    assert(local.answer() == 42)
                    assert(widgets.answer() == "foreign")
                }
            "#}
                .to_owned(),
            ),
            (local.clone(), "pub fn answer() Number { 42 }".to_owned()),
            (
                foreign.clone(),
                "pub fn answer() String { \"foreign\" }".to_owned(),
            ),
            (
                root.clone(),
                indoc::indoc! {r#"
                import ~/src/util
                pub fn answer() String { util.answer() }
            "#}
                .to_owned(),
            ),
        ]);
        let db = Arc::new(Mutex::new(Database::new(sources)));
        for modules in [
            vec![app.clone(), local.clone(), foreign.clone(), root.clone()],
            vec![root.clone(), foreign.clone(), local.clone(), app.clone()],
        ] {
            let graph = fixture_graph_with_dependencies(db.clone(), &modules, &dependencies)
                .await
                .unwrap();
            assert_eq!(graph.edges[&root], vec![foreign.clone()]);
            let mut expected = vec![local.clone(), root.clone()];
            expected.sort();
            assert_eq!(graph.edges[&app], expected);
            let result = build_fixture_with_dependencies(
                db.clone(),
                &graph,
                BuildMode::Build,
                dependencies.clone(),
            )
            .await;
            assert!(result.is_success(), "{:?}", result.modules);
            assert_eq!(
                result.artifacts[&local].module_id,
                "alder://app/src/util.mjs"
            );
            assert_eq!(
                result
                    .interfaces
                    .iter()
                    .find(|i| i.module.path.is_empty())
                    .unwrap()
                    .module
                    .package,
                dependencies.module_packages[&root]
            );
        }
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
        let interface =
            alder_can::from_module(&bump, canonical.module, &solved.annotations, interfaces);
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
        let graph = fixture_graph(db.clone(), std::slice::from_ref(&uri))
            .await
            .unwrap();
        let result = build_fixture(db, &graph).await;
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
    async fn renders_json_decode_without_an_instance() {
        assert_diagnostic_snapshot! {r#"
            fn invalid() Result[fn(Number) Number, [:invalid_json(String)]] {
                Json.decode("42")
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
        let graph = fixture_graph(db.clone(), &modules).await.unwrap();
        let result = build_fixture(db, &graph).await;

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
        let graph = fixture_graph(db.clone(), &modules).await.unwrap();
        let result = build_fixture_with_mode(db, &graph, BuildMode::Build).await;

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
        let graph = fixture_graph(db.clone(), &modules).await.unwrap();
        let result = build_fixture(db, &graph).await;

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

        let graph = fixture_graph(db.clone(), &modules).await.unwrap();
        let result = build_fixture(db, &graph).await;

        // Utils is solved first; Main canonicalizes and type checks
        // against its interface.
        assert_eq!(result.total, 2);
        assert_eq!(result.success, 2);
        assert!(result.is_success());
    }

    #[test]
    fn imported_error_row_inclusions_preserve_both_source_tails() {
        for (return_type, succeeds) in [
            ("Result[Number, [:known | :left | :right]]", true),
            ("Result[Number, [:known | :left]]", false),
        ] {
            let library = indoc::indoc! {r#"
                pub fn combine(left: Result[Number, [:known | e]], right: Result[Number, [:known | f]]) {
                    let x = left?
                    let y = right?
                    Ok(x + y)
                }
            "#};
            let consumer = indoc::indoc! {r#"
                import ~/utils
                fn left() Result[Number, [:known | :left]] { Err(:left) }
                fn right() Result[Number, [:known | :right]] { Err(:right) }
                pub fn run() RETURN_TYPE { utils.combine(left(), right()) }
            "#}
            .replace("RETURN_TYPE", return_type);
            let result = build_fixture_sync(
                vec![
                    (url("project/src/utils.ald"), Ok(library.to_owned())),
                    (url("project/src/main.ald"), Ok(consumer)),
                ],
                BuildMode::Check,
                BuildDependencies::default(),
            );
            assert_eq!(
                result.is_success(),
                succeeds,
                "return contract {return_type}"
            );
        }
    }

    #[test]
    fn imported_error_row_failure_labels_the_consumer_reference() {
        let consumer = url("project/src/main.ald");
        let source = indoc::indoc! {r#"
            import ~/utils
            fn left() Result[Number, [:known | :left]] { Err(:left) }
            fn right() Result[Number, [:known | :right]] { Err(:right) }
            pub fn run() Result[Number, [:known | :left]] {
                let result: Result[Number, [:known | :left]] = utils.combine(left(), right())
                result
            }
        "#};
        let result = build_fixture_sync(
            vec![
                (url("project/src/utils.ald"), Ok(indoc::indoc! {r#"
                    pub fn combine(left: Result[Number, [:known | e]], right: Result[Number, [:known | f]]) {
                        let x = left?
                        let y = right?
                        Ok(x + y)
                    }
                "#}.to_owned())),
                (consumer.clone(), Ok(source.to_owned())),
            ],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&consumer] else {
            panic!("the omitted error tag must fail in the consumer");
        };
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = &diagnostics[0];
        let label = miette::Diagnostic::labels(diagnostic)
            .unwrap()
            .next()
            .unwrap();
        assert_eq!(label.offset(), source.find("utils.combine").unwrap());
        assert_eq!(label.len(), "utils.combine".len());
        assert_rendered_diagnostic_snapshot!(source, diagnostic.clone());
    }

    #[test]
    fn imported_exact_error_union_supports_exhaustive_matching() {
        let result = build_fixture_sync(
            vec![
                (url("project/src/utils.ald"), Ok(indoc::indoc! {r#"
                    pub fn combine(left: Result[Number, [:known | e]], right: Result[Number, [:known | f]]) {
                        let x = left?
                        let y = right?
                        Ok(x + y)
                    }
                "#}.to_owned())),
                (url("project/src/main.ald"), Ok(indoc::indoc! {r#"
                    import ~/utils
                    fn left() Result[Number, [:known | :left]] { Err(:left) }
                    fn right() Result[Number, [:known | :right]] { Err(:right) }
                    pub fn run() Number {
                        match utils.combine(left(), right()) {
                            Ok(value) => value,
                            Err(:known) => 0,
                            Err(:left) => 1,
                            Err(:right) => 2,
                        }
                    }
                "#}.to_owned())),
            ],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        assert!(result.is_success());
    }

    #[test]
    fn trait_methods_support_every_import_form_across_interfaces() {
        let traits = url("project/src/traits.ald");
        let qualified = url("project/src/qualified.ald");
        let named = url("project/src/named.ald");
        let open = url("project/src/open.ald");
        let trait_qualified = url("project/src/trait_qualified.ald");
        let result = build_fixture_sync(
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
        let result = build_fixture_sync(
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
            indoc::indoc! {r#"
                pub enum Token { Token }
                pub trait Display[a] { fn display(value: a) String }
            "#}
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
        let package = OwnedPackageId::Named {
            author: "vendor".to_owned(),
            project: "widgets".to_owned(),
        };
        let dependencies = fixture_dependencies(
            &modules,
            BuildDependencies {
                module_packages: BTreeMap::from([
                    (model, package.clone()),
                    (implementation, package.clone()),
                ]),
                ..BuildDependencies::default()
            },
        );
        let graph = super::build_graph_with_dependencies(db.clone(), &modules, &dependencies)
            .await
            .unwrap();
        let result =
            super::build_with_dependencies(db, &graph, BuildMode::Check, dependencies).await;

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
        let graph = fixture_graph(db.clone(), std::slice::from_ref(&consumer))
            .await
            .unwrap();
        let result = build_fixture_with_dependencies(
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
    async fn imported_error_group_is_flattened_as_a_closed_row() {
        let errors = dependency_interface(
            indoc::indoc! {r#"
                pub error Failure {
                    :invalid(String),
                    :missing
                }
            "#},
            &["errors"],
            &[],
        );
        let mem = InMemorySource::new();
        let consumer = url("project/src/main.ald");
        mem.insert(
            consumer.clone(),
            indoc::indoc! {r#"
                import @vendor/widgets/errors.{ Failure }

                pub fn invalid() Result[Number, Failure] {
                    Err(:other)
                }
            "#}
            .to_owned(),
        );
        let db = Arc::new(Mutex::new(Database::new(mem)));
        let graph = fixture_graph(db.clone(), std::slice::from_ref(&consumer))
            .await
            .unwrap();
        let result = build_fixture_with_dependencies(
            db,
            &graph,
            BuildMode::Check,
            BuildDependencies {
                interfaces: vec![errors],
                ..BuildDependencies::default()
            },
        )
        .await;

        let ModuleResult::Failed { diagnostics } = &result.modules[&consumer] else {
            panic!("imported closed group must reject an unknown tag")
        };
        assert!(diagnostics[0].message().contains("type mismatch"));
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

        let graph = fixture_graph(db.clone(), &modules).await.unwrap();
        let result = build_fixture(db, &graph).await;

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
        let graph = fixture_graph(db.clone(), std::slice::from_ref(&uri))
            .await
            .unwrap();
        let result = build_fixture(db, &graph).await;
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
        let result = build_fixture_sync(
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
        let result = build_fixture_sync(
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

    #[tokio::test]
    async fn renders_async_scope_error_for_task_annotated_function() {
        assert_diagnostic_snapshot! {r#"
            fn wait() Task[()] {
                Task.sleep(1).await
            }
        "#};
    }

    #[tokio::test]
    async fn renders_async_scope_error_for_nested_lambda() {
        assert_diagnostic_snapshot! {r#"
            async fn make() {
                () -> Task.sleep(1).await
            }
        "#};
    }

    #[tokio::test]
    async fn renders_invalid_error_tag_placement_without_color() {
        assert_diagnostic_snapshot! {r#"
            fn invalid() {
                let failure = :not_found(42)
                failure
            }
        "#};
    }

    #[tokio::test]
    async fn renders_missing_closed_error_match_case_without_color() {
        assert_diagnostic_snapshot! {r#"
            error Failure {
                :invalid(String),
                :missing
            }

            fn render(value: Result[Number, Failure]) String {
                match value {
                    Ok(number) => "ok",
                    Err(:invalid(message)) => message,
                }
            }
        "#};
    }

    #[tokio::test]
    async fn renders_open_error_match_hint_without_color() {
        assert_diagnostic_snapshot! {r#"
            fn render(value: Result[Number]) String {
                match value {
                    Ok(number) => "ok",
                    Err(:invalid(message)) => message,
                }
            }
        "#};
    }

    #[tokio::test]
    async fn renders_await_outside_a_function_without_color() {
        assert_diagnostic_snapshot! {r#"
            let invalid = Task.sleep(1).await
        "#};
    }

    #[tokio::test]
    async fn renders_awaiting_a_non_task_without_color() {
        assert_diagnostic_snapshot! {r#"
            async fn invalid() {
                (42).await
            }
        "#};
    }

    #[tokio::test]
    async fn renders_try_after_awaiting_a_non_result_without_color() {
        assert_diagnostic_snapshot! {r#"
            #[extern("alder:kernel", "$taskSleep")]
            fn sleep(milliseconds: Number) Task[()]

            async fn invalid() Result[Number] {
                sleep(1).await?
                Ok(42)
            }
        "#};
    }

    #[tokio::test]
    async fn renders_abort_convention_on_a_synchronous_extern_without_color() {
        assert_diagnostic_snapshot! {r#"
            #[extern("globalThis", "JSON.parse", "abort")]
            fn parse(value: String) String
        "#};
    }

    #[tokio::test]
    async fn renders_a_missing_return_after_a_zero_iteration_loop() {
        assert_diagnostic_snapshot! {r#"
            fn missing(flag: Bool) Number {
                while flag { return 42 }
            }
        "#};
    }

    #[tokio::test]
    async fn renders_unresolved_shared_export_without_color() {
        assert_diagnostic_snapshot! {r#"
            pub let shared = []
        "#};
    }

    #[tokio::test]
    async fn imported_record_contracts_reject_incompatible_tails_and_presence() {
        for (provider, consumer) in [
            (
                indoc::indoc! {r#"
                    pub fn both(left: { r | x: Number }, right: { r | y: Number }) Number {
                        left.x + right.y
                    }
                "#},
                indoc::indoc! {r#"
                    import ~/rows
                    pub fn main() Number {
                        rows.both({ x: 1, extra: 42 }, { y: 2, extra: "wrong" })
                    }
                "#},
            ),
            (
                indoc::indoc! {r#"
                    pub fn optional() ({ value?: Number }) { {} }
                "#},
                indoc::indoc! {r#"
                    import ~/rows
                    pub fn main() Number { rows.optional().value }
                "#},
            ),
        ] {
            let mem = InMemorySource::new();
            mem.insert(url("project/src/rows.ald"), provider.to_owned());
            mem.insert(url("project/src/main.ald"), consumer.to_owned());
            let db = Arc::new(Mutex::new(Database::new(mem)));
            let modules = vec![url("project/src/rows.ald"), url("project/src/main.ald")];
            let graph = fixture_graph(db.clone(), &modules).await.unwrap();
            let result = build_fixture(db, &graph).await;
            assert!(!result.is_success(), "{provider}\n{consumer}");
            assert_eq!(
                result.success, 1,
                "the provider remains independently valid"
            );
        }
    }

    #[tokio::test]
    async fn shared_state_remains_monomorphic_across_module_interfaces() {
        let mem = InMemorySource::new();
        mem.insert(
            url("project/src/state.ald"),
            indoc::indoc! {r#"
                pub let shared: Array[Number] = []
        "#}
            .to_owned(),
        );
        mem.insert(
            url("project/src/main.ald"),
            indoc::indoc! {r#"
                import ~/state
            pub fn main() { Array.push(state.shared, "text") }
        "#}
            .to_owned(),
        );
        let db = Arc::new(Mutex::new(Database::new(mem)));
        let modules = vec![url("project/src/state.ald"), url("project/src/main.ald")];
        let graph = fixture_graph(db.clone(), &modules).await.unwrap();
        let result = build_fixture(db, &graph).await;
        assert!(
            !result.is_success(),
            "importing a shared array cannot change its element type"
        );
        assert_eq!(
            result.success, 1,
            "the concrete shared-state module itself remains valid"
        );
    }

    #[tokio::test]
    async fn renders_assignment_to_function_declaration_without_mut_hint() {
        assert_diagnostic_snapshot! {r#"
            fn identity(value) { value }
            fn replace() { identity = value -> value }
        "#};
    }

    #[tokio::test]
    async fn renders_tuple_index_overflow_without_color() {
        assert_diagnostic_snapshot! {r#"
            fn read(pair) { pair.4294967296 }
        "#};
    }

    #[tokio::test]
    async fn renders_invalid_builtin_argument_without_color() {
        assert_diagnostic_snapshot! {r#"
            fn bad() Number { String.length(42) }
        "#};
    }

    #[tokio::test]
    async fn renders_unknown_builtin_member_without_color() {
        assert_diagnostic_snapshot! {r#"
            fn bad() { Array.missing([1]) }
        "#};
    }

    #[tokio::test]
    async fn renders_recursive_alias_without_color() {
        assert_diagnostic_snapshot! {r#"
            type First = { next: Second }
            type Second = Array[First]
        "#};
    }

    #[tokio::test]
    async fn renders_generic_method_specialization_without_color() {
        assert_diagnostic_snapshot! {r#"
            trait Convert[a] { fn convert(value: a, other: b) b }
            impl Convert[Number] {
                fn convert(value: Number, other: b) b { 42 }
            }
        "#};
    }

    #[tokio::test]
    async fn renders_lambda_specializing_an_enclosing_generic_without_color() {
        assert_diagnostic_snapshot! {r#"
            fn keep(value: a) a {
                let ignored = (other: a) a -> 42
                value
            }
        "#};
    }

    #[tokio::test]
    async fn renders_generic_variable_escape_without_color() {
        assert_diagnostic_snapshot! {r#"
            #[extern("alder:kernel", "$arrayPush")]
            fn push(values: Array[a], value: a) ()
            let stored = []
            fn store(value: a) { push(stored, value) }
        "#};
    }

    #[tokio::test]
    async fn renders_invalid_async_extern_signature_without_color() {
        assert_diagnostic_snapshot! {r#"
            #[extern("globalThis", "fetch")]
            fn fetch(url: String) Task
        "#};
    }

    #[tokio::test]
    async fn renders_an_unknown_extern_convention_without_color() {
        assert_diagnostic_snapshot! {r#"
            #[extern("globalThis", "fetch", "cancel")]
            fn fetch(url: String) Task[String]
        "#};
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

        let graph = fixture_graph(db.clone(), &modules).await.unwrap();
        let result = build_fixture(db, &graph).await;

        assert_eq!(result.total, 2);
        assert_eq!(result.success, 2);
        assert!(result.is_success());
    }
}
