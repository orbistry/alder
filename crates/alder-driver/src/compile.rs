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
    /// Empty if any source module fails or is blocked.
    pub artifacts: HashMap<Url, alder_codegen::EmittedModule>,

    /// Solved semantic interfaces ready for persistent caching.
    /// Published only after the entire source build succeeds.
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

    // Reject an incoherent source package before compiling bodies against its
    // frozen registry. Otherwise each module's solver repeats the same foreign
    // errors against its own source, and missing inferred interfaces cascade.
    let registry_module = store.alloc(alder_ast::Module {
        id: ModuleId {
            package: PackageId::Builtin,
            path: &[],
        },
        imports: &[],
        items: &[],
        value_sccs: &[],
        assigned_bindings: &[],
    });
    let registry = alder_solve::TraitDatabase::build_with_package_instances(
        &store,
        registry_module,
        &interfaces,
        package_instances,
    );
    let coherence_errors = registry.validate(&store);
    let owners = sources
        .iter()
        .filter_map(|(uri, _)| {
            let identity = &identities[uri];
            let home = ModuleId {
                package: hydrate_package_id(&store, &identity.package),
                path: store.alloc_slice_fill_iter(
                    identity
                        .path
                        .iter()
                        .map(|part| store.alloc_str(part) as &str),
                ),
            };
            coherence_errors
                .iter()
                .any(|error| coherence_belongs_to(error, home))
                .then_some(uri.clone())
        })
        .collect::<std::collections::BTreeSet<_>>();
    if !coherence_errors.is_empty() {
        let source_modules = sources
            .iter()
            .map(|(uri, _)| {
                let identity = &identities[uri];
                ModuleId {
                    package: hydrate_package_id(&store, &identity.package),
                    path: store.alloc_slice_fill_iter(
                        identity
                            .path
                            .iter()
                            .map(|part| store.alloc_str(part) as &str),
                    ),
                }
            })
            .collect::<std::collections::BTreeSet<_>>();
        let diagnostics = coherence_errors
            .iter()
            .filter(|error| error.modules().is_disjoint(&source_modules))
            .map(|error| crate::report::dependency_coherence(registry_module, error))
            .collect();
        let modules = sources
            .iter()
            .map(|(uri, source)| {
                let result = if owners.contains(uri) {
                    compile_module(
                        uri,
                        source,
                        &identities[uri],
                        &store,
                        &interfaces,
                        package_instances,
                        BuildMode::Check,
                    )
                    .0
                    .result
                } else {
                    ModuleResult::Blocked
                };
                (uri.clone(), result)
            })
            .collect();
        return BuildResult {
            diagnostics,
            modules,
            total,
            success: 0,
            failed: total,
            warnings: vec![],
            artifacts: HashMap::new(),
            interfaces: vec![],
            package_instance_indexes: vec![],
        };
    }

    // Header discovery is intentionally broader than successful body checking:
    // coherence still needs declarations from invalid modules. Those headers
    // must not turn an unavailable value interface into errors in importers.
    let origins = identities
        .iter()
        .map(|(uri, identity)| (identity.clone(), vec![uri.clone()]))
        .collect();
    let imports = sources
        .iter()
        .map(|(uri, source)| {
            (
                uri,
                source
                    .as_ref()
                    .map(|source| extract_imports(source, &identities[uri].package, &origins))
                    .unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();
    let mut unavailable = sources
        .iter()
        .zip(&solved_interfaces)
        .filter_map(|((uri, _), solved)| (!solved).then_some(uri.clone()))
        .collect::<std::collections::BTreeSet<_>>();
    let mut blocked = std::collections::BTreeSet::new();
    loop {
        let mut changed = false;
        for (uri, imports) in &imports {
            if imports.iter().any(|import| unavailable.contains(import)) {
                blocked.insert((*uri).clone());
                changed |= unavailable.insert((*uri).clone());
            }
        }
        if !changed {
            break;
        }
    }

    let mut results: HashMap<Url, ModuleResult> = HashMap::new();
    let mut all_warnings: Vec<Diagnostic> = Vec::new();
    let mut artifacts = HashMap::new();
    let mut interface_files = Vec::new();
    for (uri, source) in &sources {
        if blocked.contains(uri) {
            results.insert(uri.clone(), ModuleResult::Blocked);
            continue;
        }
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
    all_warnings.sort_by(Diagnostic::source_order);
    // A package registry is a complete semantic unit, not a cache of whichever
    // modules happened to pass. In particular, package-wide evidence can refer
    // to impl headers whose bodies failed in another source module.
    if success != total {
        artifacts.clear();
        interface_files.clear();
    }
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
    let failed = |mut diagnostics: Vec<Diagnostic>| {
        diagnostics.sort_by(Diagnostic::source_order);
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
                        interfaces,
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
                        crate::report::solve(
                            report_source.clone(),
                            can_result.module,
                            interfaces,
                            error,
                        )
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
    error.modules().contains(&home)
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
macro_rules! assert_rendered_diagnostics_snapshot {
    ($source:expr, $diagnostics:expr) => {{
        let mut rendered = String::new();
        let handler = miette::GraphicalReportHandler::new_themed(
            miette::GraphicalTheme::unicode_nocolor(),
        ).with_width(80);
        for diagnostic in $diagnostics {
            handler.render_report(&mut rendered, diagnostic).expect("diagnostic renders");
        }
        insta::with_settings!({ description => $source, omit_expression => true }, {
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

    fn url(path: &str) -> Url {
        Url::parse(&format!("file:///{}", path)).unwrap()
    }

    #[test]
    fn mismatch_preserves_distinct_generic_variables() {
        let source = indoc::indoc! {r#"
            fn broken(value: fn(a, b) a) Bool { value }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("invalid return must fail");
        };
        assert_eq!(diagnostics.len(), 1);
        assert!(
            diagnostics[0].to_string().contains("fn(a, b) a"),
            "{diagnostics:?}"
        );
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
    }

    #[test]
    fn mismatch_preserves_nested_types_and_open_rows() {
        let source = indoc::indoc! {r#"
            fn broken(value: (Option[a], Result[b, [:failed(a) | e]], Task[b], {r | name: a})) Bool {
                value
            }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("invalid return must fail");
        };
        assert_eq!(diagnostics.len(), 1);
        let message = diagnostics[0].to_string();
        assert!(
            message.contains("Option[a], Result[b, [:failed(a) | c]], Task[b], { d | name: a }"),
            "{message}"
        );
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
    }

    #[test]
    fn nominal_type_mismatches_use_resolved_import_names() {
        let source = indoc::indoc! {r#"
            import ~/left.{ Token as LeftToken }
            import ~/right.{ Token as RightToken }
            fn invalid(value: RightToken) LeftToken { value }
            fn nested(value: Array[RightToken]) Array[LeftToken] { value }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![
                (uri.clone(), Ok(source.to_owned())),
                (
                    url("app/src/left.ald"),
                    Ok("pub enum Token { Token }".to_owned()),
                ),
                (
                    url("app/src/right.ald"),
                    Ok("pub enum Token { Token }".to_owned()),
                ),
            ],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("nominal types from different modules must not unify")
        };
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert_eq!(
            diagnostics[0].message(),
            "type mismatch: expected `LeftToken`, found `RightToken`"
        );
        assert_eq!(
            diagnostics[1].message(),
            "type mismatch: expected `Array[LeftToken]`, found `Array[RightToken]`"
        );
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
        let valid = source
            .replace("value: RightToken", "value: LeftToken")
            .replace("Array[RightToken]", "Array[LeftToken]");
        let result = build_fixture_sync(
            vec![
                (uri.clone(), Ok(valid)),
                (
                    url("app/src/left.ald"),
                    Ok("pub enum Token { Token }".to_owned()),
                ),
                (
                    url("app/src/right.ald"),
                    Ok("pub enum Token { Token }".to_owned()),
                ),
            ],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        assert!(matches!(result.modules[&uri], ModuleResult::Success { .. }));
    }

    #[test]
    fn transparent_alias_comparisons_keep_expanded_shapes_and_annotation_origins() {
        let source = indoc::indoc! {r#"
            type Named = { profile: { name: String } }
            type Wrong = { profile: { name: Number } }
            type Wrapped[a] = Option[Array[a]]
            fn mismatch(value: Wrong) Named { value }
            fn nested(value: Wrapped[Number]) Wrapped[String] { value }
            fn early(value: Wrong) Named { return value }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("incompatible alias expansions must fail")
        };
        assert_eq!(diagnostics.len(), 3, "{diagnostics:?}");
        assert_eq!(
            diagnostics[0].message(),
            "type mismatch: expected `{ profile: { name: String } }`, found `{ profile: { name: Number } }`"
        );
        assert_eq!(
            diagnostics[1].message(),
            "type mismatch: expected `Option[Array[String]]`, found `Option[Array[Number]]`"
        );
        for (diagnostic, annotation) in
            diagnostics
                .iter()
                .zip(["Named", "Wrapped[String]", "Named"])
        {
            let labels = miette::Diagnostic::labels(diagnostic)
                .unwrap()
                .collect::<Vec<_>>();
            let origin = labels
                .iter()
                .find(|label| label.label() == Some("the declared return type"))
                .unwrap();
            assert_eq!(
                &source[origin.offset()..origin.offset() + origin.len()],
                annotation
            );
        }
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
        let valid = source
            .replace("value: Wrong", "value: Named")
            .replace("value: Wrapped[Number]", "value: Wrapped[String]");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(valid))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        assert!(matches!(result.modules[&uri], ModuleResult::Success { .. }));
    }

    #[test]
    fn record_return_context_does_not_relabel_projection_errors() {
        for source in [
            indoc::indoc! {r#"
            type Named = { name: String }
            fn invalid(value: { present: Number }) Named {
                let read = value.missing
                { name: "valid return" }
            }
        "#},
            indoc::indoc! {r#"
            type Named = { name: String }
            fn invalid(value: { present: Number }) Named { value.missing }
        "#},
            indoc::indoc! {r#"
            type Named = { name: String }
            fn invalid(value: { present: Number }) Named { return value.missing }
        "#},
        ] {
            let uri = url("app/src/main.ald");
            let result = build_fixture_sync(
                vec![(uri.clone(), Ok(source.to_owned()))],
                BuildMode::Check,
                BuildDependencies::default(),
            );
            let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
                panic!("the field projection must fail")
            };
            assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
            assert!(diagnostics[0].message().contains("missing"));
            assert!(
                !miette::Diagnostic::labels(&diagnostics[0])
                    .unwrap()
                    .any(|label| label.label() == Some("the declared return type"))
            );
        }
    }

    #[test]
    fn nominal_type_names_follow_reexported_identities() {
        let source = indoc::indoc! {r#"
            import ~/api.{ PublicLeft as LeftToken, PublicRight as RightToken }
            fn invalid(value: RightToken) LeftToken { value }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![
                (uri.clone(), Ok(source.to_owned())),
                (
                    url("app/src/left.ald"),
                    Ok("pub enum Token { Token }".to_owned()),
                ),
                (
                    url("app/src/right.ald"),
                    Ok("pub enum Token { Token }".to_owned()),
                ),
                (
                    url("app/src/api.ald"),
                    Ok(indoc::indoc! {r#"
                    pub import ~/left.{ Token as PublicLeft }
                    pub import ~/right.{ Token as PublicRight }
                "#}
                    .to_owned()),
                ),
            ],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("re-exports retain distinct nominal identities")
        };
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(
            diagnostics[0].message(),
            "type mismatch: expected `LeftToken`, found `RightToken`"
        );
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
    }

    #[test]
    fn inferred_nominal_types_describe_module_provenance() {
        let source = indoc::indoc! {r#"
            import ~/left as left
            import ~/right as right
            fn invalid() { left.accept(right.make()) }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![
                (uri.clone(), Ok(source.to_owned())),
                (
                    url("app/src/left.ald"),
                    Ok(indoc::indoc! {r#"
                    pub enum Token { Token }
                    pub fn accept(value: Token) {}
                "#}
                    .to_owned()),
                ),
                (
                    url("app/src/right.ald"),
                    Ok(indoc::indoc! {r#"
                    pub enum Token { Token }
                    pub fn make() Token { Token::Token }
                "#}
                    .to_owned()),
                ),
            ],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("inferred types from different modules must not unify")
        };
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(
            diagnostics[0].message(),
            "type mismatch: expected `Token (from left)`, found `Token (from right)`"
        );
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
    }

    #[test]
    fn explicit_returns_preserve_their_own_annotation_origins() {
        let source = indoc::indoc! {r#"
            fn early() Number {
                return "wrong"
            }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("invalid return must fail")
        };
        assert_eq!(diagnostics.len(), 1);
        let labels = miette::Diagnostic::labels(&diagnostics[0])
            .unwrap()
            .collect::<Vec<_>>();
        assert!(
            labels
                .iter()
                .any(|label| label.label() == Some("the declared return type")),
            "{diagnostics:?}"
        );
        let primary = labels.iter().find(|label| label.primary()).unwrap();
        assert_eq!(primary.offset(), source.find("\"wrong\"").unwrap());
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
    }

    #[test]
    fn stored_nominal_identities_keep_consumer_type_aliases() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
            pub enum Token { Token }
            pub fn accept(value: Token) {}
        "#},
            &[],
            &[],
        );
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ Token as RemoteToken }
            enum Token { Token }
            fn invalid(value: Token) RemoteToken { value }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies {
                interfaces: vec![stored.clone()],
                ..BuildDependencies::default()
            },
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("stored and local nominal types remain distinct")
        };
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(
            diagnostics[0].message(),
            "type mismatch: expected `RemoteToken`, found `Token`"
        );
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ accept }
            enum Token { Token }
            fn invalid() { accept(Token::Token) }
        "#};
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies {
                interfaces: vec![stored],
                ..BuildDependencies::default()
            },
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("an unimported type still retains its package identity")
        };
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(
            diagnostics[0].message(),
            "type mismatch: expected `Token (from @vendor/widgets)`, found `Token`"
        );
        insta::with_settings!({ snapshot_suffix => "unimported" }, {
            assert_rendered_diagnostics_snapshot!(source, diagnostics);
        });
    }

    #[test]
    fn return_origins_follow_lambda_and_async_boundaries() {
        let source = indoc::indoc! {r#"
            fn lambda() {
                let _ = () Number -> { return "lambda" }
            }
            fn tail() {
                let _ = () Bool -> 42
            }
            fn nested() String {
                let _ = () -> {
                    return 1
                    return "nested"
                }
                "outer"
            }
            fn task() String {
                let _ = async {
                    return 1
                    return "task"
                }
                "outer"
            }
            fn restored() Bool {
                let _ = () Number -> 1
                let _ = async { 1 }
                return "restored"
            }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("invalid returns must fail")
        };
        assert_eq!(diagnostics.len(), 5, "{diagnostics:?}");
        for (diagnostic, expected_origin) in
            diagnostics.iter().zip([true, true, false, false, true])
        {
            let labels = miette::Diagnostic::labels(diagnostic)
                .unwrap()
                .collect::<Vec<_>>();
            assert_eq!(
                labels
                    .iter()
                    .any(|label| label.label() == Some("the declared return type")),
                expected_origin,
                "{diagnostic:?}"
            );
        }
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
    }

    #[test]
    fn unused_module_bindings_respect_exports_recursion_and_initializers() {
        let source = indoc::indoc! {r#"
            fn cycle_a() Number { cycle_b() }
            fn cycle_b() Number { cycle_a() }
            fn isolated() Number { 1 }
            fn initialize() { Io.print("keep this effect") }
            let ignored = initialize()
            let shadowed = 1
            pub fn exported(shadowed: Number) Number { shadowed }
            pub let public_value = 42
            pub fn main() Number { public_value }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri, Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        assert!(result.is_success(), "{result:?}");
        assert_eq!(result.warnings.len(), 5, "{:?}", result.warnings);
        for name in ["cycle_a", "cycle_b", "isolated", "ignored", "shadowed"] {
            assert!(
                result
                    .warnings
                    .iter()
                    .any(|warning| warning.message() == format!("unused binding `{name}`"))
            );
        }
        assert_rendered_diagnostics_snapshot!(source, &result.warnings);
    }

    #[test]
    fn record_comparisons_distinguish_missing_and_unexpected_fields() {
        let source = indoc::indoc! {r#"
            fn need(value: { name: String }) {}
            fn extra() { let value = { name: "Ada", age: 37 }
                need(value)
            }
            fn missing() { let value = { age: 37 }
                need(value)
            }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("incompatible record shapes must fail")
        };
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        for diagnostic in diagnostics {
            assert!(
                diagnostic.message().contains("expected `{ name: String }`"),
                "{diagnostic:?}"
            );
            assert!(
                miette::Diagnostic::help(diagnostic)
                    .unwrap()
                    .to_string()
                    .contains("unexpected fields: `age`"),
                "{diagnostic:?}"
            );
        }
        assert!(
            miette::Diagnostic::help(&diagnostics[1])
                .unwrap()
                .to_string()
                .contains("missing fields: `name`")
        );
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
    }

    #[test]
    fn record_comparisons_preserve_typo_suggestions() {
        let source = indoc::indoc! {r#"
            fn need(value: { name: String }) {}
            fn wrong() { need({ naem: "Ada" }) }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("misspelled record field must fail")
        };
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            miette::Diagnostic::help(&diagnostics[0])
                .unwrap()
                .to_string()
                .contains("did you mean `name` instead of `naem`?"),
            "{diagnostics:?}"
        );
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
    }

    #[test]
    fn nested_record_mismatches_retain_the_enclosing_shapes() {
        let source = indoc::indoc! {r#"
            fn need(value: { profile: { name: String, active: Bool } }) {}
            fn wrong() {
                let value = { profile: { name: 42, active: true } }
                need(value)
            }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("nested field must fail")
        };
        assert_eq!(diagnostics.len(), 1);
        assert!(
            diagnostics[0]
                .message()
                .contains("{ profile: { active: Bool, name: String } }"),
            "{diagnostics:?}"
        );
        assert!(
            diagnostics[0]
                .message()
                .contains("{ profile: { active: Bool, name: Number } }"),
            "{diagnostics:?}"
        );
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
    }

    #[test]
    fn compound_mismatches_retain_function_tuple_and_application_shapes() {
        let source = indoc::indoc! {r#"
            fn need_function(value: fn(Number) String) {}
            fn number(value: Number) Number { value }
            fn wrong_function() { need_function(number) }
            fn need_tuple(value: (String, Number)) {}
            fn wrong_tuple() { need_tuple((42, 1)) }
            fn need_array(value: Array[String]) {}
            fn wrong_array() { let values = [42]
                need_array(values)
            }
            fn need_option(value: Option[String]) {}
            fn wrong_option(value: Option[Number]) { need_option(value) }
            fn need_task(value: Task[String]) {}
            fn wrong_task(value: Task[Number]) { need_task(value) }
            fn need_result(value: Result[String, [:failed]]) {}
            fn wrong_result(value: Result[Number, [:failed]]) { need_result(value) }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("compound mismatches must fail")
        };
        assert_eq!(diagnostics.len(), 6, "{diagnostics:?}");
        for (diagnostic, shape) in diagnostics.iter().zip([
            "fn(Number) String",
            "(String, Number)",
            "Array[String]",
            "Option[String]",
            "Task[String]",
            "Result[String, [:failed]]",
        ]) {
            assert!(diagnostic.message().contains(shape), "{diagnostic:?}");
        }
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
        let valid = build_fixture_sync(
            vec![(uri.clone(), Ok(source.replace("String", "Number")))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        assert!(matches!(valid.modules[&uri], ModuleResult::Success { .. }));
    }

    #[test]
    fn infinite_types_explain_the_actual_recursive_equation() {
        let source = indoc::indoc! {r#"
            fn apply_to_self(value) { value(value) }
            fn nested(value) { if true { value } else { { next: value } } }
            fn array(value) { [value, [value]] }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("self application must fail")
        };
        assert_eq!(diagnostics.len(), 3, "{diagnostics:?}");
        for diagnostic in diagnostics {
            assert!(
                diagnostic.message().contains("would need to equal"),
                "{diagnostic:?}"
            );
            assert!(miette::Diagnostic::help(diagnostic).is_some());
        }
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
    }

    #[test]
    fn deferred_structural_cycles_explain_why_no_finite_type_exists() {
        let source = indoc::indoc! {r#"
            fn cycle(first, second) {
                first.0 = second
                second.0 = first
            }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("structural cycle must fail")
        };
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message()
                .contains("structural type would contain itself"),
            "{diagnostics:?}"
        );
        assert!(
            miette::Diagnostic::help(&diagnostics[0])
                .unwrap()
                .to_string()
                .contains("expanding the type")
        );
    }

    #[test]
    fn core_recovery_also_reports_independent_trait_obligations() {
        let source = indoc::indoc! {r#"
            trait Needed[a] { fn required(value: a) Number }
            fn trait_first() Number { Needed::required("string") }
            fn core() Number { true }
            fn dependent() Number { Needed::required(core()) }
            fn trait_failure() Number { Needed::required(1) }
        "#};
        let uri = url("app/src/main.ald");
        for mode in [BuildMode::Check, BuildMode::Build, BuildMode::Test] {
            let result = build_fixture_sync(
                vec![(uri.clone(), Ok(source.to_owned()))],
                mode,
                BuildDependencies::default(),
            );
            let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
                panic!("invalid module must fail")
            };
            assert_eq!(diagnostics.len(), 3, "{diagnostics:?}");
            assert!(
                diagnostics
                    .iter()
                    .any(|error| error.message().contains("Needed")),
                "{diagnostics:?}"
            );
            assert!(result.interfaces.is_empty());
            assert!(result.artifacts.is_empty());
            if mode == BuildMode::Check {
                assert_rendered_diagnostics_snapshot!(source, diagnostics);
            }
        }
    }

    #[test]
    fn module_warning_roots_include_tests_methods_and_entry_points() {
        let source = indoc::indoc! {r#"
            fn tested() Number { 42 }
            test "keeps its helper" { assert tested() == 42 }
            fn from_method() String { "number" }
            pub trait Describe[a] { fn describe(value: a) String }
            impl Describe[Number] {
                fn describe(_: Number) String { from_method() }
            }
            fn main() Number { 0 }
        "#};
        let result = build_fixture_sync(
            vec![(url("app/src/main.ald"), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        assert!(result.is_success(), "{result:?}");
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    }

    #[test]
    fn deferred_optional_arguments_keep_position_and_callee() {
        let source = indoc::indoc! {r#"
            fn need(value?: Number) {}
            fn ordinary() { need("wrong") }
            fn piped() { "wrong" |> need() }
            fn later(first: Number, value?: Number) {}
            fn second() { later(1, "wrong") }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("invalid arguments must fail")
        };
        assert_eq!(diagnostics.len(), 3, "{diagnostics:?}");
        for (diagnostic, (position, callee)) in
            diagnostics
                .iter()
                .zip([(1, "need"), (1, "need"), (2, "later")])
        {
            assert!(
                miette::Diagnostic::labels(diagnostic)
                    .unwrap()
                    .any(|label| label
                        .label()
                        .is_some_and(|label| label.contains(&format!("argument {position}"))
                            && label.contains(callee))),
                "{diagnostic:?}"
            );
        }
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
    }

    #[test]
    fn mismatch_explains_expectations_at_the_source() {
        let source = indoc::indoc! {r#"
            fn need(value: Number) { () }
            fn argument() { need("wrong") }
            fn annotation() {
                let value: Bool = 42
                value
            }
            fn condition() { if 1 { () } else { () } }
            fn branch(flag: Bool) { if flag { 1 } else { "wrong" } }
            fn array() { [1, "wrong"] }
            fn returned() Bool { 42 }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("invalid source must fail");
        };
        assert_eq!(diagnostics.len(), 6, "{diagnostics:?}");
        let rendered = format!("{diagnostics:?}");
        for context in [
            "argument 1",
            "annotation",
            "condition",
            "branch",
            "array element 2",
            "return value",
        ] {
            assert!(rendered.contains(context), "missing {context}: {rendered}");
        }
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
    }

    #[test]
    fn nested_expectations_keep_the_innermost_cause() {
        let source = indoc::indoc! {r#"
            fn need(value: Number) { () }
            fn nested() { need(if 1 { 2 } else { 3 }) }
            fn assigned() {
                let value = 1
                value = "wrong"
            }
            fn patterned(value: Number) { match value { true => () } }
            async fn awaited() { (1).await }
            fn propagated(value: Option[Number]) Result[Number] { Ok(value?) }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("invalid source must fail");
        };
        assert_eq!(diagnostics.len(), 5, "{diagnostics:?}");
        let rendered = format!("{diagnostics:?}");
        for context in [
            "condition must be Bool",
            "assignment",
            "pattern",
            "await requires",
            "propagation",
        ] {
            assert!(rendered.contains(context), "missing {context}: {rendered}");
        }
        assert!(
            !rendered.contains("argument 1"),
            "the condition is invalid, not its enclosing argument: {rendered}"
        );
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
    }

    #[test]
    fn call_arity_reports_counts_and_callee() {
        let source = indoc::indoc! {r#"
            fn need(value: Number) { () }
            fn too_few() { need() }
            fn too_many() { need(1, 2) }
            fn optional(value: Option[Number]) { () }
            pub fn valid() { optional() }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("invalid calls must fail");
        };
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert!(diagnostics[0].to_string().contains("expected 1, found 0"));
        assert!(diagnostics[1].to_string().contains("expected 1, found 2"));
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
    }

    #[test]
    fn cross_module_warnings_have_stable_source_order() {
        let fixtures = vec![
            (
                url("app/src/main.ald"),
                Ok("import ~/z\npub fn read(unused: Number) { z.value }".to_owned()),
            ),
            (
                url("app/src/z.ald"),
                Ok("pub let value = 1\npub fn helper(unused: Bool) { 0 }".to_owned()),
            ),
        ];
        for fixtures in [fixtures.clone(), fixtures.into_iter().rev().collect()] {
            let result =
                build_fixture_sync(fixtures, BuildMode::Check, BuildDependencies::default());
            assert_eq!(result.failed, 0, "{result:?}");
            let sources = result
                .warnings
                .iter()
                .map(|warning| warning.source().name())
                .collect::<Vec<_>>();
            assert_eq!(sources, ["/app/src/main.ald", "/app/src/z.ald"]);
        }
    }

    #[test]
    fn import_usage_includes_types_traits_qualified_names_and_wildcards() {
        let library = indoc::indoc! {r#"
            pub type Payload = { number: Number }
            pub enum Token { Token }
            pub trait Measure[a] { fn measure(value: a) Number }
            impl Measure[Number] { fn measure(value: Number) Number { value } }
            pub let value = 1
        "#};
        for source in [
            indoc::indoc! {r#"
                import ~/library.{ Payload, Measure }
                import ~/library as library
                pub fn read(payload: Payload) { Measure::measure(payload.number) }
                pub fn value() { library.value }
            "#},
            indoc::indoc! {r#"
                import ~/library.*
                pub fn read() { value }
            "#},
            indoc::indoc! {r#"
                import ~/library.{ Measure as Size }
                pub fn read(value: a) Number where a: Size { Size::measure(value) }
            "#},
            "pub import ~/library.*",
        ] {
            let result = build_fixture_sync(
                vec![
                    (url("app/src/main.ald"), Ok(source.to_owned())),
                    (url("app/src/library.ald"), Ok(library.to_owned())),
                ],
                BuildMode::Check,
                BuildDependencies::default(),
            );
            assert_eq!(result.failed, 0, "{source}\n{result:?}");
            assert!(
                result.warnings.is_empty(),
                "{source}\n{:?}",
                result.warnings
            );
        }
    }

    #[test]
    fn unused_wildcard_import_reports_one_source_warning() {
        let source = "import ~/library.*\npub fn main() { 1 }";
        let result = build_fixture_sync(
            vec![
                (url("app/src/main.ald"), Ok(source.to_owned())),
                (
                    url("app/src/library.ald"),
                    Ok("pub let first = 1\npub let second = 2".to_owned()),
                ),
            ],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        assert_eq!(result.failed, 0, "{result:?}");
        assert_eq!(result.warnings.len(), 1);
        assert_rendered_diagnostics_snapshot!(source, &result.warnings);
    }

    #[test]
    fn unused_imports_respect_aliases_shadowing_exports_and_constructors() {
        let source = indoc::indoc! {r#"
            import ~/library.{ value as used, spare as unused, value as shadowed, Token }
            import ~/library as library
            pub import ~/library.{ spare as exported }
            pub fn read() { (used, Token::Token) }
            pub fn shadow(shadowed: Number) { shadowed }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![
                (uri.clone(), Ok(source.to_owned())),
                (
                    url("app/src/library.ald"),
                    Ok("pub let value = 1\npub let spare = 2\npub enum Token { Token }".to_owned()),
                ),
            ],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        assert_eq!(result.failed, 0, "{result:?}");
        assert_eq!(result.warnings.len(), 3, "{:?}", result.warnings);
        assert_rendered_diagnostics_snapshot!(source, &result.warnings);
    }

    #[test]
    fn unused_locals_share_alternatives_and_count_pins_guards_and_captures() {
        let source = indoc::indoc! {r#"
            pub fn alternatives(input: Option[Number]) {
                match input { Some(value) | Some(value) => 0, None => 1 }
            }
            pub fn guards(input: Option[Number]) {
                match input { Some(value) if value > 0 => 1, _ => 0 }
            }
            pub fn pins(input: Number, expected: Number) {
                match input { ^expected => 1, _ => 0 }
            }
            pub fn captures(input: Number) { () -> input }
            pub fn rest(input: Array[Number]) {
                match input { [first, ..rest] => first, _ => 0 }
            }
            pub fn alias(input: Option[Number]) {
                match input { Some(value) as whole => value, None => 0 }
            }
            pub fn used_alternatives(input: Option[Number]) {
                match input { Some(value) | Some(value) => value, None => 0 }
            }
            trait Signature[a] { fn method(value: a) Number }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        assert!(
            matches!(result.modules[&uri], ModuleResult::Success { .. }),
            "{result:?}"
        );
        assert_eq!(result.warnings.len(), 3, "{:?}", result.warnings);
        assert_rendered_diagnostics_snapshot!(source, &result.warnings);

        let corrected = source
            .replace("Some(value) | Some(value) => 0", "Some(_) | Some(_) => 0")
            .replace("..rest", "..")
            .replace(" as whole", "");
        let result = build_fixture_sync(
            vec![(uri, Ok(corrected))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        assert_eq!(result.failed, 0, "{result:?}");
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    }

    #[test]
    fn unused_locals_respect_shadowing_patterns_and_effects() {
        let source = indoc::indoc! {r#"
            let calls = 0
            fn effect() { calls += 1 }
            pub fn example(input: Number) {
                let ignored = effect()
                let outer = input
                let result = {
                    let outer = 2
                    outer
                }
                let (used, unused) = (result, 3)
                let _ = effect()
                let counter = 0
                counter = 1
                { used }
            }
            pub fn unused_parameter(value: Number) { 1 }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        assert!(
            matches!(result.modules[&uri], ModuleResult::Success { .. }),
            "{result:?}"
        );
        assert_eq!(result.warnings.len(), 4, "{:?}", result.warnings);
        assert_rendered_diagnostics_snapshot!(source, &result.warnings);
    }

    #[test]
    fn missing_record_field_suggests_only_existing_nearby_fields() {
        let source = indoc::indoc! {r#"
            fn typo(record: { username: String, age: Number }) { record.usernme }
            fn absent(record: { age: Number }) { record.address }
            fn ambiguous(record: { cat: Number, cot: Number }) { record.cut }
            pub fn valid(record: { username: String }) { record.username }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("invalid accesses must fail");
        };
        assert_eq!(diagnostics.len(), 3);
        assert!(format!("{:?}", diagnostics[0]).contains("did you mean `username`"));
        assert!(!format!("{:?}", diagnostics[1]).contains("did you mean"));
        assert!(!format!("{:?}", diagnostics[2]).contains("did you mean"));
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
    }

    #[test]
    fn independent_type_errors_accumulate_without_publishing() {
        let source = indoc::indoc! {r#"
            fn first() Number { "wrong" }
            fn second() Bool { 42 }
            pub fn dependent() { first() + 1 }
        "#};
        let uri = url("app/src/main.ald");
        for mode in [BuildMode::Check, BuildMode::Build, BuildMode::Test] {
            let result = build_fixture_sync(
                vec![(uri.clone(), Ok(source.to_owned()))],
                mode,
                BuildDependencies::default(),
            );
            let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
                panic!("invalid source must fail");
            };
            assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
            if matches!(mode, BuildMode::Check) {
                assert_rendered_diagnostics_snapshot!(source, diagnostics);
            }
            assert!(result.artifacts.is_empty());
            assert!(result.interfaces.is_empty());
            assert!(!result.is_success());
        }
    }

    #[test]
    fn recovery_isolates_recursive_peers_and_deferred_generic_contracts() {
        let source = indoc::indoc! {r#"
            fn first(value: a) a { second(value) }
            fn second(value) { if true { first(value) } else { 42 } }
            fn independent(value: b) b { "wrong" }
            pub fn dependent() { first(1) }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("invalid generic contracts must fail");
        };
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
        assert!(result.interfaces.is_empty());
    }

    #[test]
    fn recovery_accumulates_independent_impl_method_errors() {
        let source = indoc::indoc! {r#"
            trait Convert[a] {
                fn first(value: a) String
                fn second(value: a) Bool
            }
            impl Convert[Number] {
                fn first(value: Number) String { 42 }
                fn second(value: Number) Bool { 42 }
            }
            impl Convert[String] {
                fn first(value: String) String { independent() }
                fn second(value: String) Bool { 42 }
            }
            fn independent() String { 42 }
            fn dependent() { Convert::first(42) }
            trait Required[a] { fn required(value: a) String }
            enum Missing { Missing }
            fn unmet(value: Missing) { Required::required(value) }
        "#};
        let uri = url("app/src/main.ald");
        for mode in [BuildMode::Check, BuildMode::Build, BuildMode::Test] {
            let result = build_fixture_sync(
                vec![(uri.clone(), Ok(source.to_owned()))],
                mode,
                BuildDependencies::default(),
            );
            let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
                panic!("invalid implementation bodies must fail")
            };
            assert_eq!(diagnostics.len(), 5, "{diagnostics:?}");
            assert!(result.interfaces.is_empty());
            assert!(result.artifacts.is_empty());
            assert!(result.package_instance_indexes.is_empty());
            if mode == BuildMode::Check {
                assert_rendered_diagnostics_snapshot!(source, diagnostics);
            }
        }
    }

    #[test]
    fn recovery_reports_default_body_errors_once_across_inheriting_impls() {
        let source = indoc::indoc! {r#"
            trait Defaults[a] {
                fn first(value: a) String { 42 }
                fn second(value: a) Bool { 42 }
                fn dependent(value: a) String { broken() }
            }
            impl Defaults[Number] {}
            impl Defaults[String] {}
            fn broken() String { 42 }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("invalid defaults must fail")
        };
        assert_eq!(diagnostics.len(), 3, "{diagnostics:?}");
        assert!(result.interfaces.is_empty());
        assert!(result.artifacts.is_empty());
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
    }

    #[test]
    fn recovery_keeps_deferred_generic_method_contracts_separate() {
        let source = indoc::indoc! {r#"
            trait Convert[a] {
                fn first(value: a, other: b) b
                fn second(value: a, other: b) b
            }
            impl Convert[Number] {
                fn first(value: Number, other: b) b { 42 }
                fn second(value: Number, other: b) b { false }
            }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("both independent generic contracts must fail")
        };
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.message().contains("does not work for every"))
        );
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
    }

    #[test]
    fn impl_method_recovery_discards_partial_shared_type_unification() {
        let source = indoc::indoc! {r#"
            let shared = []
            fn need(value: (Array[Number], Bool)) {}
            trait Work[a] {
                fn broken(marker: a) ()
                fn valid(marker: a) ()
            }
            impl Work[Number] {
                fn broken(marker: Number) () { need((shared, "wrong")) }
                fn valid(marker: Number) () { Array.push(shared, "ok") }
            }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("the first method must fail")
        };
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].message().contains("(Array[Number], Bool)"),
            "{diagnostics:?}"
        );
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
        let valid = source.replace("need((shared, \"wrong\"))", "()");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(valid))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        assert!(
            matches!(result.modules[&uri], ModuleResult::Success { .. }),
            "{result:?}"
        );
    }

    #[test]
    fn recovery_discards_partial_unification_before_rechecking_shared_state() {
        let source = indoc::indoc! {r#"
            let shared = []
            fn zzaccept(value: (Array[Number], Bool)) { () }
            fn zbroken() { zzaccept((shared, "wrong")) }
            fn avalid() { Array.push(shared, "text") }
            fn unrelated() Bool { 42 }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("invalid source must fail");
        };
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        // The broken call is visited before avalid: its first tuple element
        // temporarily constrains shared to Array[Number], then Bool fails.
        // A catch-and-continue recovery would incorrectly reject avalid too.
        assert_eq!(
            diagnostics[0].message(),
            "type mismatch: expected `(Array[Number], Bool)`, found `(Array[Number], String)`"
        );
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
    }

    #[test]
    fn recovery_accumulates_independent_deferred_and_async_errors() {
        let source = indoc::indoc! {r#"
            fn projection(value: (Number, Bool)) { value.2 }
            fn propagate(value: Number) Option[Number] { Some(value?) }
            async fn wait() Number { "wrong" }
            pub fn dependent() { projection((1, true)) }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("invalid source must fail");
        };
        assert_eq!(diagnostics.len(), 3, "{diagnostics:?}");
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
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
    fn failed_modules_block_importers_without_publishing_partial_builds() {
        let broken = url("project/src/broken.ald");
        let facade = url("project/src/facade.ald");
        let consumer = url("project/src/consumer.ald");
        let independent = url("project/src/independent.ald");
        for signature in [
            "pub fn value() Number { true }",
            "pub fn value() { 1 + true }",
        ] {
            for mode in [BuildMode::Check, BuildMode::Build, BuildMode::Test] {
                let result = build_fixture_sync(
                    vec![
                        (
                            consumer.clone(),
                            Ok("import ~/facade\npub fn main() { facade.value() }".to_owned()),
                        ),
                        (facade.clone(), Ok("pub import ~/broken.*".to_owned())),
                        (independent.clone(), Ok("pub fn other() { 42 }".to_owned())),
                        (broken.clone(), Ok(signature.to_owned())),
                    ],
                    mode,
                    BuildDependencies::default(),
                );
                assert!(
                    matches!(result.modules[&broken], ModuleResult::Failed { .. }),
                    "{result:?}"
                );
                assert!(
                    matches!(result.modules[&consumer], ModuleResult::Blocked),
                    "dependent errors must not cascade: {result:?}"
                );
                assert!(matches!(result.modules[&facade], ModuleResult::Blocked));
                assert!(matches!(
                    result.modules[&independent],
                    ModuleResult::Success { .. }
                ));
                assert!(
                    result.artifacts.is_empty(),
                    "failed builds cannot publish executable output"
                );
                assert!(
                    result.interfaces.is_empty(),
                    "failed builds cannot publish partial interfaces"
                );
                assert!(result.package_instance_indexes.is_empty());
            }
        }
    }

    #[test]
    fn invalid_sibling_impl_bodies_cannot_publish_consumer_evidence() {
        let model = url("project/src/model.ald");
        let consumer = url("project/src/consumer.ald");
        let implementation = url("project/src/implementation.ald");
        for mode in [BuildMode::Check, BuildMode::Build, BuildMode::Test] {
            let result = build_fixture_sync(vec![
                (model.clone(), Ok("pub enum Token { Token }\npub trait Display[a] { fn display(value: a) String }".to_owned())),
                (consumer.clone(), Ok("import ~/model.{ Token, display }\npub fn render(value: Token) String { display(value) }".to_owned())),
                (implementation.clone(), Ok("import ~/model.{ Token, Display }\nimpl Display[Token] { fn display(value: Token) String { 42 } }".to_owned())),
            ], mode, BuildDependencies::default());
            assert!(
                matches!(result.modules[&implementation], ModuleResult::Failed { .. }),
                "{result:?}"
            );
            assert!(
                matches!(result.modules[&consumer], ModuleResult::Success { .. }),
                "the consumer can select the invalid body's header without importing that module: {result:?}"
            );
            assert!(result.artifacts.is_empty());
            assert!(result.interfaces.is_empty());
            assert!(result.package_instance_indexes.is_empty());
        }
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

    #[tokio::test]
    async fn option_lifting_ambiguity_does_not_label_an_unrelated_argument() {
        let source = indoc::indoc! {r#"
            fn consume(value?: Number) {}
            fn relate(first?: a, second: a) {}
            fn conflict(first, second) {
                consume(42)
                relate(first, second)
                relate(second, first)
            }
        "#};
        let diagnostic = compile_failure(source).await;
        let label = miette::Diagnostic::labels(&diagnostic)
            .unwrap()
            .next()
            .unwrap();
        assert_eq!(
            &source[label.offset()..label.offset() + label.len()],
            "first"
        );
        assert_rendered_diagnostic_snapshot!(source, diagnostic);
    }

    #[tokio::test]
    async fn option_lifting_mismatch_does_not_label_an_unrelated_argument() {
        let source = indoc::indoc! {r#"
            fn consume(value?: Number) {}
            fn conflict() {
                let nested = Some(Some(7))
                consume(42)
                consume(nested)
            }
        "#};
        let diagnostic = compile_failure(source).await;
        let label = miette::Diagnostic::labels(&diagnostic)
            .unwrap()
            .next()
            .unwrap();
        assert_eq!(
            &source[label.offset()..label.offset() + label.len()],
            "nested"
        );
        assert_rendered_diagnostic_snapshot!(source, diagnostic);
    }

    #[tokio::test]
    async fn renders_ambiguous_option_lifting_without_color() {
        assert_diagnostic_snapshot! {r#"
            fn relate(first?: a, second: a) {}
            fn conflict(first, second) {
                relate(first, second)
                relate(second, first)
            }
        "#};
    }

    #[tokio::test]
    async fn renders_invalid_result_error_argument_without_color() {
        assert_diagnostic_snapshot! {r#"
            fn identity(value: Result[Number, String]) { value }
        "#};
    }

    #[tokio::test]
    async fn renders_option_propagation_in_result_function_without_color() {
        assert_diagnostic_snapshot! {r#"
            fn read(value: Option[Number]) Result[Number] { Ok(value?) }
        "#};
    }

    #[tokio::test]
    async fn renders_overlapping_open_result_rows_without_color() {
        assert_diagnostic_snapshot! {r#"
            trait Marker[f] { fn pass(value: f[a]) f[a] { value } }
            impl Marker[Result[_, [:left | e]]] {}
            impl Marker[Result[_, [:left | :right]]] {}
        "#};
    }

    #[tokio::test]
    async fn renders_overlapping_open_record_rows_without_color() {
        assert_diagnostic_snapshot! {r#"
            trait Marker[a] { fn pass(value: a) a { value } }
            impl Marker[{ r | x: Number }] {}
            impl Marker[{ x: Number, y: String }] {}
        "#};
    }

    #[tokio::test]
    async fn renders_custom_error_group_impl_without_color() {
        assert_diagnostic_snapshot! {r#"
            error Failure { :failed }
            trait Marker[a] { fn pass(value: a) a { value } }
            impl Marker[Failure] {}
        "#};
    }

    #[test]
    fn user_result_module_does_not_grant_error_tag_permission() {
        let uri = url("project/src/Result.ald");
        let source = indoc::indoc! {r#"
            fn err(value: a) a { value }
            pub fn main() { err(:failed) }
        "#};
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("a user module named Result must not gain built-in error tag permission");
        };
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(
            diagnostics[0].message(),
            "error tags are only values inside `Err`"
        );
    }

    #[test]
    fn imported_error_groups_reject_custom_implementations() {
        assert_imported_error_group_impl_rejected(indoc::indoc! {r#"
            import ~/model.{ Failure }
            trait Marker[a] { fn pass(value: a) a { value } }
            impl Marker[Failure] {}
        "#});
    }

    #[test]
    fn imported_error_group_aliases_reject_custom_implementations() {
        assert_imported_error_group_impl_rejected(indoc::indoc! {r#"
            import ~/model.{ Alias }
            trait Marker[a] { fn pass(value: a) a { value } }
            impl Marker[Alias] {}
        "#});
    }

    #[test]
    fn stored_error_group_alias_rejects_custom_implementation() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                pub error Failure { :failed }
                pub type Alias = Failure
            "#},
            &[],
            &[],
        );
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored = bincode::deserialize(&bytes).unwrap();
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ Alias }
            fn unrelated() Number { 42 }
            trait Marker[a] { fn pass(value: a) a { value } }
            impl Marker[Alias] {}
        "#};
        let uri = url("project/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies {
                interfaces: vec![stored],
                ..BuildDependencies::default()
            },
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("custom stored error-group implementation unexpectedly compiled");
        };
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(
            diagnostics[0].message(),
            "cannot define a custom trait implementation for error group `Failure`"
        );
        assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
    }

    #[test]
    fn stored_sparse_tuple_constraints_check_consumer_shapes() {
        let mut producer = dependency_interface(
            "pub fn accept(value: a, sample: Number) a { value }",
            &[],
            &[],
        );
        let crate::interface::OwnedType::Fn { params, .. } = &producer.values[0].scheme.typ.typ
        else {
            panic!("function");
        };
        let tuple = params[0].clone();
        let element = params[1].clone();
        producer.values[0]
            .scheme
            .tuple_shapes
            .push(crate::interface::OwnedTupleShape {
                tuple,
                length: 2,
                elements: vec![(1, element)],
                region: alder_region::Region::zero(),
            });
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let uri = url("project/src/main.ald");
        for (source, accepted) in [
            (
                indoc::indoc! {r#"
                import @vendor/widgets.{ accept }
                    pub fn main() {
                        accept((true, 42), 0)
                        accept(("independent", 42), 0)
                    }
            "#},
                true,
            ),
            (
                indoc::indoc! {r#"
                import @vendor/widgets.{ accept }
                pub fn main() { accept((true, "wrong"), 0) }
            "#},
                false,
            ),
            (
                indoc::indoc! {r#"
                import @vendor/widgets.{ accept }
                pub fn main() { accept((true, 42, false), 0) }
            "#},
                false,
            ),
            (
                indoc::indoc! {r#"
                    import @vendor/widgets.{ accept }
                    pub fn main() { accept(42, 0) }
                "#},
                false,
            ),
            (
                indoc::indoc! {r#"
                    import @vendor/widgets.{ accept }
                    fn relay(value) { accept(value, 0) }
                    pub fn main() { relay((true, "wrong")) }
                "#},
                false,
            ),
            (
                indoc::indoc! {r#"
                    import @vendor/widgets.{ accept }
                    pub fn relay(value: a) a { accept(value, 0) }
                "#},
                false,
            ),
        ] {
            let result = build_fixture_sync(
                vec![(uri.clone(), Ok(source.to_owned()))],
                BuildMode::Check,
                BuildDependencies {
                    interfaces: vec![stored.clone()],
                    ..BuildDependencies::default()
                },
            );
            assert_eq!(
                result.is_success(),
                accepted,
                "{source}\n{:#?}",
                result.modules
            );
            if !accepted {
                let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
                    panic!("consumer diagnostic");
                };
                assert!(
                    diagnostics
                        .iter()
                        .all(|diagnostic| diagnostic.source().text() == source)
                );
            }
        }
    }

    #[test]
    fn source_sparse_tuple_constraints_survive_stored_forwarders() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                pub fn preserve(value) {
                    let first = value.0
                    value
                }
            "#},
            &["tuples"],
            &[],
        );
        let relay = dependency_interface(
            indoc::indoc! {r#"
            import ~/tuples.{ preserve }
            pub fn relay(value) { preserve(value) }
        "#},
            &[],
            std::slice::from_ref(&producer),
        );
        assert!(!relay.values[0].scheme.tuple_shapes.is_empty());
        let bytes = bincode::serialize(&vec![producer, relay]).unwrap();
        let stored: Vec<InterfaceFile> = bincode::deserialize(&bytes).unwrap();
        let uri = url("project/src/main.ald");
        for (source, accepted) in [
            (
                indoc::indoc! {r#"
                import @vendor/widgets.{ relay }
                pub fn main() {
                    let first: (Number, String) = relay((42, "text"))
                    let second: (Bool, Number) = relay((true, 7))
                    (first, second)
                }
            "#},
                true,
            ),
            (
                indoc::indoc! {r#"
                import @vendor/widgets.{ relay }
                pub fn main() { relay((42, "text", false)) }
            "#},
                false,
            ),
            (
                indoc::indoc! {r#"
                import @vendor/widgets.{ relay }
                pub fn main() {
                    let wrong: (Number, Bool) = relay((42, "text"))
                    wrong
                }
            "#},
                false,
            ),
        ] {
            let result = build_fixture_sync(
                vec![(uri.clone(), Ok(source.to_owned()))],
                BuildMode::Check,
                BuildDependencies {
                    interfaces: stored.clone(),
                    ..BuildDependencies::default()
                },
            );
            assert_eq!(
                result.is_success(),
                accepted,
                "{source}\n{:#?}",
                result.modules
            );
        }
    }

    #[test]
    fn stored_maximum_tuple_projection_reports_compact_consumer_error() {
        let producer = dependency_interface("pub fn last(value) { value.4294967295 }", &[], &[]);
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored = bincode::deserialize(&bytes).unwrap();
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ last }
            pub fn main() { last((1, 2)) }
        "#};
        let uri = url("project/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies {
                interfaces: vec![stored],
                ..BuildDependencies::default()
            },
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("wrong fixed tuple length must reject");
        };
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message().contains("4294967296"));
        assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
    }

    fn assert_imported_error_group_impl_rejected(source: &str) {
        let model = url("project/src/model.ald");
        let implementation = url("project/src/implementation.ald");
        let sources = vec![
            (
                model,
                Ok(indoc::indoc! {r#"
                pub error Failure { :failed }
                pub type Alias = Failure
            "#}
                .to_owned()),
            ),
            (implementation.clone(), Ok(source.to_owned())),
        ];
        for reverse in [false, true] {
            let mut sources = sources.clone();
            if reverse {
                sources.reverse();
            }
            let result =
                build_fixture_sync(sources, BuildMode::Check, BuildDependencies::default());
            let ModuleResult::Failed { diagnostics } = &result.modules[&implementation] else {
                panic!("custom imported error-group implementation unexpectedly compiled");
            };
            assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
            let diagnostic = &diagnostics[0];
            assert_eq!(
                diagnostic.message(),
                "cannot define a custom trait implementation for error group `Failure`"
            );
            assert_eq!(diagnostic.source().text(), source);
            assert!(diagnostic.source().name().ends_with("/implementation.ald"));
            let labels = miette::Diagnostic::labels(diagnostic)
                .expect("implementation has a source label")
                .collect::<Vec<_>>();
            assert_eq!(labels.len(), 1);
            let label = &labels[0];
            assert_eq!(
                &source[label.offset()..label.offset() + label.len()],
                source.lines().last().expect("implementation line")
            );
        }
    }

    #[tokio::test]
    async fn renders_recursive_error_group_without_color() {
        assert_diagnostic_snapshot! {r#"
            error Recursive { :nested(Result[Number, Recursive]) }
        "#};
    }

    #[tokio::test]
    async fn renders_bodyless_trait_invalid_result_error_without_color() {
        assert_diagnostic_snapshot! {r#"
            pub trait Read[a] {
                fn read(value: a) Result[Number, String]
            }
        "#};
    }

    #[tokio::test]
    async fn renders_unused_invalid_result_alias_without_color() {
        assert_diagnostic_snapshot! {r#"
            pub type Invalid = Result[Number, String]
        "#};
    }

    #[tokio::test]
    async fn renders_missing_alternative_binding_without_color() {
        assert_diagnostic_snapshot! {r#"
            fn read(input: Option[Number]) Number {
                match input { Some(value) | None => value }
            }
        "#};
    }

    #[tokio::test]
    async fn renders_extra_alternative_binding_without_color() {
        assert_diagnostic_snapshot! {r#"
            fn read(input: Option[Number]) Number {
                match input { None | Some(value) => 0 }
            }
        "#};
    }

    #[test]
    fn synchronized_update_requires_a_task_returning_callback() {
        let source = indoc::indoc! {r#"
            pub async fn main() {
                let cell = SynchronizedRef.make(0).await
                SynchronizedRef.update(cell, value -> value + 1).await
            }
        "#};
        let result = build_fixture_sync(
            vec![(url("project/src/main.ald"), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        assert!(!result.is_success());
        assert!(result.artifacts.is_empty() && result.interfaces.is_empty());
        let ModuleResult::Failed { diagnostics } = &result.modules[&url("project/src/main.ald")]
        else {
            panic!("synchronous callback must not satisfy a task contract")
        };
        assert_eq!(diagnostics.len(), 1);
        assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
    }

    #[tokio::test]
    async fn structural_error_json_reports_missing_payload_codec() {
        assert_diagnostic_snapshot! {r#"
            pub fn encode(value: Result[Number, [:callback(fn(Number) Number)]]) String {
                Json.encode(value)
            }
        "#};
    }

    #[tokio::test]
    async fn structural_error_show_reports_missing_payload_capability() {
        assert_diagnostic_snapshot! {r#"
            error Failed { :callback(fn(Number) Number) }
            pub fn render(value: Result[Number, Failed]) String { show(value) }
        "#};
    }

    #[tokio::test]
    async fn fiber_for_each_requires_unit_callback_results() {
        assert_diagnostic_snapshot! {r#"
            pub async fn main() {
                Fiber.forEach([1, 2], value -> async { value + 1 }).await
            }
        "#};
    }

    #[tokio::test]
    async fn fiber_map_requires_task_callback_results() {
        assert_diagnostic_snapshot! {r#"
            pub async fn main() {
                Fiber.map([1, 2], value -> value + 1).await
            }
        "#};
    }

    #[tokio::test]
    async fn renders_contradictory_associative_record_overlays_without_color() {
        assert_diagnostic_snapshot! {r#"
            fn invalid(left, middle, right) {
                let first = { ..{ ..left, ..middle }, ..right }
                let second = { ..left, ..{ ..middle, ..right } }
                let number: Number = first.value
                let text: String = second.value
                (number, text)
            }
        "#};
    }

    #[test]
    fn stored_synchronization_contracts_preserve_inference_and_reject_shared_writes() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                pub async fn fresh(value: a) SynchronizedRef[a] {
                    SynchronizedRef.make(value).await
                }
                pub async fn gate() Semaphore { Semaphore.make(2).await }
                pub fn protect(gate: Semaphore, task: Task[a]) Task[a] {
                    Semaphore.withPermits(gate, 1, task)
                }
                let payload = []
                pub fn shared() { SynchronizedRef.make(payload) }
                pub async fn specialize() {
                    let cell = shared().await
                    Array.push(SynchronizedRef.get(cell).await, 42)
                }
            "#},
            &[],
            &[],
        );
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ fresh, gate, protect, shared }
            pub async fn main() String {
                let semaphore = gate().await
                let number = protect(semaphore, fresh(42)).await
                let text = protect(semaphore, fresh("hello")).await
                SynchronizedRef.update(number, value -> async { value + 1 }).await
                let cell = shared().await
                Array.push(SynchronizedRef.get(cell).await, SynchronizedRef.get(number).await)
                SynchronizedRef.get(text).await
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
            import @vendor/widgets.{ shared }
            pub async fn main() {
                let cell = shared().await
                SynchronizedRef.set(cell, ["wrong"]).await
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
        assert!(result.artifacts.is_empty() && result.interfaces.is_empty());
        let ModuleResult::Failed { diagnostics } = &result.modules[&url("project/src/main.ald")]
        else {
            panic!("stored shared cell contract must reject String payloads")
        };
        assert_eq!(diagnostics.len(), 1);
        assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
    }

    #[test]
    fn stored_structural_error_show_preserves_generic_payload_bounds() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                pub fn describe(value: Result[Number, [:bad(a) | :missing]]) String
                    where a: Show { show(value) }
            "#},
            &[],
            &[],
        );
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ describe }
            error Local { :missing, :bad(Number) }
            pub fn main() {
                let number: Result[Number, Local] = Err(:bad(42))
                describe(number)
                describe(Err(:bad("text")))
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
            import @vendor/widgets.{ describe }
            pub fn main() String { describe(Err(:bad((value: Number) -> value))) }
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
        assert!(result.artifacts.is_empty() && result.interfaces.is_empty());
        let ModuleResult::Failed { diagnostics } = &result.modules[&url("project/src/main.ald")]
        else {
            panic!("stored structural Show must retain its payload bound")
        };
        assert_eq!(diagnostics.len(), 1);
        assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
    }

    #[test]
    fn stored_structural_error_hash_preserves_generic_payload_bounds() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                pub fn fingerprint(value: Result[Number, [:bad(a) | :missing]]) BigInt
                    where a: Hash { hash(value) }
                pub fn equal(left: Result[Number, [:bad(a) | :missing]], right: Result[Number, [:bad(a) | :missing]]) Bool
                    where a: Hash { left == right }
            "#},
            &[],
            &[],
        );
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ fingerprint, equal }
            error Local { :missing, :bad(Number) }
            pub fn main() {
                let number: Result[Number, Local] = Err(:bad(42))
                fingerprint(number)
                fingerprint(Err(:bad("text")))
                equal(number, number)
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
            import @vendor/widgets.{ fingerprint }
            pub fn main() BigInt { fingerprint(Err(:bad((value: Number) -> value))) }
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
        assert!(result.artifacts.is_empty() && result.interfaces.is_empty());
        let ModuleResult::Failed { diagnostics } = &result.modules[&url("project/src/main.ald")]
        else {
            panic!("stored structural Hash must retain its payload bound")
        };
        assert_eq!(diagnostics.len(), 1);
        assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
    }

    #[test]
    fn stored_traversal_function_values_preserve_optional_slots_and_callback_types() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                pub fn mapping() { Fiber.map }
                pub fn each() { Fiber.forEach }
                pub fn trying() { Fiber.tryMap }
                pub fn tryingEach() { Fiber.tryForEach }
            "#},
            &[],
            &[],
        );
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ mapping, each, trying, tryingEach }
            async fn checked(value: Number) Result[Number, [:bad]] { Ok(value) }
            async fn checkedUnit(value: Number) Result[(), [:bad]] { Ok(()) }
            pub async fn main() {
                let mapper = mapping()
                let numbers: Array[Number] = mapper([1], value -> async { value }).await
                let texts: Array[String] = mapping()(["text"], value -> async { value }, { concurrency: 2 }).await
                each()(numbers, value -> async { () }).await
                each()(texts, value -> async { () }, None).await
                let results: Result[Array[Number], [:bad]] = trying()(numbers, checked, {}).await
                let done: Result[(), [:bad]] = tryingEach()(numbers, checkedUnit).await
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
            import @vendor/widgets.{ each }
            pub async fn main() {
                each()([1], value -> async { value }).await
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
        assert!(result.artifacts.is_empty() && result.interfaces.is_empty());
        let ModuleResult::Failed { diagnostics } = &result.modules[&url("project/src/main.ald")]
        else {
            panic!("stored forEach must retain its unit callback requirement")
        };
        assert_eq!(diagnostics.len(), 1);
        assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
    }

    #[test]
    fn stored_builtin_alias_contracts_preserve_record_payloads() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                pub type Config = Fiber::MapOptions
                pub fn read(options: Config) Option[Number] { options.concurrency }
            "#},
            &[],
            &[],
        );
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ Config, read }
            pub fn main() Option[Number] {
                let options: Config = { concurrency: 8 }
                read({})
                read(options)
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
            import @vendor/widgets.{ read }
            pub fn main() Option[Number] { read({ concurrency: "eight" }) }
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
            panic!("stored builtin alias must retain its numeric payload")
        };
        assert_eq!(diagnostics.len(), 1);
        assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
    }

    #[test]
    fn stored_ref_contracts_preserve_generic_allocations_and_restrict_shared_payloads() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                pub async fn fresh(value: a) Ref[a] { Ref.make(value).await }
                let payload = []
                pub fn shared() { Ref.make(payload) }
                pub async fn specialize() {
                    let cell = shared().await
                    Array.push(Ref.get(cell).await, 42)
                }
            "#},
            &[],
            &[],
        );
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ fresh, shared }
            pub async fn main() String {
                let number = fresh(42).await
                let text = fresh("hello").await
                let cell = shared().await
                Array.push(Ref.get(cell).await, Ref.get(number).await)
                Ref.get(text).await
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
            import @vendor/widgets.{ shared }
            pub async fn main() {
                let cell = shared().await
                Ref.set(cell, ["wrong"]).await
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
            panic!("shared Ref payload must retain its checked type")
        };
        assert_eq!(diagnostics.len(), 1);
        assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
    }

    #[test]
    fn stored_option_lifting_preserves_a_universal_relay_contract() {
        let bytes = {
            let producer = dependency_interface(
                indoc::indoc! {r#"
                    fn consume(value?: a) {}
                    pub fn relay(value: a) a {
                        consume(value)
                        value
                    }
                "#},
                &[],
                &[],
            );
            bincode::serialize(&producer).unwrap()
        };
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ relay }
            pub fn main() (Number, String, Option[Number]) {
                (relay(42), relay("hello"), relay(None))
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
        assert!(result.is_success(), "{:#?}", result.modules);
    }

    #[test]
    fn stored_trait_overlay_methods_preserve_independent_row_arguments() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                fn merge(left, right) { { ..left, ..right } }
                pub trait Read[a] {
                    fn read(marker: a, left: { r | value: Number }, right: { s | other: Bool }) Number {
                        let result = { ..merge(left, right), value: 42 }
                        result.value
                    }
                }
                impl Read[Number] {}
                impl Read[String] {
                    fn read(marker: String, left: { r | value: Number }, right: { s | other: Bool }) Number {
                        let result = { ..merge(left, right), value: 7 }
                        result.value
                    }
                }
            "#},
            &[],
            &[],
        );
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ read }
            pub fn main() (Number, Number) {
                (
                    read(0, { value: 0, extra: true }, { value: "discarded", other: false }),
                    read("marker", { value: 1, extra: "text" }, { value: false, other: true }),
                )
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
        assert!(result.is_success(), "{:#?}", result.modules);
    }

    #[test]
    fn stored_local_annotation_contracts_preserve_recursive_polymorphism() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                pub fn first(value: a, count: Number) a {
                    let local: a = value
                    if count == 0 { local } else { second(local, count - 1) }
                }
                pub fn second(value: b, count: Number) b {
                    let local: b = value
                    if count == 0 { local } else { first(local, count - 1) }
                }
            "#},
            &[],
            &[],
        );
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ first, second }
            pub fn main() (Number, String) {
                (first(42, 3), second("text", 4))
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
        assert!(result.is_success(), "{:#?}", result.modules);
    }

    #[test]
    fn stored_method_contract_preserves_independent_universals() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                pub trait Select[a] {
                    fn select(value: a, other: b) b { other }
                }
                impl Select[Number] {
                    fn select(value: Number, other: b) b { other }
                }
                impl Select[String] {}
                pub fn identity(value: a) a { value }
            "#},
            &[],
            &[],
        );
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ select, identity }
            pub fn main() (Number, String, Number, String) {
                (
                    identity(select(0, 42)),
                    identity(select(0, "hello")),
                    select("default", 7),
                    select("default", "world"),
                )
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
        assert!(result.is_success(), "{:#?}", result.modules);
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
    fn stored_sparse_tuple_capture_keeps_shared_payload_monomorphic() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                let shared: Array[Number] = []
                pub fn expose(pair) {
                    pair.0 = shared
                    pair
                }
            "#},
            &[],
            &[],
        );
        assert!(!producer.values[0].scheme.tuple_shapes.is_empty());
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        for (source, valid) in [
            (
                indoc::indoc! {r#"
                    import @vendor/widgets.{ expose }
                    pub fn check() {
                        let first = expose(([], "kept"))
                        let second = expose(([], true))
                        Array.push(first.0, 42)
                        Array.push(second.0, 7)
                        let text: String = first.1
                        let flag: Bool = second.1
                    }
                "#},
                true,
            ),
            (
                indoc::indoc! {r#"
                    import @vendor/widgets.{ expose }
                    pub fn invalid() {
                        let pair = expose(([], "kept"))
                        Array.push(pair.0, "wrong")
                    }
                "#},
                false,
            ),
        ] {
            let result = build_fixture_sync(
                vec![(url("project/src/main.ald"), Ok(source.to_owned()))],
                BuildMode::Check,
                BuildDependencies {
                    interfaces: vec![stored.clone()],
                    ..BuildDependencies::default()
                },
            );
            assert_eq!(result.is_success(), valid, "{:#?}", result.modules);
            if !valid {
                assert!(result.interfaces.is_empty());
                assert!(result.artifacts.is_empty());
            }
        }
    }

    #[test]
    fn stored_joint_constraints_preserve_shared_error_payloads() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                let shared = [42]
                fn expose(pair, patch) {
                    pair.0 = { ..{ value: Err(:saved(shared)) }, ..patch }
                    pair
                }
                pub fn propagate(pair, patch) {
                    let value = expose(pair, patch).0.value?
                    Ok(value)
                }
            "#},
            &[],
            &[],
        );
        let scheme = &producer.values[0].scheme;
        assert!(!scheme.tuple_shapes.is_empty());
        assert!(!scheme.record_overlays.is_empty());
        assert!(!scheme.error_row_inclusions.is_empty());
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ propagate }
            pub fn numbers() Result[Number, [:saved(Array[Number])]] {
                propagate(({ value: Err(:saved([])) }, ()), {})
            }
            pub fn strings() Result[String, [:saved(Array[Number])]] {
                propagate(({ value: Err(:saved([])) }, true), {})
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
            import @vendor/widgets.{ propagate }
            pub fn invalid() Result[Number, [:saved(Array[String])]] {
                propagate(({ value: Err(:saved([])) }, ()), {})
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
            panic!("stored joint constraints must retain the captured Number array")
        };
        assert_eq!(diagnostics.len(), 1);
        assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
    }

    #[test]
    fn stored_traits_supply_default_helpers_to_new_implementations() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                pub trait Format[a] where a: Show {
                    async fn format(value: a, other: b) String where b: Show {
                        Task.sleep(0).await
                        String.concat(show(value), show(other))
                    }
                }
            "#},
            &[],
            &[],
        );
        assert_eq!(
            producer.traits[0].methods[0].default_symbol.as_deref(),
            Some("$default$Format$format")
        );
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ Format }
            #[derive(Show)]
            pub enum Local { Local }
            impl Format[Local] {}
            pub async fn main() String { Format::format(Local::Local, 42).await }
        "#};
        let result = build_fixture_sync(
            vec![(url("project/src/main.ald"), Ok(source.to_owned()))],
            BuildMode::Build,
            BuildDependencies {
                interfaces: vec![stored],
                ..BuildDependencies::default()
            },
        );
        assert!(result.is_success(), "{:#?}", result.modules);
        let code = result.artifacts[&url("project/src/main.ald")].code();
        assert!(
            code.contains("import { $default$Format$format as "),
            "{code}"
        );
        assert!(
            code.contains("[\"format\"] = ($methodDict0, $a0, $a1)"),
            "{code}"
        );
        assert!(result.interfaces.iter().any(|interface| {
            interface.instances.iter().any(|implementation| {
                implementation
                    .methods
                    .iter()
                    .any(|(method, implementation)| {
                        method.name == "format"
                            && matches!(implementation,
                            crate::interface::OwnedMethodImplementation::Default { symbol }
                            if symbol == "$default$Format$format")
                    })
            })
        }));
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
    fn stored_reexport_aliases_preserve_shared_state_and_safe_polymorphism() {
        let leaf = dependency_interface(
            indoc::indoc! {"
                pub let shared: Array[Number] = []
                pub let identity = value -> value
                let operation = value -> value
                pub fn forward(value) { operation(value) }
                pub let forwarded = forward
                pub fn specialize() { operation = (value: Number) -> value + 1 }
            "},
            &["leaf"],
            &[],
        );
        let named = dependency_interface(
            "pub import ~/leaf.{ shared as storage, identity as same, forwarded as call, specialize }",
            &["named"],
            &[leaf],
        );
        let facade = dependency_interface("pub import ~/named.*", &["facade"], &[named]);
        let bytes = bincode::serialize(&facade).unwrap();
        drop(facade);
        for (source, valid) in [
            (
                indoc::indoc! {r#"
                import @vendor/widgets/facade.{ storage, same, call, specialize }
                pub fn valid() {
                    Array.push(storage, 42)
                    let number: Number = same(42)
                    let text: String = same("text")
                    specialize()
                    let result: Number = call(number)
                    (result, text)
                }
            "#},
                true,
            ),
            (
                indoc::indoc! {r#"
                import @vendor/widgets/facade.{ storage }
                pub fn invalid() { Array.push(storage, "wrong") }
            "#},
                false,
            ),
            (
                indoc::indoc! {r#"
                import @vendor/widgets/facade.{ call }
                pub fn invalid() String { call("wrong") }
            "#},
                false,
            ),
        ] {
            let result = build_fixture_sync(
                vec![(url("project/src/main.ald"), Ok(source.to_owned()))],
                BuildMode::Check,
                BuildDependencies {
                    interfaces: vec![bincode::deserialize(&bytes).unwrap()],
                    ..BuildDependencies::default()
                },
            );
            assert_eq!(
                result.is_success(),
                valid,
                "{source}\n{:#?}",
                result.modules
            );
            if !valid {
                assert!(result.interfaces.is_empty());
                assert!(result.artifacts.is_empty());
                let ModuleResult::Failed { diagnostics } =
                    &result.modules[&url("project/src/main.ald")]
                else {
                    panic!("invalid alias consumer must fail")
                };
                assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
                assert!(
                    diagnostics[0].to_string().contains("type mismatch"),
                    "{diagnostics:?}"
                );
            }
        }
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
    fn stored_open_spread_defaults_accept_absence_and_check_overwrites() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                type Config = { value: Option[Number] }
                pub fn copy(record) Config { { ..record } }
            "#},
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
        for (source, expected_success) in [
            (
                indoc::indoc! {r#"
                import @vendor/widgets.{ copy }
                pub fn main() {
                    let absent = copy({})
                    let present = copy({ value: Some(42) })
                    assert(absent.value == None)
                    assert(present.value == Some(42))
                }
            "#},
                true,
            ),
            (
                indoc::indoc! {r#"
                import @vendor/widgets.{ copy }
                pub fn main() { copy({ value: Some("wrong") }) }
            "#},
                false,
            ),
        ] {
            let result = build_fixture_sync(
                vec![(url("project/src/main.ald"), Ok(source.to_owned()))],
                BuildMode::Check,
                BuildDependencies {
                    interfaces: vec![copied.clone()],
                    ..BuildDependencies::default()
                },
            );
            assert_eq!(
                result.is_success(),
                expected_success,
                "{:#?}",
                result.modules
            );
        }
    }

    #[test]
    fn stored_closed_overlay_grouping_cannot_hide_contradictory_consumer_contracts() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                pub fn split(record) { { ..record, x: 0, y: true } }
                pub fn grouped(record) { { ..record, ..{ x: 0, y: true } } }
            "#},
            &[],
            &[],
        );
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ split, grouped }
            pub fn invalid(record) {
                let first: Number = split(record).value
                let second: String = grouped(record).value
                (first, second)
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
            !result.is_success(),
            "contradictory consumer contract was accepted"
        );
        assert!(result.interfaces.is_empty());
        assert!(result.artifacts.is_empty());
        let ModuleResult::Failed { diagnostics } = &result.modules[&url("project/src/main.ald")]
        else {
            panic!("invalid consumer must fail")
        };
        assert_eq!(diagnostics.len(), 1);
        assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
    }

    #[test]
    fn stored_overwritten_overlay_fields_preserve_consumer_constraints() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                pub fn padded(left, right) { { ..left, ..{ x: "discarded", kept: true }, ..right, x: 0 } }
                pub fn plain(left, right) { { ..left, kept: true, ..right, x: 0 } }
            "#},
            &[],
            &[],
        );
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        for (source, expected_success) in [
            (
                indoc::indoc! {r#"
                import @vendor/widgets.{ padded, plain }
                pub fn invalid(left, right) {
                    let first: Number = padded(left, right).value
                    let second: String = plain(left, right).value
                    (first, second)
                }
            "#},
                false,
            ),
            (
                indoc::indoc! {r#"
                import @vendor/widgets.{ padded, plain }
                pub fn valid() {
                    let first = padded({ value: false }, { value: "right", kept: 42 })
                    let second = plain({ value: false }, { value: "right", kept: 42 })
                    let value: String = first.value
                    let kept: Number = first.kept
                    let x: Number = second.x
                    (value, kept, x)
                }
            "#},
                true,
            ),
        ] {
            let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
            let result = build_fixture_sync(
                vec![(url("project/src/main.ald"), Ok(source.to_owned()))],
                BuildMode::Check,
                BuildDependencies {
                    interfaces: vec![stored],
                    ..BuildDependencies::default()
                },
            );
            assert_eq!(
                result.is_success(),
                expected_success,
                "{:#?}",
                result.modules
            );
            if !expected_success {
                assert!(result.interfaces.is_empty());
                assert!(result.artifacts.is_empty());
            }
        }
    }

    #[test]
    fn stored_mutual_overlays_preserve_recursive_payload_requirements() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                fn merge(left, right) { { ..left, ..right } }
                pub fn even(count: Number, value, other) {
                    let merged = merge(value, other)
                    if count == 0 { merged }
                    else { odd(count - 1, { nested: merged }, other) }
                }
                pub fn odd(count: Number, value, other) {
                    let merged = merge(value, other)
                    if count == 0 { merged }
                    else { even(count - 1, { nested: merged }, other) }
                }
            "#},
            &[],
            &[],
        );
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        for (source, valid) in [
            (
                indoc::indoc! {r#"
                    import @vendor/widgets.{ even, odd }
                    pub fn run() {
                        let number: Number = even(2, { nested: { nested: 0 } }, { nested: 42 }).nested
                        let text: String = odd(3, { nested: { nested: "seed" } }, { nested: "text" }).nested
                        (number, text)
                    }
                "#},
                true,
            ),
            (
                indoc::indoc! {r#"
                    import @vendor/widgets.{ even }
                    pub fn run() { even(2, { nested: { nested: 0 } }, {}) }
                "#},
                false,
            ),
        ] {
            let result = build_fixture_sync(
                vec![(url("project/src/main.ald"), Ok(source.to_owned()))],
                BuildMode::Check,
                BuildDependencies {
                    interfaces: vec![bincode::deserialize(&bytes).unwrap()],
                    ..BuildDependencies::default()
                },
            );
            assert_eq!(
                result.is_success(),
                valid,
                "{source}\n{:#?}",
                result.modules
            );
            if !valid {
                assert!(result.interfaces.is_empty());
                assert!(result.artifacts.is_empty());
                let ModuleResult::Failed { diagnostics } =
                    &result.modules[&url("project/src/main.ald")]
                else {
                    panic!("invalid mutual overlay consumer must fail")
                };
                assert_eq!(diagnostics.len(), 1);
                assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
            }
        }
    }

    #[test]
    fn stored_recursive_overlays_preserve_cycle_breaking_requirements() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                fn merge(left, right) { { ..left, ..right } }
                pub fn repeat(count: Number, value, other) {
                    let merged = merge(value, other)
                    if count == 0 { merged }
                    else { repeat(count - 1, { nested: merged }, other) }
                }
            "#},
            &[],
            &[],
        );
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        for (source, valid) in [
            (
                indoc::indoc! {r#"
                import @vendor/widgets.{ repeat }
                pub fn run() Number {
                    repeat(2, { nested: { nested: 0 } }, { nested: 42 }).nested
                }
            "#},
                true,
            ),
            (
                indoc::indoc! {r#"
                import @vendor/widgets.{ repeat }
                pub fn run() {
                    repeat(2, { nested: { nested: 0 } }, {})
                }
            "#},
                false,
            ),
        ] {
            let result = build_fixture_sync(
                vec![(url("project/src/main.ald"), Ok(source.to_owned()))],
                BuildMode::Check,
                BuildDependencies {
                    interfaces: vec![bincode::deserialize(&bytes).unwrap()],
                    ..BuildDependencies::default()
                },
            );
            assert_eq!(result.is_success(), valid, "{source}");
            if !valid {
                assert!(result.interfaces.is_empty());
                assert!(result.artifacts.is_empty());
                let ModuleResult::Failed { diagnostics } =
                    &result.modules[&url("project/src/main.ald")]
                else {
                    panic!("invalid recursive overlay consumer must fail")
                };
                assert_eq!(diagnostics.len(), 1);
                assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
            }
        }
    }

    #[test]
    fn stored_known_open_overwrites_cannot_hide_contradictory_contracts() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                pub fn padded(left, right: { r | x: Number }) { { ..left, x: "discarded", ..right } }
                pub fn plain(left, right: { r | x: Number }) { { ..left, ..right } }
            "#},
            &[],
            &[],
        );
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let source = indoc::indoc! {"
            import @vendor/widgets.{ padded, plain }
            pub fn invalid(left, right) {
                let first: Number = padded(left, right).value
                let second: String = plain(left, right).value
                (first, second)
            }
        "};
        let result = build_fixture_sync(
            vec![(url("project/src/main.ald"), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies {
                interfaces: vec![bincode::deserialize(&bytes).unwrap()],
                ..BuildDependencies::default()
            },
        );
        assert!(
            !result.is_success(),
            "contradictory imported overlays accepted"
        );
        assert!(result.interfaces.is_empty());
        assert!(result.artifacts.is_empty());
        let ModuleResult::Failed { diagnostics } = &result.modules[&url("project/src/main.ald")]
        else {
            panic!("invalid consumer must fail")
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
    fn stored_associative_overlays_preserve_result_relationships() {
        let producer = dependency_interface(
            indoc::indoc! {r#"
                pub fn both(left, middle, right) {
                    ({ ..{ ..left, ..middle }, ..right }, { ..left, ..{ ..middle, ..right } })
                }
            "#},
            &[],
            &[],
        );
        assert!(!producer.values[0].scheme.record_overlays.is_empty());
        let bytes = bincode::serialize(&producer).unwrap();
        drop(producer);
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let source = indoc::indoc! {r#"
            import @vendor/widgets.{ both }
            pub fn numbers() (Number, Number) {
                let records = both({ value: "old" }, { value: true }, { value: 42 })
                (records.0.value, records.1.value)
            }
            pub fn strings() (String, String) {
                let records = both({ value: 42 }, { value: true }, { value: "last" })
                (records.0.value, records.1.value)
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
            import @vendor/widgets.{ both }
            pub fn invalid(left, middle, right) (Number, String) {
                let records = both(left, middle, right)
                (records.0.value, records.1.value)
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
        assert!(result.artifacts.is_empty() && result.interfaces.is_empty());
        let ModuleResult::Failed { diagnostics } = &result.modules[&url("project/src/main.ald")]
        else {
            panic!("equivalent stored results must not acquire contradictory fields")
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
    fn reexport_type_trait_collisions_reject_both_import_orders() {
        for (index, source) in [
            indoc::indoc! {"
                pub import ~/left.*
                pub import ~/right.*
            "},
            indoc::indoc! {"
                pub import ~/right.*
                pub import ~/left.*
            "},
            indoc::indoc! {"
                pub import ~/left.{ Shared as Renamed }
                pub import ~/right.{ Shared as Renamed }
            "},
            indoc::indoc! {"
                pub import ~/right.{ Shared as Renamed }
                pub import ~/left.{ Shared as Renamed }
            "},
        ]
        .into_iter()
        .enumerate()
        {
            for declaration in ["pub type Shared = Number", "pub enum Shared { Value }"] {
                let facade = url("project/src/facade.ald");
                let result = build_fixture_sync(
                    vec![
                        (url("project/src/left.ald"), Ok(declaration.to_owned())),
                        (
                            url("project/src/right.ald"),
                            Ok("pub trait Shared[a] { fn read(value: a) Number }".to_owned()),
                        ),
                        (facade.clone(), Ok(source.to_owned())),
                    ],
                    BuildMode::Build,
                    BuildDependencies::default(),
                );
                assert!(!result.is_success(), "collision accepted: {source}");
                assert!(!result.artifacts.contains_key(&facade));
                assert!(
                    !result
                        .interfaces
                        .iter()
                        .any(|interface| interface.module.path == ["facade"])
                );
                let ModuleResult::Failed { diagnostics } = &result.modules[&facade] else {
                    panic!("colliding facade must fail")
                };
                assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
                if index == 0 && declaration.starts_with("pub type") {
                    assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
                }
            }
        }
    }

    #[test]
    fn reexport_type_trait_distinct_aliases_remain_usable() {
        let alias = dependency_interface("pub type Shared = Number", &["left"], &[]);
        let trait_ = dependency_interface(
            "pub trait Shared[a] { fn read(value: a) Number }",
            &["right"],
            &[],
        );
        let facade = dependency_interface(
            indoc::indoc! {"
                pub import ~/left.{ Shared as Payload }
                pub import ~/right.{ Shared as Reader }
            "},
            &["facade"],
            &[alias, trait_],
        );
        let bytes = bincode::serialize(&facade).unwrap();
        drop(facade);
        let source = indoc::indoc! {"
            import @vendor/widgets/facade.{ Payload, Reader }
            pub fn use_reader(value: a) Payload where a: Reader { Reader::read(value) }
        "};
        let result = build_fixture_sync(
            vec![(url("project/src/main.ald"), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies {
                interfaces: vec![bincode::deserialize(&bytes).unwrap()],
                ..BuildDependencies::default()
            },
        );
        assert!(result.is_success(), "{:#?}", result.modules);
    }

    #[test]
    fn named_reexports_cannot_rename_private_type_or_trait_declarations() {
        let leaf = indoc::indoc! {"
            type SecretAlias = Number
            enum SecretEnum { Hidden }
            trait SecretTrait[a] { fn secret_method(value: a) Number }
            pub fn visible() Number { 42 }
        "};
        for source in [
            "pub import ~/leaf.{ SecretAlias as PublicAlias }",
            "pub import ~/leaf.{ SecretEnum as PublicEnum }",
            "pub import ~/leaf.{ SecretTrait as PublicTrait }",
            "pub import ~/leaf.{ secret_method as public_method }",
        ] {
            let facade = url("project/src/facade.ald");
            let result = build_fixture_sync(
                vec![
                    (url("project/src/leaf.ald"), Ok(leaf.to_owned())),
                    (facade.clone(), Ok(source.to_owned())),
                ],
                BuildMode::Build,
                BuildDependencies::default(),
            );
            assert!(!result.is_success(), "private re-export accepted: {source}");
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
            assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
            let expected = if source.contains("secret_method") {
                "does not export `secret_method`"
            } else {
                "private"
            };
            assert!(
                diagnostics[0].to_string().contains(expected),
                "{diagnostics:?}"
            );
        }
    }

    #[tokio::test]
    async fn public_reexport_cycles_fail_graph_construction_deterministically() {
        let files = [
            (
                url("project/src/left.ald"),
                "pub import ~/right.*".to_owned(),
            ),
            (
                url("project/src/right.ald"),
                "pub import ~/left.{ answer }".to_owned(),
            ),
            (url("project/src/main.ald"), "import ~/left".to_owned()),
        ];
        let mut expected = None;
        for iteration in 0..6 {
            let mut ordered = files.to_vec();
            ordered.rotate_left(iteration % 3);
            if iteration >= 3 {
                ordered.reverse();
            }
            let modules = ordered
                .iter()
                .map(|(uri, _)| uri.clone())
                .collect::<Vec<_>>();
            let db = Arc::new(Mutex::new(Database::new(InMemorySource::with_files(
                ordered,
            ))));
            let error = fixture_graph(db, &modules)
                .await
                .expect_err("public imports cannot bypass cycle rejection");
            assert!(
                matches!(error, DriverError::ImportCycle { .. }),
                "{error:?}"
            );
            let rendered = error.to_string();
            assert!(rendered.contains("left -> right -> left"), "{rendered}");
            if let Some(expected) = &expected {
                assert_eq!(&rendered, expected);
            } else {
                expected = Some(rendered);
            }
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
    fn deferred_declarations_publish_only_provisional_types() {
        let producer = url("project/src/data.ald");
        let consumer = url("project/src/main.ald");
        let source = indoc::indoc! {r#"
            pub table users {}
            pub schema SignUp from users {}
            pub macro identity(x) { x }
            pub fn answer() Number { 42 }
        "#};
        for mode in [BuildMode::Check, BuildMode::Build, BuildMode::Test] {
            let result = build_fixture_sync(
                vec![
                    (producer.clone(), Ok(source.to_owned())),
                    (
                        consumer.clone(),
                        Ok(indoc::indoc! {r#"
                        import ~/data.{ SignUp, answer }
                        pub fn keep(value: SignUp) SignUp { value }
                        pub fn main() Number { answer() }
                    "#}
                        .to_owned()),
                    ),
                ],
                mode,
                BuildDependencies::default(),
            );
            assert!(result.is_success(), "{:#?}", result.modules);
            let interface = result
                .interfaces
                .iter()
                .find(|interface| interface.module.path == ["data"])
                .unwrap();
            assert_eq!(
                interface
                    .values
                    .iter()
                    .map(|value| value.exported_as.as_str())
                    .collect::<Vec<_>>(),
                ["answer"]
            );
            assert_eq!(
                interface
                    .types
                    .iter()
                    .map(|typ| typ.reference.name.as_str())
                    .collect::<Vec<_>>(),
                ["users", "SignUp"]
            );
            if mode == BuildMode::Check {
                assert!(result.artifacts.is_empty());
            } else {
                let code = result.artifacts[&producer].code();
                assert!(
                    !code.contains("users")
                        && !code.contains("SignUp")
                        && !code.contains("identity"),
                    "{code}"
                );
            }
        }
    }

    #[test]
    fn deferred_type_names_cannot_be_used_as_runtime_values() {
        let producer = url("project/src/data.ald");
        let consumer = url("project/src/main.ald");
        let source = indoc::indoc! {r#"
            import ~/data
            pub fn main() { data.users }
        "#};
        let mut diagnostic = None;
        for mode in [BuildMode::Check, BuildMode::Build, BuildMode::Test] {
            let result = build_fixture_sync(
                vec![
                    (producer.clone(), Ok("pub table users {}".to_owned())),
                    (consumer.clone(), Ok(source.to_owned())),
                ],
                mode,
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
                panic!("runtime table reference must fail")
            };
            diagnostic = Some(diagnostics[0].clone());
        }
        assert_rendered_diagnostic_snapshot!(source, diagnostic.unwrap());
    }

    #[test]
    fn declared_macro_invocation_cannot_publish_a_runtime_stub() {
        let uri = url("project/src/main.ald");
        let source = indoc::indoc! {r#"
            macro identity(x) { x }
            pub fn main() { identity!(42) }
        "#};
        for mode in [BuildMode::Check, BuildMode::Build, BuildMode::Test] {
            let result = build_fixture_sync(
                vec![(uri.clone(), Ok(source.to_owned()))],
                mode,
                BuildDependencies::default(),
            );
            assert!(!result.is_success());
            assert!(result.artifacts.is_empty());
            assert!(result.interfaces.is_empty());
            let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
                panic!("macro invocation must fail before codegen")
            };
            assert!(!diagnostics.is_empty());
        }
    }

    #[test]
    fn unicode_before_deferred_expression_preserves_diagnostic_snippet() {
        let source = "pub fn view() { (\"café 😀\", <div />) }";
        let diagnostic = unavailable_codegen(source);
        let label = miette::Diagnostic::labels(&diagnostic)
            .unwrap()
            .find(|label| label.primary())
            .unwrap();
        assert_eq!(
            &source[label.offset()..label.offset() + label.len()],
            "<div />"
        );
        assert_rendered_diagnostic_snapshot!(source, diagnostic);
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

    #[test]
    fn superclass_cycle_labels_a_local_trait_when_the_last_trait_is_foreign() {
        let source = indoc::indoc! {r#"
            let marker = 42

            pub trait Local[a] { fn local(value: a) a }
        "#};
        let bump = Bump::new();
        let parsed = alder_parse::parse_module(&bump, source).unwrap();
        let canonical = alder_can::canonicalize(
            &bump,
            alder_can::Context {
                home: ModuleId {
                    package: PackageId::Application,
                    path: &["local"],
                },
                imports: &[],
                interfaces: &[],
            },
            &parsed,
        )
        .unwrap();
        let local = canonical
            .module
            .items
            .iter()
            .find_map(|item| match item.value.kind {
                alder_ast::ItemKind::Trait(declaration) => Some(declaration.id),
                _ => None,
            })
            .unwrap();
        let foreign = alder_ast::TraitId(alder_ast::QualifiedName {
            module: ModuleId {
                package: PackageId::Application,
                path: &["foreign"],
            },
            name: "Foreign",
        });
        let diagnostic = crate::report::solve(
            Source::new("local.ald", source),
            canonical.module,
            &[],
            &alder_solve::SolveError::Coherence(alder_solve::CoherenceError::SuperclassCycle {
                traits: &[local, foreign],
            }),
        );
        let labels = miette::Diagnostic::labels(&diagnostic)
            .unwrap()
            .collect::<Vec<_>>();
        assert_eq!(labels.len(), 1);
        assert!(
            labels[0].offset() > source.find("\n\n").unwrap(),
            "the label must point to the local trait, not the first source character"
        );
        assert_rendered_diagnostic_snapshot!(source, diagnostic);
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
            &[],
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
            &[],
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
            kind: alder_can::WarningKind::UnusedBinding {
                name: "unused",
                form: alder_can::BindingForm::Pattern,
            },
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

    #[test]
    fn stored_direct_error_group_annotations_preserve_structural_identity() {
        let bytes = {
            let producer = dependency_interface(
                indoc::indoc! {r#"
                pub error Failure { :bad(Number), :missing }
                pub fn relay(value: Failure) Failure { value }
                pub fn render(value: Failure) String { show(value) }
            "#},
                &[],
                &[],
            );
            bincode::serialize(&producer).unwrap()
        };
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let uri = url("project/src/main.ald");
        for (source, accepted) in [
            (
                indoc::indoc! {r#"
                import @vendor/widgets.{ relay, render }
                error Local { :missing, :bad(Number) }
                pub fn describe(value: Local) String { render(relay(value)) }
            "#},
                true,
            ),
            (
                indoc::indoc! {r#"
                import @vendor/widgets.{ relay, render }
                error Local { :missing, :bad(String) }
                pub fn describe(value: Local) String { render(relay(value)) }
            "#},
                false,
            ),
        ] {
            let result = build_fixture_sync(
                vec![(uri.clone(), Ok(source.to_owned()))],
                BuildMode::Check,
                BuildDependencies {
                    interfaces: vec![stored.clone()],
                    ..BuildDependencies::default()
                },
            );
            assert_eq!(result.is_success(), accepted, "{:?}", result.modules);
            if !accepted {
                let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
                    panic!("incompatible payloads must be diagnosed in the consumer");
                };
                assert_eq!(diagnostics.len(), 1);
                assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
            }
        }
    }

    #[test]
    fn stored_projection_cycle_is_rejected_without_application_source() {
        let valid = dependency_interface(
            indoc::indoc! {r#"
            pub enum Counter { Counter }
            pub trait Pair[i] {
                type Left
                type Right
            }
            impl Pair[Counter] {
                type Left = Number
                type Right = String
            }
        "#},
            &["associated"],
            &[],
        );
        let mut cyclic = valid.clone();
        let implementation = cyclic
            .instances
            .iter_mut()
            .find(|implementation| implementation.trait_ref.trait_.0.name == "Pair")
            .unwrap();
        let left = implementation.assoc_bindings[0].assoc.clone();
        let right = implementation.assoc_bindings[1].assoc.clone();
        for (binding, target) in implementation.assoc_bindings.iter_mut().zip([right, left]) {
            binding.typ.typ =
                crate::interface::OwnedType::Projection(crate::interface::OwnedProjection {
                    trait_ref: implementation.trait_ref.clone(),
                    assoc: target,
                });
        }
        let bump = Bump::new();
        let cyclic = InterfaceFile::dehydrate(&cyclic.hydrate(&bump)).unwrap();
        for (interface, accepted) in [(valid, true), (cyclic, false)] {
            let bytes = bincode::serialize(&interface).unwrap();
            let interface = bincode::deserialize(&bytes).unwrap();
            let result = build_fixture_sync(
                vec![],
                BuildMode::Check,
                BuildDependencies {
                    interfaces: vec![interface],
                    ..BuildDependencies::default()
                },
            );
            assert_eq!(result.is_success(), accepted);
            if !accepted {
                assert_eq!(result.diagnostics.len(), 1);
                let diagnostic = &result.diagnostics[0];
                assert!(diagnostic.message().contains("associated type cycle"));
                assert!(diagnostic.message().contains("Left"));
                assert!(diagnostic.message().contains("Right"));
                assert!(miette::Diagnostic::labels(diagnostic).is_none());
                let related = miette::Diagnostic::related(diagnostic)
                    .unwrap()
                    .collect::<Vec<_>>();
                assert!(related[0].to_string().contains("vendor/widgets/associated"));
                assert!(result.interfaces.is_empty());
                assert!(result.package_instance_indexes.is_empty());
            }
        }
    }

    #[test]
    fn stored_superclass_cycle_is_a_build_level_error_without_source_spans() {
        let valid = dependency_interface(
            indoc::indoc! {r#"
            pub trait First[a] { fn first(value: a) a }
            pub trait Second[a] where a: First { fn second(value: a) a }
        "#},
            &["hierarchy"],
            &[],
        );
        let mut cyclic = valid.clone();
        let first = cyclic
            .traits
            .iter()
            .position(|trait_| trait_.id.0.name == "First")
            .unwrap();
        let second = cyclic
            .traits
            .iter()
            .position(|trait_| trait_.id.0.name == "Second")
            .unwrap();
        let mut back_edge = cyclic.traits[second].superclasses[0].clone();
        back_edge.trait_ = cyclic.traits[second].id.clone();
        cyclic.traits[first].superclasses.push(back_edge);
        // Simulate semantically invalid stored metadata with a valid checksum:
        // persistence integrity must not substitute for coherence validation.
        let bump = Bump::new();
        let cyclic = InterfaceFile::dehydrate(&cyclic.hydrate(&bump)).unwrap();
        for (interface, accepted) in [(valid, true), (cyclic, false)] {
            let bytes = bincode::serialize(&interface).unwrap();
            let interface = bincode::deserialize(&bytes).unwrap();
            let result = build_fixture_sync(
                vec![],
                BuildMode::Build,
                BuildDependencies {
                    interfaces: vec![interface],
                    ..BuildDependencies::default()
                },
            );
            assert_eq!(result.is_success(), accepted);
            if !accepted {
                assert_eq!(result.diagnostics.len(), 1);
                let diagnostic = &result.diagnostics[0];
                assert!(diagnostic.message().contains("trait superclass cycle"));
                assert!(diagnostic.message().contains("First"));
                assert!(diagnostic.message().contains("Second"));
                assert!(miette::Diagnostic::labels(diagnostic).is_none());
                let related = miette::Diagnostic::related(diagnostic)
                    .unwrap()
                    .collect::<Vec<_>>();
                assert_eq!(related.len(), 1);
                assert!(related[0].to_string().contains("vendor/widgets/hierarchy"));
                assert!(result.artifacts.is_empty());
                assert!(result.interfaces.is_empty());
            }
        }
    }

    #[test]
    fn source_and_stored_dependency_overlap_retains_local_ownership() {
        let api = dependency_interface(
            indoc::indoc! {r#"
            pub enum Token { Token }
            pub trait Display[a] { fn display(value: a) String }
        "#},
            &["api"],
            &[],
        );
        let source = indoc::indoc! {r#"
            import @vendor/widgets/api.{ Token, Display }
            impl Display[Token] {
                fn display(value: Token) String { "token" }
            }
        "#};
        let stored = dependency_interface(source, &["stored"], std::slice::from_ref(&api));
        let index = PackageInstanceIndexFile::new(
            api.module.package.clone(),
            vec![stored.module.clone()],
            stored.instances,
        )
        .unwrap();
        let bytes = bincode::serialize(&(api, index)).unwrap();
        let (api, index): (InterfaceFile, PackageInstanceIndexFile) =
            bincode::deserialize(&bytes).unwrap();
        let live = url("project/dependency/live.ald");
        let main = url("project/src/main.ald");
        for path in ["a_live", "z_live"] {
            for include_stored in [false, true] {
                let result = build_fixture_sync(
                    vec![
                        (main.clone(), Ok("pub fn main() { 42 }".to_owned())),
                        (live.clone(), Ok(source.to_owned())),
                    ],
                    BuildMode::Build,
                    BuildDependencies {
                        module_packages: BTreeMap::from([(
                            live.clone(),
                            api.module.package.clone(),
                        )]),
                        module_paths: BTreeMap::from([(live.clone(), vec![path.to_owned()])]),
                        interfaces: vec![api.clone()],
                        package_instance_indexes: if include_stored {
                            vec![index.clone()]
                        } else {
                            vec![]
                        },
                        ..BuildDependencies::default()
                    },
                );
                if !include_stored {
                    assert!(
                        result.is_success(),
                        "a valid local implementation must still compile: {:?}",
                        result.modules
                    );
                    continue;
                }
                assert!(!result.is_success());
                assert!(
                    result.diagnostics.is_empty(),
                    "the same overlap must not also appear at build level"
                );
                assert!(matches!(result.modules[&main], ModuleResult::Blocked));
                let ModuleResult::Failed { diagnostics } = &result.modules[&live] else {
                    panic!("the source-side implementation must retain its diagnostic");
                };
                assert_eq!(diagnostics.len(), 1);
                assert_eq!(diagnostics[0].source().text(), source);
                assert_eq!(
                    miette::Diagnostic::labels(&diagnostics[0]).unwrap().count(),
                    1
                );
                assert!(
                    miette::Diagnostic::help(&diagnostics[0])
                        .unwrap()
                        .to_string()
                        .contains("vendor/widgets/stored")
                );
                assert!(result.artifacts.is_empty());
                assert!(result.interfaces.is_empty());
                assert!(result.package_instance_indexes.is_empty());
                if path == "a_live" {
                    assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
                }
            }
        }
    }

    #[test]
    fn stored_dependency_overlap_does_not_blame_application_sources() {
        let api = dependency_interface(
            indoc::indoc! {r#"
            pub enum Token { Token }
            pub trait Display[a] { fn display(value: a) String }
        "#},
            &["api"],
            &[],
        );
        let source = indoc::indoc! {r#"
            import @vendor/widgets/api.{ Token, Display }
            impl Display[Token] { fn display(value: Token) String { "token" } }
        "#};
        let first = dependency_interface(source, &["first"], std::slice::from_ref(&api));
        let second = dependency_interface(source, &["second"], std::slice::from_ref(&api));
        let index = PackageInstanceIndexFile::new(
            api.module.package.clone(),
            vec![
                api.module.clone(),
                first.module.clone(),
                second.module.clone(),
            ],
            first
                .instances
                .into_iter()
                .chain(second.instances)
                .collect(),
        )
        .unwrap();
        let bytes = bincode::serialize(&(api, index)).unwrap();
        let (api, index): (InterfaceFile, PackageInstanceIndexFile) =
            bincode::deserialize(&bytes).unwrap();
        let consumer = url("project/src/main.ald");
        let invalid = url("project/src/invalid.ald");
        let application = (consumer.clone(), Ok("pub fn main() { 42 }".to_owned()));
        for sources in [
            vec![],
            vec![application.clone()],
            vec![
                application.clone(),
                (
                    url("project/src/helper.ald"),
                    Ok("pub fn answer() { 42 }".to_owned()),
                ),
            ],
            vec![
                application,
                (
                    invalid.clone(),
                    Ok(indoc::indoc! {r#"
                enum Key { Value(Number) }
                impl Eq[Key] { fn eq(left: Key, right: Key) Bool { true } }
            "#}
                    .to_owned()),
                ),
            ],
        ] {
            let result = build_fixture_sync(
                sources,
                BuildMode::Build,
                BuildDependencies {
                    interfaces: vec![api.clone()],
                    package_instance_indexes: vec![index.clone()],
                    ..BuildDependencies::default()
                },
            );
            assert!(
                !result.is_success(),
                "even a build with no source modules must reject an invalid registry"
            );
            for (uri, module) in &result.modules {
                if uri == &invalid {
                    let ModuleResult::Failed { diagnostics } = module else {
                        panic!("local conflict must remain diagnosed")
                    };
                    assert_eq!(diagnostics.len(), 1);
                } else {
                    assert!(matches!(module, ModuleResult::Blocked), "{uri}: {module:?}");
                }
            }
            assert!(result.artifacts.is_empty());
            assert!(result.interfaces.is_empty());
            assert_eq!(
                result.diagnostics.len(),
                1,
                "dependency errors must not repeat per source"
            );
            let diagnostic = &result.diagnostics[0];
            assert_eq!(diagnostic.source().name(), "dependency trait registry");
            assert!(miette::Diagnostic::labels(diagnostic).is_none());
            if result.modules.is_empty() {
                assert_rendered_diagnostic_snapshot!(source, diagnostic.clone());
            }
        }
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
    fn package_coherence_does_not_blame_unrelated_sources() {
        let invalid = url("project/src/invalid.ald");
        let helper = url("project/src/helper.ald");
        let consumer = url("project/src/consumer.ald");
        let invalid_source = indoc::indoc! {r#"
            enum Key { Value(Number) }
            impl Eq[Key] {
                fn eq(left: Key, right: Key) Bool { true }
            }
        "#};
        let sources = vec![
            (invalid.clone(), Ok(invalid_source.to_owned())),
            (helper.clone(), Ok("pub fn answer() { 42 }".to_owned())),
            (
                consumer.clone(),
                Ok(indoc::indoc! {r#"
                import ~/helper.{ answer }
                pub fn main() Number { answer() }
            "#}
                .to_owned()),
            ),
        ];
        for reverse in [false, true] {
            let mut sources = sources.clone();
            if reverse {
                sources.reverse();
            }
            let result =
                build_fixture_sync(sources, BuildMode::Build, BuildDependencies::default());
            assert!(
                !result.is_success(),
                "package coherence must still reject the build"
            );
            let ModuleResult::Failed { diagnostics } = &result.modules[&invalid] else {
                panic!("the invalid implementation must retain its diagnostic");
            };
            assert_eq!(diagnostics.len(), 1);
            assert_eq!(
                diagnostics[0].message(),
                "overlapping implementations of `Eq` are not allowed"
            );
            for uri in [&helper, &consumer] {
                assert!(
                    matches!(result.modules[uri], ModuleResult::Blocked),
                    "unrelated source {uri} received: {:?}",
                    result.modules[uri]
                );
            }
            assert!(result.artifacts.is_empty());
            assert!(result.interfaces.is_empty());
        }
    }

    #[test]
    fn package_coherence_labels_only_local_implementations() {
        let model = url("project/src/model.ald");
        let first = url("project/src/first.ald");
        let second = url("project/src/second.ald");
        let source = indoc::indoc! {r#"
            import ~/model.{ Token, Display }
            impl Display[Token] {
                fn display(value: Token) String { "token" }
            }
        "#};
        let result = build_fixture_sync(
            vec![
                (
                    model,
                    Ok(indoc::indoc! {r#"
                pub enum Token { Token }
                pub trait Display[a] { fn display(value: a) String }
            "#}
                    .to_owned()),
                ),
                (first.clone(), Ok(source.to_owned())),
                (second.clone(), Ok(source.to_owned())),
            ],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        for (suffix, uri) in [("first", first), ("second", second)] {
            let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
                panic!("both defining modules must retain the overlap error");
            };
            assert_eq!(diagnostics.len(), 1);
            assert_eq!(
                miette::Diagnostic::labels(&diagnostics[0]).unwrap().count(),
                1,
                "a foreign implementation must not acquire a local source span"
            );
            insta::with_settings!({ snapshot_suffix => suffix }, {
                assert_rendered_diagnostic_snapshot!(source, diagnostics[0].clone());
            });
        }
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
    async fn renders_tuple_read_out_of_bounds_without_color() {
        assert_diagnostic_snapshot! {r#"
            fn read(pair: (Number, String)) { pair.2 }
        "#};
    }

    #[tokio::test]
    async fn renders_tuple_write_out_of_bounds_without_color() {
        assert_diagnostic_snapshot! {r#"
            fn write(pair: (Number, String)) {
                pair.2 = 42
            }
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

    #[test]
    fn generic_contracts_explain_specialization_and_independent_variables() {
        let source = indoc::indoc! {r#"
            fn specialize(value: a) a { 42 }
            fn collapse(first: a, second: b) a { second }
            fn nested(value: a, left: b, right: c) a { (left, right) }
            fn tuple(value: a) { value.0 }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("invalid generic contracts must fail")
        };
        assert_eq!(diagnostics.len(), 4, "{diagnostics:?}");
        assert!(
            miette::Diagnostic::help(&diagnostics[0])
                .unwrap()
                .to_string()
                .contains("the caller chooses")
        );
        assert!(
            miette::Diagnostic::help(&diagnostics[1])
                .unwrap()
                .to_string()
                .contains("independently chosen")
        );
        assert!(
            miette::Diagnostic::help(&diagnostics[2])
                .unwrap()
                .to_string()
                .contains("`(b, c)`")
        );
        assert!(
            miette::Diagnostic::help(&diagnostics[3])
                .unwrap()
                .to_string()
                .contains("tuple of length"),
            "{:?}",
            diagnostics[3]
        );
        for diagnostic in diagnostics {
            assert!(
                !miette::Diagnostic::help(diagnostic)
                    .unwrap()
                    .to_string()
                    .contains("give the declaration a concrete signature")
            );
        }
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
        let valid = indoc::indoc! {r#"
            fn keep(value: a) a { value }
            fn choose(first: a, second: b) a { first }
            fn pair(left: b, right: c) (b, c) { (left, right) }
            fn first(value: (a, b)) a { value.0 }
        "#};
        let valid = build_fixture_sync(
            vec![(uri.clone(), Ok(valid.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        assert!(matches!(valid.modules[&uri], ModuleResult::Success { .. }));
    }

    #[tokio::test]
    async fn renders_local_annotation_specializing_an_enclosing_generic_without_color() {
        assert_diagnostic_snapshot! {r#"
            fn keep(value: a) a {
                let ignored: a = 42
                value
            }
        "#};
    }

    #[test]
    fn generic_record_restrictions_explain_unknown_overwrites() {
        let source = indoc::indoc! {r#"
            fn merge(left, right) { { ..left, ..right } }
            fn invalid(left: { r | value: Number }, right: { s | marker: Bool }) Number {
                merge(left, right).value
            }
            fn invalid_rows(left: { r | value: Number }, right: { s | marker: Bool }) ({ r | value: Number, marker: Bool }) {
                merge(left, right)
            }
        "#};
        let uri = url("app/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
            panic!("an independent row may overwrite the required field")
        };
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert!(
            miette::Diagnostic::help(&diagnostics[0])
                .unwrap()
                .to_string()
                .contains("which spread operand supplies this field"),
            "{diagnostics:?}"
        );
        assert!(
            miette::Diagnostic::help(&diagnostics[1])
                .unwrap()
                .to_string()
                .contains("two independent rows are equal"),
            "{diagnostics:?}"
        );
        assert_rendered_diagnostics_snapshot!(source, diagnostics);
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
