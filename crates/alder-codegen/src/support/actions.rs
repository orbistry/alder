//! Typed action-record replacements; all property types come from solved headers.
use crate::EmittedModule;

pub fn client_stub(module_id: &str, route_id: &str, names: &[String]) -> EmittedModule {
    super::generated_module(module_id, |js| {
        let mut body = js.vec();
        body.push(js.import(
            "alder:kernel",
            &[("$webAction".to_owned(), "$webAction".to_owned())],
        ));
        let mut properties = js.vec();
        for (index, name) in names.iter().enumerate() {
            let local = format!("$action{index}");
            let call = js.call(
                js.identifier("$webAction"),
                [js.string(route_id), js.string(name), js.identifier("input")],
            );
            body.push(js.function(
                &local,
                &["input".to_owned()],
                js.builder.vec1(js.return_statement(call)),
                false,
            ));
            properties.push(js.property(name, js.identifier(&local)));
        }
        body.push(js.variable(
            oxc_ast::ast::VariableDeclarationKind::Const,
            "$actions",
            Some(js.object(properties)),
        ));
        body.push(js.export(&[("$actions".to_owned(), "actions".to_owned())]));
        js.program(body)
    })
}
