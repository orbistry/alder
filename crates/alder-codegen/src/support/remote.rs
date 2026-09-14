//! Browser replacements for solved remote modules, emitted directly as Oxc AST.

use crate::EmittedModule;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Query,
    Command,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Command => "command",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    pub arity: usize,
    pub kind: Kind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WireSchema {
    pub root: u32,
    pub nodes: Vec<WireNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WireVariant {
    pub tag: String,
    pub fields: Vec<(String, u32)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WireNode {
    Unit,
    Bool,
    Number,
    BigInt,
    String,
    Date,
    Array(u32),
    Tuple(Vec<u32>),
    Record(Vec<(String, u32)>),
    Option(u32),
    Map(u32, u32),
    Set(u32),
    Enum(Vec<WireVariant>),
    ErrorRow {
        variants: Vec<WireVariant>,
        open: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Validation {
    pub name: String,
    pub arguments: WireSchema,
    pub result: WireSchema,
}

/// Each function has separate input and completed-result validators. The host
/// distinguishes invalid request (400) from invalid server result (500).
pub fn validators(module_id: &str, validations: &[Validation]) -> EmittedModule {
    super::generated_module(module_id, |js| {
        let mut body = js.vec();
        body.push(js.import(
            "alder:kernel",
            &[("$webValidate".into(), "$webValidate".into())],
        ));
        let mut exports = Vec::new();
        for (index, validation) in validations.iter().enumerate() {
            for (suffix, schema) in [
                ("Args", &validation.arguments),
                ("Result", &validation.result),
            ] {
                let schema_name = format!("$schema{index}{suffix}");
                body.push(js.variable(
                    oxc_ast::ast::VariableDeclarationKind::Const,
                    &schema_name,
                    Some(schema_expression(js, schema)),
                ));
                let name = format!("{}{suffix}", validation.name);
                let call = js.call(
                    js.identifier("$webValidate"),
                    [js.identifier("value"), js.identifier(&schema_name)],
                );
                body.push(js.function(
                    &name,
                    &["value".into()],
                    js.builder.vec1(js.return_statement(call)),
                    false,
                ));
                exports.push((name.clone(), name));
            }
        }
        body.push(js.export(&exports));
        js.program(body)
    })
}

fn schema_expression<'a>(
    js: &crate::js_ast::JsAst<'a>,
    schema: &WireSchema,
) -> oxc_ast::ast::Expression<'a> {
    let fields = |fields: &[(String, u32)]| {
        js.array(
            fields
                .iter()
                .map(|(name, node)| js.array([js.string(name), js.number(f64::from(*node))])),
        )
    };
    let variants = |variants: &[WireVariant]| {
        js.array(
            variants
                .iter()
                .map(|variant| js.array([js.string(&variant.tag), fields(&variant.fields)])),
        )
    };
    let nodes = js.array(schema.nodes.iter().map(|node| match node {
        WireNode::Unit => js.array([js.string("unit")]),
        WireNode::Bool => js.array([js.string("bool")]),
        WireNode::Number => js.array([js.string("number")]),
        WireNode::BigInt => js.array([js.string("bigint")]),
        WireNode::String => js.array([js.string("string")]),
        WireNode::Date => js.array([js.string("date")]),
        WireNode::Array(item) => js.array([js.string("array"), js.number(f64::from(*item))]),
        WireNode::Option(item) => js.array([js.string("option"), js.number(f64::from(*item))]),
        WireNode::Set(item) => js.array([js.string("set"), js.number(f64::from(*item))]),
        WireNode::Map(key, value) => js.array([
            js.string("map"),
            js.number(f64::from(*key)),
            js.number(f64::from(*value)),
        ]),
        WireNode::Tuple(items) => js.array([
            js.string("tuple"),
            js.array(items.iter().map(|item| js.number(f64::from(*item)))),
        ]),
        WireNode::Record(items) => js.array([js.string("record"), fields(items)]),
        WireNode::Enum(items) => js.array([js.string("enum"), variants(items)]),
        WireNode::ErrorRow {
            variants: items,
            open,
        } => js.array([js.string("error"), variants(items), js.boolean(*open)]),
    }));
    js.object(js.builder.vec_from_iter([
        js.property("root", js.number(f64::from(schema.root))),
        js.property("nodes", nodes),
    ]))
}

pub fn module_id(module: alder_ast::ModuleId<'_>) -> String {
    crate::module_specifier(module)
}

/// `$webRemote` returns an Alder Task. The compiler already publishes the
/// remote function as Task-returning for both server and browser callers.
pub fn client_stub(module_id: &str, functions: &[Function]) -> EmittedModule {
    super::generated_module(module_id, |js| {
        let mut body = js.vec();
        body.push(js.import(
            "alder:kernel",
            &[("$webRemote".into(), "$webRemote".into())],
        ));
        let mut exports = Vec::new();
        for (index, function) in functions.iter().enumerate() {
            let local = format!("$remote{index}");
            let parameters = (0..function.arity)
                .map(|index| format!("$arg{index}"))
                .collect::<Vec<_>>();
            let call = js.call(
                js.identifier("$webRemote"),
                [
                    js.string(module_id),
                    js.string(&function.name),
                    js.array(parameters.iter().map(|name| js.identifier(name))),
                    js.string(function.kind.as_str()),
                ],
            );
            body.push(js.function(
                &local,
                &parameters,
                js.builder.vec1(js.return_statement(call)),
                false,
            ));
            exports.push((local, function.name.clone()));
        }
        body.push(js.export(&exports));
        js.program(body)
    })
}

/// Public server identity stays stable while implementations live behind a
/// private ID. Instrumentation happens only when the lazy Task is executed.
pub fn server_stub(
    module_id: &str,
    implementation_id: &str,
    functions: &[Function],
) -> EmittedModule {
    let mut module = super::generated_module(module_id, |js| {
        let mut body = js.vec();
        body.push(js.import(
            "alder:kernel",
            &[("$webRemoteServer".into(), "$webRemoteServer".into())],
        ));
        body.push(
            js.import(
                implementation_id,
                &functions
                    .iter()
                    .enumerate()
                    .map(|(index, function)| {
                        (function.name.clone(), format!("$implementation{index}"))
                    })
                    .collect::<Vec<_>>(),
            ),
        );
        body.push(js.export_all(implementation_id));
        let mut exports = Vec::new();
        for (index, function) in functions.iter().enumerate() {
            let local = format!("$remote{index}");
            let parameters = (0..function.arity)
                .map(|index| format!("$arg{index}"))
                .collect::<Vec<_>>();
            let call = js.call(
                js.identifier(&format!("$implementation{index}")),
                parameters.iter().map(|name| js.identifier(name)),
            );
            let invoke = js.arrow(&[], js.builder.vec1(js.return_statement(call)), false);
            let task = js.call(
                js.identifier("$webRemoteServer"),
                [
                    js.string(module_id),
                    js.string(function.kind.as_str()),
                    invoke,
                ],
            );
            body.push(js.function(
                &local,
                &parameters,
                js.builder.vec1(js.return_statement(task)),
                false,
            ));
            exports.push((local, function.name.clone()));
        }
        body.push(js.export(&exports));
        js.program(body)
    });
    module.dependencies.push(implementation_id.to_owned());
    module
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_facade_defers_implementation_and_preserves_auxiliary_exports() {
        let module = server_stub(
            "alder://app/remote.mjs",
            "alder://app/remote.mjs.__server",
            &[Function {
                name: "lookup".into(),
                arity: 1,
                kind: Kind::Query,
            }],
        );
        let code = module.code();
        assert!(code.contains("$webRemoteServer"));
        assert!(code.contains("() =>"));
        assert!(code.contains("export * from"));
        assert!(code.contains("$remote0 as lookup"));
        assert_eq!(module.dependencies, ["alder://app/remote.mjs.__server"]);
    }

    #[test]
    fn remote_replacement_exports_only_http_task_stubs() {
        let module = client_stub(
            "alder://app/users.remote.mjs",
            &[
                Function {
                    name: "getUser".into(),
                    arity: 1,
                    kind: Kind::Query,
                },
                Function {
                    name: "deleteUser".into(),
                    arity: 2,
                    kind: Kind::Command,
                },
            ],
        );
        let code = module.code();
        assert!(code.contains("$webRemote"));
        assert!(code.contains("getUser"));
        assert!(code.contains("deleteUser"));
        assert!(code.contains("\"query\""));
        assert!(code.contains("\"command\""));
        assert_eq!(module.module_id, "alder://app/users.remote.mjs");
        assert!(module.dependencies.is_empty());
    }
}
