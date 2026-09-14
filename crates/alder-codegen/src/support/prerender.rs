//! Build-time rendering entries, generated as Oxc ASTs like production entries.
use std::collections::BTreeMap;

use oxc_ast::ast::VariableDeclarationKind;

use crate::EmittedModule;

#[derive(Clone, Debug)]
pub struct Target {
    pub route: String,
    pub segments: Vec<(String, String)>,
    pub paths: Vec<String>,
    pub trailing_slash: String,
    pub entries: Option<(String, bool, String, String)>,
}

#[derive(Clone, Debug)]
pub struct Page {
    pub path: String,
    pub html: String,
    pub data: String,
}

pub fn entry(
    targets: &[Target],
    cloudflare: bool,
    adapters: &[super::cloudflare::Adapter],
) -> EmittedModule {
    super::generated_module("alder:web-prerender", |js| {
        let mut body = js.vec();
        body.push(js.namespace_import("alder:web-server", "$server"));
        if cloudflare && !adapters.is_empty() {
            body.push(js.namespace_import("alder:cloudflare-adapters", "$adapters"));
        }
        let aliases = targets
            .iter()
            .filter_map(|target| target.entries.as_ref())
            .flat_map(|(module, _, validators, _)| [module.clone(), validators.clone()])
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .enumerate()
            .map(|(index, name)| (name, format!("$entry{index}")))
            .collect::<BTreeMap<_, _>>();
        for (module, alias) in &aliases {
            body.push(js.namespace_import(module, alias));
        }
        let descriptors = js.array(targets.iter().map(|target| {
            let mut fields = js.vec();
            fields.push(js.property("route", js.string(&target.route)));
            fields.push(
                js.property(
                    "segments",
                    js.array(
                        target
                            .segments
                            .iter()
                            .map(|(kind, value)| js.array([js.string(kind), js.string(value)])),
                    ),
                ),
            );
            fields.push(js.property(
                "paths",
                js.array(target.paths.iter().map(|path| js.string(path))),
            ));
            fields.push(js.property("trailingSlash", js.string(&target.trailing_slash)));
            if let Some((module, callable, validators, name)) = &target.entries {
                let value = js.member(js.identifier(&aliases[module]), "entries");
                let value = if *callable { js.call(value, []) } else { value };
                fields.push(js.property(
                    "entries",
                    js.arrow(&[], js.builder.vec1(js.return_statement(value)), false),
                ));
                fields.push(js.property(
                    "validate",
                    js.member(js.identifier(&aliases[validators]), name),
                ));
            }
            js.object(fields)
        }));
        let runtime = if cloudflare {
            "$webPrerenderWorker"
        } else {
            "$webPrerender"
        };
        body.push(js.import("alder:kernel", &[(runtime.to_owned(), runtime.to_owned())]));
        let mut arguments = vec![js.member(js.identifier("$server"), "default"), descriptors];
        if cloudflare {
            arguments.push(
                if adapters
                    .iter()
                    .any(|adapter| adapter.kind == super::cloudflare::AdapterKind::Queue)
                {
                    js.member(js.identifier("$adapters"), "queue")
                } else {
                    js.undefined()
                },
            );
        }
        let call = js.call(js.identifier(runtime), arguments);
        if cloudflare {
            body.push(js.export_default(call));
            let classes = adapters
                .iter()
                .filter(|adapter| adapter.kind != super::cloudflare::AdapterKind::Queue)
                .collect::<Vec<_>>();
            if !classes.is_empty() {
                let mut exports = Vec::new();
                for (index, adapter) in classes.iter().enumerate() {
                    let local = format!("$class{index}");
                    body.push(js.variable(
                        VariableDeclarationKind::Const,
                        &local,
                        Some(js.member(js.identifier("$adapters"), &adapter.name)),
                    ));
                    exports.push((local, adapter.name.clone()));
                }
                body.push(js.export(&exports));
            }
        } else {
            let encoded = js.call(
                js.member(js.identifier("JSON"), "stringify"),
                [js.await_expression(call)],
            );
            body.push(js.expression_statement(js.call(
                js.member(js.identifier("__alderHost"), "reportBuild"),
                [encoded],
            )));
        }
        js.program(body)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support::cloudflare::{Adapter, AdapterKind};

    fn target() -> Target {
        Target {
            route: "/[id]".into(),
            segments: vec![("param".into(), "id".into())],
            paths: vec!["/one".into()],
            trailing_slash: "Always".into(),
            entries: None,
        }
    }

    #[test]
    fn standalone_render_reports_through_host_sink() {
        let code = entry(&[target()], false, &[]).code();
        assert!(
            code.contains("__alderHost[\"reportBuild\"](JSON[\"stringify\"](await $webPrerender("),
            "{code}"
        );
        assert!(code.contains("\"trailingSlash\": \"Always\""), "{code}");
        assert!(!code.contains("console"));
        assert!(!code.contains("export default"));
    }

    #[test]
    fn entries_values_and_functions_are_deferred_and_share_module_imports() {
        let mut value = target();
        value.entries = Some((
            "alder://entries.mjs".into(),
            false,
            "alder:validators".into(),
            "checkEntries".into(),
        ));
        let mut function = value.clone();
        function.entries.as_mut().unwrap().1 = true;
        let code = entry(&[value, function], false, &[]).code();
        assert_eq!(code.matches("from \"alder://entries.mjs\"").count(), 1);
        assert_eq!(code.matches("from \"alder:validators\"").count(), 1);
        assert!(code.contains("[\"entries\"];"), "{code}");
        assert!(code.contains("[\"entries\"]();"), "{code}");
        assert_eq!(code.matches("\"entries\": () =>").count(), 2, "{code}");
        assert_eq!(code.matches("[\"checkEntries\"]").count(), 2);
    }

    #[test]
    fn cloudflare_render_preserves_queue_and_class_adapters() {
        let adapters = [
            AdapterKind::Queue,
            AdapterKind::DurableObject,
            AdapterKind::Workflow,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, kind)| Adapter {
            kind,
            name: format!("Adapter{index}"),
            module: "alder://impl.mjs".into(),
            dictionary: format!("$dictionary{index}"),
        })
        .collect::<Vec<_>>();
        let code = entry(&[target()], true, &adapters).code();
        assert_eq!(
            code.matches("from \"alder:cloudflare-adapters\"").count(),
            1
        );
        assert!(code.contains("$adapters[\"queue\"]"), "{code}");
        assert!(code.contains("as Adapter1"));
        assert!(code.contains("as Adapter2"));
        assert!(!code.contains("as Adapter0"));
        assert!(!code.contains("reportBuild"));
    }
}
