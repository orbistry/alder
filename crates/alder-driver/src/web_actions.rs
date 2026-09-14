//! Solved page action contracts, independent of HTTP dispatch and source I/O.
use alder_codegen::support::remote::{Validation, WireSchema};

use crate::interface::*;
use crate::web_routes::Route;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub name: String,
    pub input: OwnedLocatedType,
    pub result: OwnedLocatedType,
    /// One-element argument tuple, matching the action transport envelope.
    pub args_schema: WireSchema,
    pub result_schema: WireSchema,
    pub args_validator: String,
    pub result_validator: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageActions {
    pub route_id: String,
    pub source_uri: String,
    pub server_module_id: String,
    pub stub_module: OwnedModuleId,
    pub stub_module_id: String,
    pub validator_module_id: String,
    /// Original solved record scheme, with identity redirected to the stub.
    pub value: OwnedValue,
    pub actions: Vec<Action>,
}

impl PageActions {
    pub fn client_stub(&self) -> alder_codegen::EmittedModule {
        alder_codegen::support::actions::client_stub(
            &self.stub_module_id,
            &self.route_id,
            &self
                .actions
                .iter()
                .map(|action| action.name.clone())
                .collect::<Vec<_>>(),
        )
    }

    pub fn validators(&self) -> alder_codegen::EmittedModule {
        alder_codegen::support::remote::validators(
            &self.validator_module_id,
            &self
                .actions
                .iter()
                .map(|action| Validation {
                    name: action.name.clone(),
                    arguments: action.args_schema.clone(),
                    result: action.result_schema.clone(),
                })
                .collect::<Vec<_>>(),
        )
    }
}

fn underlying(typ: &OwnedType) -> &OwnedType {
    match typ {
        OwnedType::Alias { target, .. } => underlying(&target.typ),
        other => other,
    }
}

fn builtin<'a>(typ: &'a OwnedType, name: &str, arity: usize) -> Option<&'a [OwnedLocatedType]> {
    match underlying(typ) {
        OwnedType::Named { reference, args }
            if reference.module.package == OwnedPackageId::Builtin
                && reference.module.path.is_empty()
                && reference.name == name
                && args.len() == arity =>
        {
            Some(args)
        }
        _ => None,
    }
}

/// Caller supplies the solved +page.server interface and all declarations used
/// by its input/result types (including dependency interfaces).
pub fn collect(
    route: &Route,
    interface: &InterfaceFile,
    all_interfaces: &[InterfaceFile],
) -> Result<Option<PageActions>, Vec<String>> {
    let Some(server) = &route.page_server else {
        return Ok(None);
    };
    let Some(value) = interface
        .values
        .iter()
        .find(|value| value.exported_as == "actions")
    else {
        return Ok(None);
    };
    let OwnedType::Record { fields, ext: None } = underlying(&value.scheme.typ.typ) else {
        return Err(vec![
            "actions must be a closed record of named action functions".to_owned(),
        ]);
    };
    if !value.scheme.trait_predicates.is_empty() || !value.scheme.projection_equalities.is_empty() {
        return Err(vec![
            "actions cannot require caller-supplied trait dictionaries".to_owned(),
        ]);
    }
    let mut actions = Vec::new();
    let mut errors = Vec::new();
    for field in fields {
        let OwnedType::Fn { params, ret } = underlying(&field.typ.typ) else {
            errors.push(format!("action {} must be a function", field.name));
            continue;
        };
        let [input] = params.as_slice() else {
            errors.push(format!(
                "action {} must accept exactly one input record",
                field.name
            ));
            continue;
        };
        if !matches!(underlying(&input.typ), OwnedType::Record { ext: None, .. }) {
            errors.push(format!(
                "action {} input must be a closed record",
                field.name
            ));
            continue;
        }
        let Some(task) = builtin(&ret.typ, "Task", 1) else {
            errors.push(format!(
                "action {} must return Task[Result[Output, Errors]]",
                field.name
            ));
            continue;
        };
        if builtin(&task[0].typ, "Result", 2).is_none() {
            errors.push(format!(
                "action {} must return Task[Result[Output, Errors]]",
                field.name
            ));
            continue;
        }
        let args_schema =
            crate::web_build::wire_schema(&OwnedType::Tuple(params.clone()), all_interfaces);
        let result_schema = crate::web_build::wire_schema(&task[0].typ, all_interfaces);
        match (args_schema, result_schema) {
            (Ok(args_schema), Ok(result_schema)) => actions.push(Action {
                name: field.name.clone(),
                input: input.clone(),
                result: task[0].clone(),
                args_schema,
                result_schema,
                args_validator: format!("{}Args", field.name),
                result_validator: format!("{}Result", field.name),
            }),
            (Err(error), _) | (_, Err(error)) => errors.push(format!(
                "action {} wire type is not serializable: {error}",
                field.name
            )),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    actions.sort_by(|left, right| left.name.cmp(&right.name));
    let bump = bumpalo::Bump::new();
    let server_module_id = alder_codegen::module_specifier(interface.hydrate(&bump).home);
    let mut stub_module = interface.module.clone();
    stub_module.path.push("__actions".to_owned());
    let mut stub_interface = interface.clone();
    stub_interface.module = stub_module.clone();
    let stub_module_id = alder_codegen::module_specifier(stub_interface.hydrate(&bump).home);
    let mut value = value.clone();
    value.identity = OwnedValueIdentity::Binding(OwnedQualifiedName {
        module: stub_module.clone(),
        name: "actions".to_owned(),
    });
    value.kind = OwnedValueKind::Let;
    Ok(Some(PageActions {
        route_id: route.id.clone(),
        source_uri: server.uri.clone(),
        server_module_id,
        validator_module_id: format!("{stub_module_id}.__validators"),
        stub_module_id,
        stub_module,
        value,
        actions,
    }))
}
