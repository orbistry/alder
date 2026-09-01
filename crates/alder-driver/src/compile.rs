//! Module compilation orchestration.
//!
//! Each module runs Elm's full pipeline: parse -> canonicalize ->
//! constrain -> solve -> `Interface::from_module` with the solver's
//! annotations. Modules compile in dependency order, and each solved
//! module's interface is deep-copied into a build-wide arena so dependents
//! canonicalize their imports against it — interfaces only ever exist for
//! type-solved modules.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use alder_can::Interface;
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
}

/// Compile all modules through the full pipeline, in dependency order.
///
/// The async part only fetches sources; the CPU-bound compilation runs on
/// tokio's blocking pool (`spawn_blocking`) so no executor worker is ever
/// stalled.
pub async fn build(db: Arc<Mutex<Database>>, graph: &DepGraph) -> BuildResult {
    let modules: Vec<&Url> = graph.levels().into_iter().flatten().collect();
    let sources = fetch_sources(&db, &modules).await;

    tokio::task::spawn_blocking(move || build_sync(sources))
        .await
        .expect("compile task panicked")
}

/// Compile modules one at a time in dependency order, threading each
/// solved module's interface to its dependents through a build-wide arena.
///
/// Type checking is inherently dependency-ordered, so within-build
/// parallelism is limited to source fetching for now.
fn build_sync(sources: Vec<(Url, Result<String, String>)>) -> BuildResult {
    let store = Bump::new();
    let mut interfaces: BTreeMap<&str, Interface<'_>> = BTreeMap::new();

    let mut results: HashMap<Url, ModuleResult> = HashMap::new();
    let mut all_warnings: Vec<String> = Vec::new();

    for (uri, source) in &sources {
        let (output, interface) = compile_module(uri, source, &store, &interfaces);
        if let Some(interface) = interface {
            interfaces.insert(interface.home.name, interface);
        }
        all_warnings.extend(output.warnings);
        results.insert(output.uri, output.result);
    }

    let total = results.len();
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

    let mut results = Vec::with_capacity(uris.len());
    while let Some(res) = set.join_next().await {
        if let Ok(r) = res {
            results.push(r);
        }
    }
    results
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
    interfaces: &BTreeMap<&'s str, Interface<'s>>,
) -> (CompileOutput, Option<Interface<'s>>) {
    let failed = |message: String| {
        (
            CompileOutput {
                uri: uri.clone(),
                result: ModuleResult::Failed { message },
                warnings: vec![],
            },
            None,
        )
    };

    let source = match source {
        Ok(s) => s,
        Err(e) => return failed(e.clone()),
    };

    let bump = Bump::new();
    let src: &str = bump.alloc_str(source);
    let mut parser = alder_parse::Parser::new(&bump, src.as_bytes());

    let module = match parser.module() {
        Ok(module) => module,
        Err(e) => return failed(format!("{:?}", e)),
    };

    let context = alder_can::Context {
        package: None,
        interfaces: Some(interfaces),
    };
    let can_result = match alder_can::canonicalize(&bump, context, &module) {
        Ok(can_result) => can_result,
        Err(errors) => return failed(format!("{:?}", errors)),
    };
    let warnings: Vec<String> = can_result
        .warnings
        .iter()
        .map(|w| format!("{:?}", w))
        .collect();

    let mut uf = alder_constrain::UnionFind::new();
    let constraint = alder_constrain::constrain(&bump, &mut uf, &can_result.module);
    let annotations = match alder_solve::run(&bump, &mut uf, &constraint) {
        Ok(annotations) => annotations,
        Err(errors) => return failed(format!("{:?}", errors)),
    };

    let interface = alder_can::from_module(&bump, &can_result.module, &annotations);
    let stored = alder_can::deep_copy_interface(store, &interface);

    (
        CompileOutput {
            uri: uri.clone(),
            result: ModuleResult::Success {
                decl_count: count_decls(can_result.module.decls),
            },
            warnings,
        },
        Some(stored),
    )
}

fn count_decls(decls: &alder_ast::Decls<'_>) -> usize {
    match decls {
        alder_ast::Decls::Declare { next, .. } => 1 + count_decls(next),
        alder_ast::Decls::DeclareRec {
            following, next, ..
        } => 1 + following.len() + count_decls(next),
        alder_ast::Decls::Empty => 0,
    }
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
        for import in module.imports {
            let import_name = import.import.value;

            // Try to resolve import to a known module
            if let Some(uri) = resolve_import(import_name, current, known_modules) {
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
fn resolve_import(name: &str, _current: &Url, known_modules: &[Url]) -> Option<Url> {
    // Convert module name to file path pattern
    // e.g., "Json.Decode" -> "Json/Decode.alder"
    let path_pattern = format!("{}.alder", name.replace('.', "/"));

    // Find matching module
    known_modules
        .iter()
        .find(|uri| uri.path().ends_with(&path_pattern))
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
        let uri = url("Main.alder");
        mem.insert(
            uri.clone(),
            r#"
module Main exposing (..)

main = 42
"#
            .to_string(),
        );

        let db = Arc::new(Mutex::new(Database::new(mem)));
        let modules = vec![uri];
        let graph = build_graph(db.clone(), &modules).await.unwrap();
        let result = build(db, &graph).await;

        assert_eq!(result.total, 1);
        assert_eq!(result.success, 1);
        assert!(result.is_success());
    }

    #[tokio::test]
    async fn test_compile_invalid_module() {
        let mem = InMemorySource::new();
        let uri = url("Bad.alder");
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
            url("Utils.alder"),
            r#"
module Utils exposing (..)

helper = 1
"#
            .to_string(),
        );

        mem.insert(
            url("Main.alder"),
            r#"
module Main exposing (..)

import Utils

main = Utils.helper
"#
            .to_string(),
        );

        let db = Arc::new(Mutex::new(Database::new(mem)));
        let modules = vec![url("Utils.alder"), url("Main.alder")];

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
            url("Utils.alder"),
            r#"
module Utils exposing (..)

helper = 1
"#
            .to_string(),
        );

        mem.insert(
            url("Main.alder"),
            r#"
module Main exposing (..)

import Utils

main = Utils.helper "not a function argument"
"#
            .to_string(),
        );

        let db = Arc::new(Mutex::new(Database::new(mem)));
        let modules = vec![url("Utils.alder"), url("Main.alder")];

        let graph = build_graph(db.clone(), &modules).await.unwrap();
        let result = build(db, &graph).await;

        // Utils.helper is a number, not a function: Main gets a type
        // error against the imported annotation.
        assert_eq!(result.total, 2);
        assert_eq!(result.success, 1);
        assert_eq!(result.failed, 1);
        assert!(matches!(
            result.modules[&url("Main.alder")],
            ModuleResult::Failed { .. }
        ));
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
            url("Utils.alder"),
            r#"
module Utils exposing (..)

ping x = pong x

pong x = ping x
"#
            .to_string(),
        );

        mem.insert(
            url("Main.alder"),
            r#"
module Main exposing (..)

import Utils

main = Utils.pong 1
"#
            .to_string(),
        );

        let db = Arc::new(Mutex::new(Database::new(mem)));
        let modules = vec![url("Utils.alder"), url("Main.alder")];

        let graph = build_graph(db.clone(), &modules).await.unwrap();
        let result = build(db, &graph).await;

        assert_eq!(result.total, 2);
        assert_eq!(result.success, 2);
        assert!(result.is_success());
    }
}
