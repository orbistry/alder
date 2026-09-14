//! Remote boundary effect lifting and wire-contract validation.

use super::*;
use alder_codegen::support::remote::{Kind, WireSchema};

mod schema;

pub fn wire_schema(typ: &OwnedType, interfaces: &[InterfaceFile]) -> Result<WireSchema, String> {
    let declarations = interfaces
        .iter()
        .flat_map(|interface| &interface.types)
        .map(|declaration| (declaration.reference.clone(), declaration))
        .collect();
    schema::schema(typ, &declarations)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteExport {
    pub name: String,
    pub kind: Kind,
    pub params: Vec<OwnedLocatedType>,
    /// Completed Task value; preserves Result and its open error row.
    pub result: OwnedLocatedType,
    pub args_schema: WireSchema,
    pub result_schema: WireSchema,
    pub args_validator: String,
    pub result_validator: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteModule {
    pub source_uri: Url,
    pub module: OwnedModuleId,
    pub esm_id: String,
    pub validator_module_id: String,
    pub exports: Vec<RemoteExport>,
}

pub(super) fn lift_function<'a>(
    bump: &'a Bump,
    item: &'a Located<alder_source::Item<'a>>,
) -> &'a Located<alder_source::Item<'a>> {
    let alder_source::ItemKind::Fn(function) = item.value.kind else {
        return item;
    };
    if !matches!(item.value.visibility, alder_source::Visibility::Pub(_)) || function.is_async {
        return item;
    }
    // Explicit Task-returning functions already expose the boundary effect.
    if function.ret.is_some_and(|typ| matches!(typ.value, alder_source::Type::Named { path, .. } if path.segments.last().is_some_and(|name| name.value == "Task"))) { return item; }
    let function = bump.alloc(alder_source::FnDecl {
        is_async: true,
        name: function.name,
        params: function.params,
        ret: function.ret,
        where_clause: function.where_clause,
        body: function.body,
    });
    bump.alloc(Located::at(
        item.region,
        alder_source::Item {
            attributes: item.value.attributes,
            visibility: item.value.visibility,
            kind: alder_source::ItemKind::Fn(function),
        },
    ))
}

pub(super) fn collect(
    manifest: &RouteManifest,
    sources: &[(Url, Result<String, String>)],
    dependencies: &BuildDependencies,
    interfaces: &[InterfaceFile],
) -> Result<Vec<RemoteModule>, Vec<Diagnostic>> {
    let mut modules = Vec::new();
    let mut errors = Vec::new();
    let declarations = dependencies
        .interfaces
        .iter()
        .chain(interfaces)
        .flat_map(|interface| interface.types.iter())
        .map(|decl| (decl.reference.clone(), decl))
        .collect::<BTreeMap<_, _>>();
    for source in &manifest.remotes {
        let Ok(uri) = Url::parse(&source.uri) else {
            continue;
        };
        let Some(interface) = interfaces.iter().find(|interface| {
            dependencies.module_packages.get(&uri) == Some(&interface.module.package)
                && dependencies.module_paths.get(&uri) == Some(&interface.module.path)
        }) else {
            continue;
        };
        let source_text = sources
            .iter()
            .find(|(candidate, _)| candidate == &uri)
            .and_then(|(_, text)| text.as_ref().ok())
            .cloned()
            .unwrap_or_default();
        let arena = Bump::new();
        let Ok(parsed) = alder_parse::parse_module(&arena, arena.alloc_str(&source_text)) else {
            continue;
        };
        let mut functions = BTreeMap::new();
        for item in parsed.items {
            if !matches!(item.value.visibility, alder_source::Visibility::Pub(_)) {
                continue;
            }
            let alder_source::ItemKind::Fn(function) = item.value.kind else {
                // Types may be public, but executable public values and reexports
                // have no generated HTTP invocation contract.
                if !matches!(
                    item.value.kind,
                    alder_source::ItemKind::TypeAlias(_) | alder_source::ItemKind::Error(_)
                ) {
                    errors.push(error(&uri, &source_text, item.region, "remote modules may export functions and data types, but not values or reexports"));
                }
                continue;
            };
            let kinds = item
                .value
                .attributes
                .iter()
                .filter(|attribute| matches!(attribute.value.name.value, "query" | "command"))
                .collect::<Vec<_>>();
            if kinds.len() > 1
                || kinds
                    .iter()
                    .any(|attribute| !attribute.value.args.is_empty())
            {
                errors.push(error(
                    &uri,
                    &source_text,
                    item.region,
                    "use at most one argument-free #[query] or #[command] on a remote function",
                ));
            }
            let kind = if kinds
                .first()
                .is_some_and(|attribute| attribute.value.name.value == "command")
            {
                Kind::Command
            } else {
                Kind::Query
            };
            functions.insert(function.name.value, (function.name.region, kind));
        }
        let mut exports = Vec::new();
        for value in &interface.values {
            let Some((region, kind)) = functions.get(value.exported_as.as_str()) else {
                continue;
            };
            let OwnedType::Fn { params, ret } = underlying(&value.scheme.typ.typ) else {
                errors.push(error(
                    &uri,
                    &source_text,
                    *region,
                    "a remote export must be a function",
                ));
                continue;
            };
            let OwnedType::Named { reference, args } = underlying(&ret.typ) else {
                errors.push(error(
                    &uri,
                    &source_text,
                    *region,
                    "remote functions must produce Task values",
                ));
                continue;
            };
            if reference.module.package != OwnedPackageId::Builtin
                || reference.name != "Task"
                || args.len() != 1
            {
                errors.push(error(
                    &uri,
                    &source_text,
                    *region,
                    "remote functions must produce exactly one Task layer",
                ));
                continue;
            }
            let result = args[0].clone();
            let argument_schema = schema::schema(&OwnedType::Tuple(params.clone()), &declarations);
            let result_schema = schema::schema(&result.typ, &declarations);
            for schema in [&argument_schema, &result_schema] {
                if let Err(reason) = schema {
                    errors.push(error(
                        &uri,
                        &source_text,
                        *region,
                        &format!("remote wire type is not serializable: {reason}"),
                    ));
                }
            }
            if !value.scheme.trait_predicates.is_empty()
                || !value.scheme.projection_equalities.is_empty()
            {
                errors.push(error(
                    &uri,
                    &source_text,
                    *region,
                    "remote functions cannot require caller-supplied trait dictionaries",
                ));
                continue;
            }
            if let (Ok(args_schema), Ok(result_schema)) = (argument_schema, result_schema) {
                exports.push(RemoteExport {
                    name: value.exported_as.clone(),
                    kind: *kind,
                    params: params.clone(),
                    result,
                    args_schema,
                    result_schema,
                    args_validator: format!("{}Args", value.exported_as),
                    result_validator: format!("{}Result", value.exported_as),
                });
            }
        }
        exports.sort_by(|a, b| a.name.cmp(&b.name));
        let esm_id = alder_codegen::support::remote::module_id(interface.hydrate(&arena).home);
        modules.push(RemoteModule {
            source_uri: uri,
            module: interface.module.clone(),
            validator_module_id: format!("{esm_id}.__validators"),
            esm_id,
            exports,
        });
    }
    if errors.is_empty() {
        Ok(modules)
    } else {
        Err(errors)
    }
}

fn error(uri: &Url, source: &str, region: Region, message: &str) -> Diagnostic {
    Diagnostic::error(Source::new(uri, source), message)
        .with_code("alder::web::remote_contract")
        .with_primary_label(region, "remote function contract")
}
