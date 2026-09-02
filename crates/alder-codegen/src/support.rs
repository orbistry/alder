//! Generated runtime-facing modules built directly as Oxc ASTs.

use oxc_ast::ast::{Program, Statement, VariableDeclarationKind};
use oxc_span::SourceType;
use oxc_syntax::operator::{BinaryOperator, LogicalOperator};
use rolldown_ecmascript::EcmaAst;

use crate::{EmittedModule, js_ast::JsAst};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Standalone,
    Cloudflare,
    Test,
}

pub fn entry_module(
    application_entry: &str,
    kind: EntryKind,
    application_modules: &[String],
) -> EmittedModule {
    generated_module("alder:entry", |js| {
        let mut body = js.vec();
        match kind {
            EntryKind::Standalone => {
                body.push(js.import(application_entry, &[("main".to_owned(), "main".to_owned())]));
                let result = js.await_expression(js.call(js.identifier("main"), []));
                body.push(js.variable(VariableDeclarationKind::Const, "$result", Some(result)));
                let is_present = js.identifier("$result");
                let is_error = js.binary(
                    js.member(js.identifier("$result"), "$"),
                    BinaryOperator::StrictEquality,
                    js.string("Err"),
                );
                let condition = js.logical(is_present, LogicalOperator::And, is_error);
                let mut error = js.vec();
                error.push(js.expression_statement(js.call(
                    js.member(js.identifier("console"), "error"),
                    [js.member(js.identifier("$result"), "_0")],
                )));
                error.push(exit_statement(js, js.number(1.0)));
                let mut success = js.vec();
                success.push(exit_statement(js, js.number(0.0)));
                body.push(js.if_statement(condition, error, Some(js.block(success))));
                body.push(js.export_default(js.identifier("$result")));
            }
            EntryKind::Cloudflare => {
                body.push(js.import(
                    application_entry,
                    &[("fetch".to_owned(), "$fetch".to_owned())],
                ));
                let properties = js
                    .builder
                    .vec1(js.property("fetch", js.identifier("$fetch")));
                body.push(js.export_default(js.object(properties)));
            }
            EntryKind::Test => {
                body.push(js.import(
                    "alder:kernel",
                    &[("$runTests".to_owned(), "$runTests".to_owned())],
                ));
                for module in application_modules {
                    body.push(js.side_effect_import(module));
                }
                let failed = js.await_expression(js.call(js.identifier("$runTests"), []));
                body.push(js.variable(VariableDeclarationKind::Const, "$failed", Some(failed)));
                let status = js.conditional(
                    js.binary(
                        js.identifier("$failed"),
                        BinaryOperator::StrictEquality,
                        js.number(0.0),
                    ),
                    js.number(0.0),
                    js.number(1.0),
                );
                body.push(exit_statement(js, status));
                body.push(js.export_default(js.identifier("$failed")));
            }
        }
        js.program(body)
    })
}

fn exit_statement<'a>(js: &JsAst<'a>, status: oxc_ast::ast::Expression<'a>) -> Statement<'a> {
    let host = js.member(js.identifier("globalThis"), "__alderHost");
    js.expression_statement(js.call(js.member(host, "exit"), [status]))
}

fn generated_module(
    module_id: &str,
    build: impl for<'a> FnOnce(&JsAst<'a>) -> Program<'a>,
) -> EmittedModule {
    let mut ast = EcmaAst {
        source_type: SourceType::mjs(),
        ..EcmaAst::default()
    };
    ast.program.with_mut(|fields| {
        let js = JsAst::new(fields.allocator);
        *fields.program = build(&js);
    });
    EmittedModule {
        module_id: module_id.to_owned(),
        ast,
        dependencies: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_generated_entry_asts() {
        let modules = vec!["alder://app/main.mjs".to_owned()];
        insta::assert_snapshot!(
            "support_entry_standalone",
            entry_module("alder://app/main.mjs", EntryKind::Standalone, &modules).code()
        );
        insta::assert_snapshot!(
            "support_entry_cloudflare",
            entry_module("alder://app/main.mjs", EntryKind::Cloudflare, &modules).code()
        );
        insta::assert_snapshot!(
            "support_entry_test",
            entry_module("alder://app/main.mjs", EntryKind::Test, &modules).code()
        );
    }
}
