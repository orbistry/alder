//! Module compilation orchestration.
//!
//! Each module runs Elm's full pipeline: parse -> canonicalize ->
//! constrain -> solve -> `Interface::from_module` with the solver's
//! annotations. Modules compile in dependency order, and each solved
//! module's interface is deep-copied into a build-wide arena so dependents
//! canonicalize their imports against it — interfaces only ever exist for
//! type-solved modules.

use std::collections::HashMap;
use std::sync::Arc;

use alder_ast::{
    Interface, ModuleId, PackageId, PackageName, ResolvedImport, ResolvedImportKind,
    ResolvedImportName, Visibility,
};
use bumpalo::Bump;
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use url::Url;

use crate::database::Database;
use crate::error::DriverError;
use crate::graph::DepGraph;

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
        /// Parse or other error message.
        message: String,
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
    pub warnings: Vec<String>,

    /// ESM modules produced in build or test mode, keyed by source URI.
    pub artifacts: HashMap<Url, alder_codegen::EmittedModule>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BuildMode {
    #[default]
    Check,
    Build,
    Test,
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
    warnings: Vec<String>,
    artifact: Option<alder_codegen::EmittedModule>,
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
    let modules: Vec<&Url> = graph.levels().into_iter().flatten().collect();
    let sources = fetch_sources(&db, &modules).await;

    tokio::task::spawn_blocking(move || build_sync(sources, mode))
        .await
        .expect("compile task panicked")
}

/// Compile modules in dependency order, retrying failures after new package
/// interfaces become available. The retry pass makes sibling-module instance
/// visibility independent of the arbitrary order within a graph level.
///
/// Type checking is inherently dependency-ordered, so within-build
/// parallelism is limited to source fetching for now.
fn build_sync(sources: Vec<(Url, Result<String, String>)>, mode: BuildMode) -> BuildResult {
    let store = Bump::new();
    let mut interfaces: Vec<Interface<'_>> = Vec::new();

    let mut results: HashMap<Url, ModuleResult> = HashMap::new();
    let mut all_warnings: Vec<String> = Vec::new();
    let mut artifacts = HashMap::new();
    let total = sources.len();
    let mut pending = (0..total).collect::<Vec<_>>();
    let mut failures = HashMap::new();

    loop {
        let mut next = Vec::new();
        let mut progress = false;
        for index in pending {
            let (uri, source) = &sources[index];
            let (output, interface) = compile_module(uri, source, &store, &interfaces, mode);
            if let Some(interface) = interface {
                progress = true;
                interfaces.push(interface);
                failures.remove(&output.uri);
                all_warnings.extend(output.warnings);
                if let Some(artifact) = output.artifact {
                    artifacts.insert(output.uri.clone(), artifact);
                }
                results.insert(output.uri, output.result);
            } else {
                failures.insert(output.uri, output.result);
                next.push(index);
            }
        }
        if next.is_empty() {
            break;
        }
        if !progress {
            results.extend(failures);
            break;
        }
        pending = next;
    }

    let success = results
        .values()
        .filter(|r| matches!(r, ModuleResult::Success { .. }))
        .count();

    BuildResult {
        modules: results,
        total,
        success,
        failed: total - success,
        warnings: all_warnings,
        artifacts,
    }
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
    store: &'s Bump,
    interfaces: &[Interface<'s>],
    mode: BuildMode,
) -> (CompileOutput, Option<Interface<'s>>) {
    let failed = |message: String| {
        (
            CompileOutput {
                uri: uri.clone(),
                result: ModuleResult::Failed { message },
                warnings: vec![],
                artifact: None,
            },
            None,
        )
    };

    let source = match source {
        Ok(s) => s,
        Err(e) => return failed(e.clone()),
    };

    let src: &'s str = store.alloc_str(source);
    let mut parser = alder_parse::Parser::new(store, src.as_bytes());

    let module = match parser.module() {
        Ok(module) => module,
        Err(e) => return failed(format!("{:?}", e)),
    };

    let home = module_id_from_uri(store, uri);
    let imports = resolve_imports(store, &module);
    let interfaces = store.alloc_slice_copy(interfaces);
    let context = alder_can::Context {
        home,
        imports,
        interfaces,
    };
    let can_result = match alder_can::canonicalize(store, context, &module) {
        Ok(can_result) => can_result,
        Err(errors) => return failed(format!("{:?}", errors)),
    };
    let warnings: Vec<String> = can_result
        .warnings
        .iter()
        .map(|w| format!("{:?}", w))
        .collect();

    let constraint = alder_constrain::constrain(store, can_result.module);
    let trait_database = alder_solve::TraitDatabase::build(store, can_result.module, interfaces);
    let solved = match alder_solve::solve(store, &constraint, &trait_database) {
        Ok(solved) => solved,
        Err(errors) => return failed(alder_solve::format_errors(&errors)),
    };

    let interface = alder_can::from_module(store, can_result.module, &solved.annotations);
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
                    return failed(format!(
                        "code generation failed at {:?}: {}",
                        error.region, error.message
                    ));
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
        Some(interface),
    )
}

fn module_id_from_uri<'a>(bump: &'a Bump, uri: &Url) -> ModuleId<'a> {
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
        package: PackageId::Application,
        path: bump.alloc_slice_fill_iter(
            segments
                .into_iter()
                .map(|part| bump.alloc_str(part) as &str),
        ),
    }
}

fn resolve_imports<'a>(
    bump: &'a Bump,
    module: &alder_source::Module<'a>,
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
                alder_source::ModuleRoot::Local(_) => (PackageId::Application, None),
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
mod tests {
    use super::*;
    use crate::source::InMemorySource;

    fn url(path: &str) -> Url {
        Url::parse(&format!("file:///{}", path)).unwrap()
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
        mem.insert(uri.clone(), "pub fn main() { 42 }".to_string());

        let db = Arc::new(Mutex::new(Database::new(mem)));
        let modules = vec![uri.clone()];
        let graph = build_graph(db.clone(), &modules).await.unwrap();
        let result = build_with_mode(db, &graph, BuildMode::Build).await;

        assert!(result.is_success());
        assert_eq!(result.artifacts.len(), 1);
        assert_eq!(result.artifacts[&uri].module_id, "alder://app/main.mjs");
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
            "trait Display[a] { fn display(value: a) -> String }\nfn main() { display(1) }"
                .to_string(),
        );

        let db = Arc::new(Mutex::new(Database::new(mem)));
        let graph = build_graph(db.clone(), std::slice::from_ref(&uri))
            .await
            .unwrap();
        let result = build(db, &graph).await;
        let ModuleResult::Failed { message } = &result.modules[&uri] else {
            panic!("missing trait evidence must fail compilation");
        };
        assert!(message.contains("no implementation of `Display[Number]` was found"));
        assert!(!message.contains("MissingInstance"));
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
                    Ok("pub enum Token { Token }\npub trait Display[a] { fn display(value: a) -> String }".to_owned()),
                ),
                (
                    consumer.clone(),
                    Ok("import ~/model.{ Token, display }\npub fn render(value: Token) -> String { display(value) }".to_owned()),
                ),
                (
                    implementation,
                    Ok("import ~/model.{ Token, Display }\nimpl Display[Token] { fn display(value: Token) -> String { \"token\" } }".to_owned()),
                ),
            ],
            BuildMode::Check,
        );

        assert!(result.is_success(), "{:?}", result.modules[&consumer]);
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
