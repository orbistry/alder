//! Rolldown is isolated here because its Rust API intentionally has no semver
//! stability guarantee. Callers deal only in owned virtual modules and ESM.

use std::{
    borrow::Cow,
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use alder_codegen::EmittedModule;
use alder_codegen::support;
use oxc_ast::ast::Statement;
use rolldown::{Bundler, BundlerOptions, InputItem, OutputFormat};
use rolldown_common::ModuleType;
use rolldown_ecmascript::EcmaAst;
use rolldown_plugin::{
    HookLoadArgs, HookLoadOutput, HookResolveIdArgs, HookResolveIdOutput, HookTransformAstArgs,
    HookUsage, Plugin, PluginContext,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Standalone,
    Cloudflare,
    Test,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
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

    let application_modules = modules
        .iter()
        .map(|module| module.module_id.clone())
        .collect::<Vec<_>>();
    let mut sources = BTreeMap::new();
    sources.insert(
        "alder:kernel".to_owned(),
        alder_kernel::KERNEL_JS.to_owned(),
    );
    for (name, code) in builtin_modules() {
        sources.insert(format!("alder://std/{name}.mjs"), code);
    }
    let support_kind = match kind {
        EntryKind::Standalone => support::EntryKind::Standalone,
        EntryKind::Cloudflare => support::EntryKind::Cloudflare,
        EntryKind::Test => support::EntryKind::Test,
    };
    let generated_support = [support::entry_module(
        entry_module,
        support_kind,
        &application_modules,
    )];
    let asts = modules
        .into_iter()
        .chain(generated_support)
        .map(|module| (module.module_id, module.ast))
        .collect();
    let plugin = Arc::new(VirtualModules {
        asts: Mutex::new(asts),
        sources,
    });

    let mut bundler = Bundler::with_plugins(
        BundlerOptions {
            input: Some(vec![InputItem {
                name: Some("main".to_owned()),
                import: "alder:entry".to_owned(),
            }]),
            cwd: Some(std::env::current_dir()?),
            format: Some(OutputFormat::Esm),
            ..Default::default()
        },
        vec![plugin],
    )
    .map_err(|error| Error::Rolldown(error.to_string()))?;
    let output = bundler
        .generate()
        .await
        .map_err(|error| Error::Rolldown(error.to_string()))?;
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
            "Option",
            exports(&[
                ("$optionSome", "some"),
                ("$optionNone", "none"),
                ("$optionMap", "map"),
            ]),
        ),
        (
            "Result",
            exports(&[
                ("$resultOk", "ok"),
                ("$resultErr", "err"),
                ("$resultMap", "map"),
            ]),
        ),
        (
            "Array",
            exports(&[
                ("$arrayLength", "length"),
                ("$arrayPush", "push"),
                ("$arrayMap", "map"),
                ("$arrayFilter", "filter"),
            ]),
        ),
        (
            "String",
            exports(&[("$stringLength", "length"), ("$stringConcat", "concat")]),
        ),
        ("Number", exports(&[("$numberParse", "parse")])),
        ("BigInt", exports(&[("$bigIntParse", "parse")])),
        (
            "Map",
            exports(&[("$mapNew", "new"), ("$mapGet", "get"), ("$mapSet", "set")]),
        ),
        (
            "Set",
            exports(&[("$setNew", "new"), ("$setHas", "has"), ("$setAdd", "add")]),
        ),
        (
            "Json",
            exports(&[("$jsonEncode", "encode"), ("$jsonDecode", "decode")]),
        ),
        ("Ref", exports(&[("$refSame", "same")])),
        ("Io", exports(&[("$ioPrint", "print")])),
        ("Cli", exports(&[("$cliArgs", "args")])),
        ("Task", exports(&[("$taskSleep", "sleep")])),
        (
            "Fiber",
            exports(&[("$fiberAll", "all"), ("$fiberRace", "race")]),
        ),
        ("Http", String::new()),
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
struct VirtualModules {
    asts: Mutex<BTreeMap<String, EcmaAst>>,
    sources: BTreeMap<String, String>,
}

impl VirtualModules {
    fn contains(&self, id: &str) -> bool {
        self.sources.contains_key(id)
            || self
                .asts
                .lock()
                .expect("virtual AST map mutex poisoned")
                .contains_key(id)
    }
}

impl Plugin for VirtualModules {
    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("alder-virtual-modules")
    }

    fn resolve_id(
        &self,
        _ctx: &PluginContext,
        args: &HookResolveIdArgs<'_>,
    ) -> impl std::future::Future<Output = rolldown_plugin::HookResolveIdReturn> + Send {
        let resolved = self
            .contains(args.specifier)
            .then(|| HookResolveIdOutput::from_id(args.specifier));
        async move { Ok(resolved) }
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

    // Parse hand-written JavaScript used only as a bundler fixture. Production
    // Alder modules arrive from alder-codegen as already-built `EcmaAst`s.
    fn parsed_javascript_fixture(code: &str) -> EmittedModule {
        EmittedModule {
            module_id: "alder://app/main.mjs".to_owned(),
            ast: rolldown_ecmascript::EcmaCompiler::parse("fixture.mjs", code, Default::default())
                .unwrap(),
            dependencies: Vec::new(),
        }
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
        assert!(code.contains("await main"));
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
