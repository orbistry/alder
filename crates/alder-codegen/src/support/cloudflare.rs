//! Cloudflare host adapters. Every dictionary symbol comes from a solved module
//! interface, retaining its real method and superclass codec ABI.
use oxc_ast::ast::VariableDeclarationKind;

use crate::EmittedModule;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdapterKind {
    DurableObject,
    Queue,
    Workflow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Adapter {
    pub kind: AdapterKind,
    /// Exported class name, or the exact queue name for a queue consumer.
    pub name: String,
    pub module: String,
    pub dictionary: String,
}

/// (qualified Alder context type key, exact Workers environment binding name).
pub type ProviderBinding = (String, String);

pub fn adapters(adapters: &[Adapter], providers: &[ProviderBinding]) -> EmittedModule {
    super::generated_module("alder:cloudflare-adapters", |js| {
        let mut body = js.vec();
        let mut exports = Vec::new();
        let mut native = Vec::new();
        if adapters
            .iter()
            .any(|adapter| adapter.kind == AdapterKind::DurableObject)
        {
            native.push(("DurableObject".to_owned(), "$DurableObject".to_owned()));
        }
        if adapters
            .iter()
            .any(|adapter| adapter.kind == AdapterKind::Workflow)
        {
            native.push((
                "WorkflowEntrypoint".to_owned(),
                "$WorkflowEntrypoint".to_owned(),
            ));
        }
        if !native.is_empty() {
            body.push(js.import("cloudflare:workers", &native));
        }
        body.push(js.import(
            "alder:kernel",
            &[
                (
                    "$cloudflareDurableClass".to_owned(),
                    "$cloudflareDurableClass".to_owned(),
                ),
                (
                    "$cloudflareWorkflowClass".to_owned(),
                    "$cloudflareWorkflowClass".to_owned(),
                ),
                (
                    "$cloudflareQueueHandler".to_owned(),
                    "$cloudflareQueueHandler".to_owned(),
                ),
            ],
        ));
        body.push(
            js.variable(
                VariableDeclarationKind::Const,
                "$providers",
                Some(
                    js.array(
                        providers
                            .iter()
                            .map(|(key, binding)| js.array([js.string(key), js.string(binding)])),
                    ),
                ),
            ),
        );
        exports.push(("$providers".to_owned(), "providers".to_owned()));
        let mut queues = Vec::new();
        for (index, adapter) in adapters.iter().enumerate() {
            let dictionary = format!("$dictionary{index}");
            body.push(js.import(
                &adapter.module,
                &[(adapter.dictionary.clone(), dictionary.clone())],
            ));
            let (factory, base) = match adapter.kind {
                AdapterKind::DurableObject => ("$cloudflareDurableClass", "$DurableObject"),
                AdapterKind::Workflow => ("$cloudflareWorkflowClass", "$WorkflowEntrypoint"),
                AdapterKind::Queue => {
                    queues.push(js.array([js.string(&adapter.name), js.identifier(&dictionary)]));
                    continue;
                }
            };
            let local = format!("$class{index}");
            body.push(js.variable(
                VariableDeclarationKind::Const,
                &local,
                Some(js.call(
                    js.identifier(factory),
                    [
                        js.identifier(base),
                        js.identifier(&dictionary),
                        js.identifier("$providers"),
                    ],
                )),
            ));
            exports.push((local, adapter.name.clone()));
        }
        if !queues.is_empty() {
            body.push(js.variable(
                VariableDeclarationKind::Const,
                "$queue",
                Some(js.call(
                    js.identifier("$cloudflareQueueHandler"),
                    [js.array(queues), js.identifier("$providers")],
                )),
            ));
            exports.push(("$queue".to_owned(), "queue".to_owned()));
        }
        body.push(js.export(&exports));
        js.program(body)
    })
}
