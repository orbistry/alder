//! Rolldown is isolated here because its Rust API intentionally has no semver
//! stability guarantee. Callers deal only in owned virtual modules and ESM.

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

use alder_codegen::EmittedModule;
use alder_codegen::support;
use alder_report::{Diagnostic, Source};
use oxc_ast::ast::Statement;
use rolldown::{Bundler, BundlerOptions, InputItem, OutputFormat};
use rolldown_common::ModuleType;
use rolldown_ecmascript::EcmaAst;
use rolldown_plugin::{
    HookLoadArgs, HookLoadOutput, HookResolveIdArgs, HookResolveIdOutput, HookTransformAstArgs,
    HookUsage, Plugin, PluginContext, PluginContextResolveOptions,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Standalone,
    Cloudflare,
    Test,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Diagnostic(Box<Diagnostic>),
    #[error("entry module {0} was not emitted")]
    MissingEntry(String),
    #[error("bundle staging failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("rolldown failed: {0}")]
    Rolldown(String),
    #[error("rolldown did not emit a JavaScript chunk")]
    MissingChunk,
    #[error("Node compatibility imports are not supported: {0}")]
    NodeImport(String),
}

pub async fn bundle(
    modules: impl IntoIterator<Item = EmittedModule>,
    entry_module: &str,
    kind: EntryKind,
) -> Result<String, Error> {
    let modules: Vec<_> = modules.into_iter().collect();
    let application_modules = modules
        .iter()
        .map(|module| module.module_id.clone())
        .collect::<Vec<_>>();
    let support_kind = match kind {
        EntryKind::Standalone => support::EntryKind::Standalone,
        EntryKind::Cloudflare => support::EntryKind::Cloudflare,
        EntryKind::Test => support::EntryKind::Test,
    };
    if !modules
        .iter()
        .any(|module| module.module_id == entry_module)
    {
        return Err(Error::MissingEntry(entry_module.to_owned()));
    }
    let entry = support::entry_module(entry_module, support_kind, &application_modules);
    bundle_entry(modules.into_iter().chain([entry]), "alder:entry").await
}

/// Bundle an already-generated entry AST, without imposing a main/fetch ABI.
/// Web applications use distinct generated server and browser entries.
pub async fn bundle_entry(
    modules: impl IntoIterator<Item = EmittedModule>,
    entry_module: &str,
) -> Result<String, Error> {
    let mut modules: Vec<_> = modules.into_iter().collect();
    modules.sort_by(|left, right| left.module_id.cmp(&right.module_id));
    if !modules
        .iter()
        .any(|module| module.module_id == entry_module)
    {
        return Err(Error::MissingEntry(entry_module.to_owned()));
    }
    for module in &modules {
        if let Some(specifier) = node_import(&module.ast) {
            return Err(Error::NodeImport(specifier.to_owned()));
        }
    }

    let mut sources = BTreeMap::new();
    sources.insert(
        "alder:kernel".to_owned(),
        alder_kernel::KERNEL_JS.to_owned(),
    );
    for (name, code) in builtin_modules() {
        sources.insert(format!("alder://std/{name}.mjs"), code);
    }
    let origins = modules
        .iter()
        .filter_map(|module| {
            module.source_path.as_ref().map(|path| {
                (
                    module.module_id.clone(),
                    ModuleOrigin {
                        path: path.clone(),
                        source: Source::new(
                            path.to_string_lossy(),
                            module.source_text.clone().unwrap_or_default(),
                        ),
                        extern_regions: module.extern_regions.clone(),
                    },
                )
            })
        })
        .collect();
    let asts: BTreeMap<_, _> = modules
        .into_iter()
        .map(|module| (module.module_id, module.ast))
        .collect();
    let resolution_errors = Arc::new(Mutex::new(Vec::new()));
    let plugin = Arc::new(VirtualModules {
        resolution_errors: resolution_errors.clone(),
        ids: asts.keys().cloned().collect(),
        origins,
        asts: Mutex::new(asts),
        sources,
    });

    let mut bundler = Bundler::with_plugins(
        BundlerOptions {
            input: Some(vec![InputItem {
                name: Some("main".to_owned()),
                import: entry_module.to_owned(),
            }]),
            cwd: Some(std::env::current_dir()?),
            format: Some(OutputFormat::Esm),
            ..Default::default()
        },
        vec![plugin],
    )
    .map_err(|error| Error::Rolldown(error.to_string()))?;
    let output = bundler.generate().await.map_err(|error| {
        let mut diagnostics = resolution_errors
            .lock()
            .expect("resolution error mutex poisoned");
        diagnostics.sort_by(|left, right| {
            left.source()
                .name()
                .cmp(right.source().name())
                .then_with(|| left.message().cmp(right.message()))
        });
        if diagnostics.is_empty() {
            return Error::Rolldown(error.to_string());
        }
        let primary = diagnostics.remove(0);
        Error::Diagnostic(Box::new(
            diagnostics
                .drain(..)
                .fold(primary, Diagnostic::with_related),
        ))
    })?;
    output
        .assets
        .into_iter()
        .find_map(|asset| match asset {
            rolldown_common::Output::Chunk(chunk) => Some(chunk.code.to_string()),
            rolldown_common::Output::Asset(_) => None,
        })
        .ok_or(Error::MissingChunk)
}

fn node_import(ast: &EcmaAst) -> Option<&str> {
    ast.program()
        .body
        .iter()
        .find_map(|statement| match statement {
            Statement::ImportDeclaration(import) if import.source.value.starts_with("node:") => {
                Some(import.source.value.as_str())
            }
            Statement::ExportAllDeclaration(export) if export.source.value.starts_with("node:") => {
                Some(export.source.value.as_str())
            }
            Statement::ExportNamedDeclaration(export) => export
                .source
                .as_ref()
                .filter(|source| source.value.starts_with("node:"))
                .map(|source| source.value.as_str()),
            _ => None,
        })
}

fn builtin_modules() -> BTreeMap<&'static str, String> {
    BTreeMap::from([
        (
            "html",
            exports(&[
                ("$webSsr", "renderToString"),
                ("$webSsrAsyncString", "renderToStringAsync"),
                ("$webResourceDeclaration", "resource"),
                ("$webResourceRefresh", "refresh"),
                ("$webResourceCancel", "cancel"),
                ("$webPreventDefault", "preventDefault"),
                ("$webFormValues", "formValues"),
                ("$webSubmit", "submit"),
            ]),
        ),
        (
            "cloudflare",
            exports(&[
                ("$cloudflareKvGet", "kvGet"),
                ("$cloudflareKvPut", "kvPut"),
                ("$cloudflareStorageGet", "storageGet"),
                ("$cloudflareStoragePut", "storagePut"),
                ("$cloudflareStep", "step"),
            ]),
        ),
        (
            "http",
            exports(&[
                ("$httpText", "text"),
                ("$httpJson", "json"),
                ("$httpEmpty", "empty"),
                ("$httpRedirect", "redirect"),
                ("$httpStatus", "status"),
                ("$httpResponseHeader", "responseHeader"),
                ("$httpWithHeader", "withHeader"),
                ("$httpRequest", "request"),
                ("$httpMethod", "method"),
                ("$httpRequestUrl", "requestUrl"),
                ("$httpRequestHeader", "requestHeader"),
                ("$httpPathname", "pathname"),
                ("$httpSearchParam", "searchParam"),
                ("$httpReadText", "readText"),
                ("$httpReadForm", "readForm"),
                ("$httpReadJson", "readJson"),
                ("$httpResponseText", "responseText"),
                ("$httpFetch", "fetch"),
            ]),
        ),
        (
            "option",
            exports(&[
                ("$optionSome", "some"),
                ("$optionNone", "none"),
                ("$optionMap", "map"),
            ]),
        ),
        (
            "result",
            exports(&[
                ("$resultOk", "ok"),
                ("$resultErr", "err"),
                ("$resultMap", "map"),
            ]),
        ),
        (
            "array",
            exports(&[
                ("$arrayLength", "length"),
                ("$arrayIter", "iter"),
                ("$arrayPush", "push"),
                ("$arrayMap", "map"),
                ("$arrayFilter", "filter"),
            ]),
        ),
        (
            "string",
            exports(&[("$stringLength", "length"), ("$stringConcat", "concat")]),
        ),
        ("number", exports(&[("$numberParse", "parse")])),
        ("bigint", exports(&[("$bigIntParse", "parse")])),
        (
            "map",
            exports(&[("$mapNew", "new"), ("$mapGet", "get"), ("$mapSet", "set")]),
        ),
        (
            "set",
            exports(&[("$setNew", "new"), ("$setHas", "has"), ("$setAdd", "add")]),
        ),
        (
            "json",
            exports(&[("$jsonEncodeWith", "encode"), ("$jsonDecodeWith", "decode")]),
        ),
        (
            "ref",
            exports(&[
                ("$refSame", "same"),
                ("$refMake", "make"),
                ("$refGet", "get"),
                ("$refSet", "set"),
                ("$refUpdate", "update"),
                ("$refModify", "modify"),
            ]),
        ),
        ("io", exports(&[("$ioPrint", "print")])),
        ("cli", exports(&[("$cliArgs", "args")])),
        ("task", exports(&[("$taskSleep", "sleep")])),
        (
            "synchronized_ref",
            exports(&[
                ("$synchronizedRefMake", "make"),
                ("$synchronizedRefGet", "get"),
                ("$synchronizedRefSet", "set"),
                ("$synchronizedRefUpdate", "update"),
                ("$synchronizedRefModify", "modify"),
            ]),
        ),
        (
            "semaphore",
            exports(&[
                ("$semaphoreMake", "make"),
                ("$semaphoreWithPermits", "withPermits"),
            ]),
        ),
        (
            "fiber",
            exports(&[
                ("$fiberUnbounded", "unbounded"),
                ("$fiberMapWithOptions", "map"),
                ("$fiberForEachWithOptions", "forEach"),
                ("$fiberTryMapWithOptions", "tryMap"),
                ("$fiberTryForEachWithOptions", "tryForEach"),
                ("$fiberFork", "fork"),
                ("$fiberJoin", "join"),
                ("$fiberInterrupt", "interrupt"),
                ("$fiberAll", "all"),
                ("$fiberRace", "race"),
                ("$fiberScope", "scope"),
                ("$fiberAddFinalizer", "addFinalizer"),
                ("$fiberUninterruptible", "uninterruptible"),
            ]),
        ),
    ])
}

fn exports(names: &[(&str, &str)]) -> String {
    let imports = names
        .iter()
        .map(|(source, _)| *source)
        .collect::<Vec<_>>()
        .join(", ");
    let exports = names
        .iter()
        .map(|(source, target)| format!("{source} as {target}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("import {{ {imports} }} from \"alder:kernel\";\nexport {{ {exports} }};\n")
}

#[derive(Debug)]
struct ModuleOrigin {
    path: std::path::PathBuf,
    source: Source,
    extern_regions: Vec<(String, alder_region::Region)>,
}

impl ModuleOrigin {
    fn resolution_failure(&self, specifier: &str, reason: &str) -> Diagnostic {
        let mut diagnostic = Diagnostic::error(
            self.source.clone(),
            format!("cannot resolve extern module `{specifier}`"),
        )
        .with_code("alder::bundle::extern_resolution")
        .with_help(format!(
            "{reason}. Relative extern paths are resolved beside {}. Check the wrapper's path.",
            self.path.display()
        ));
        for (_, region) in self
            .extern_regions
            .iter()
            .filter(|(module, _)| module == specifier)
        {
            diagnostic = diagnostic.with_primary_label(*region, "this extern requires the module");
        }
        diagnostic
    }
}

#[derive(Debug)]
struct VirtualModules {
    asts: Mutex<BTreeMap<String, EcmaAst>>,
    ids: BTreeSet<String>,
    origins: BTreeMap<String, ModuleOrigin>,
    resolution_errors: Arc<Mutex<Vec<Diagnostic>>>,
    sources: BTreeMap<String, String>,
}

impl VirtualModules {
    fn contains(&self, id: &str) -> bool {
        self.sources.contains_key(id) || self.ids.contains(id)
    }
}

impl Plugin for VirtualModules {
    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("alder-virtual-modules")
    }

    async fn resolve_id(
        &self,
        ctx: &PluginContext,
        args: &HookResolveIdArgs<'_>,
    ) -> rolldown_plugin::HookResolveIdReturn {
        if self.contains(args.specifier) {
            return Ok(Some(HookResolveIdOutput::from_id(args.specifier)));
        }
        if args.specifier == "cloudflare:workers" {
            return Ok(Some(HookResolveIdOutput {
                id: args.specifier.into(),
                external: Some(rolldown_common::ResolvedExternal::Bool(true)),
                ..Default::default()
            }));
        }
        if args.specifier.starts_with("node:") {
            return Err(std::io::Error::other(format!(
                "Node compatibility imports are not supported: {}",
                args.specifier
            ))
            .into());
        }
        let Some(origin) = args.importer.and_then(|id| self.origins.get(id)) else {
            return Ok(None);
        };
        let physical_path = origin
            .path
            .to_str()
            .ok_or_else(|| std::io::Error::other("extern importer path is not valid UTF-8"))?;
        let resolved = ctx
            .resolve(
                args.specifier,
                Some(physical_path),
                Some(PluginContextResolveOptions {
                    import_kind: args.kind,
                    is_entry: args.is_entry,
                    custom: args.custom.clone(),
                    ..Default::default()
                }),
            )
            .await?
            .map_err(|error| {
                let diagnostic = origin.resolution_failure(args.specifier, &error.to_string());
                self.resolution_errors
                    .lock()
                    .expect("resolution error mutex poisoned")
                    .push(diagnostic);
                std::io::Error::other(format!(
                    "cannot resolve extern module {:?} from {}: {error}",
                    args.specifier, physical_path
                ))
            })?;
        Ok(Some(HookResolveIdOutput::from_resolved_id(resolved)))
    }

    fn load(
        &self,
        _ctx: &PluginContext,
        args: &HookLoadArgs<'_>,
    ) -> impl std::future::Future<Output = rolldown_plugin::HookLoadReturn> + Send {
        let output = self.sources.get(args.id).map_or_else(
            || {
                self.contains(args.id).then(|| HookLoadOutput {
                    code: "".into(),
                    module_type: Some(ModuleType::Js),
                    ..Default::default()
                })
            },
            |source| {
                Some(HookLoadOutput {
                    code: source.clone().into(),
                    module_type: Some(ModuleType::Js),
                    ..Default::default()
                })
            },
        );
        async move { Ok(output) }
    }

    fn transform_ast(
        &self,
        _ctx: &PluginContext,
        args: HookTransformAstArgs<'_>,
    ) -> impl std::future::Future<Output = rolldown_plugin::HookTransformAstReturn> + Send {
        let replacement = self
            .asts
            .lock()
            .expect("virtual AST map mutex poisoned")
            .remove(args.id);
        async move { Ok(replacement.unwrap_or(args.ast)) }
    }

    fn register_hook_usage(&self) -> HookUsage {
        HookUsage::ResolveId | HookUsage::Load | HookUsage::TransformAst
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_extern_module_labels_its_alder_declaration() {
        let source = indoc::indoc! {r#"
            #[extern("./client.js", "answer")]
            pub fn answer() Task[Number]
        "#};
        let origin = ModuleOrigin {
            path: "/project/src/api.ald".into(),
            source: Source::new("/project/src/api.ald", source),
            extern_regions: vec![(
                "./client.js".to_owned(),
                alder_region::Region::new(
                    alder_region::Position::new(1, 1),
                    alder_region::Position::new(2, 28),
                ),
            )],
        };
        let diagnostic =
            origin.resolution_failure("./client.js", "Cannot find module './client.js'");
        let mut rendered = String::new();
        miette::GraphicalReportHandler::new_themed(miette::GraphicalTheme::unicode_nocolor())
            .with_width(80)
            .render_report(&mut rendered, &diagnostic)
            .unwrap();
        insta::with_settings!({ description => source, omit_expression => true }, {
            insta::assert_snapshot!(rendered);
        });
    }

    // Parse hand-written JavaScript used only as a bundler fixture. Production
    // Alder modules arrive from alder-codegen as already-built `EcmaAst`s.
    fn parsed_javascript_fixture(code: &str) -> EmittedModule {
        EmittedModule {
            source_path: None,
            source_text: None,
            extern_regions: vec![],
            module_id: "alder://app/main.mjs".to_owned(),
            ast: rolldown_ecmascript::EcmaCompiler::parse("fixture.mjs", code, Default::default())
                .unwrap(),
            dependencies: Vec::new(),
            store_keys: Vec::new(),
        }
    }

    #[tokio::test]
    async fn every_builtin_module_links_all_of_its_exports() {
        // This is a hand-written JS bundler fixture, not Alder code generation.
        // Returning namespace objects keeps every export reachable by the host.
        let names = builtin_modules().into_keys().collect::<Vec<_>>();
        let imports = names
            .iter()
            .map(|name| format!("import * as {name} from 'alder://std/{name}.mjs';"))
            .collect::<Vec<_>>()
            .join("\n");
        let source = format!(
            "{imports}\nexport function main() {{ return [{}]; }}",
            names.join(", ")
        );
        bundle(
            [parsed_javascript_fixture(&source)],
            "alder://app/main.mjs",
            EntryKind::Standalone,
        )
        .await
        .expect("every builtin facade export must resolve against the embedded kernel");
    }

    #[tokio::test]
    async fn bundles_a_standalone_virtual_module() {
        let code = bundle(
            [parsed_javascript_fixture(
                "function main() { return 42; }\nexport { main };\n",
            )],
            "alder://app/main.mjs",
            EntryKind::Standalone,
        )
        .await
        .unwrap();
        assert!(code.contains("return 42"));
        assert!(code.contains("await $runMain(main())"), "{code}");
    }

    #[tokio::test]
    async fn rejects_node_compatibility_imports() {
        let error = bundle(
            [parsed_javascript_fixture(
                "import { readFile } from \"node:fs\"; export function main() {}",
            )],
            "alder://app/main.mjs",
            EntryKind::Standalone,
        )
        .await
        .unwrap_err();
        assert!(matches!(error, Error::NodeImport(ref name) if name == "node:fs"));
    }
}
