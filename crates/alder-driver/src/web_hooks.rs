//! Solved hook, endpoint, and error-boundary contracts. Generic hook parameters
//! are checked by specialization, never rejected just for being polymorphic.
use std::collections::BTreeMap;

use alder_region::Region;

use crate::interface::*;
use crate::web_routes::RouteManifest;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractDiagnostic {
    pub source_uri: String,
    pub region: Region,
    pub message: String,
}

fn at(typ: OwnedType) -> OwnedLocatedType {
    OwnedLocatedType {
        region: Region::zero(),
        typ,
    }
}
fn underlying(typ: &OwnedType) -> &OwnedType {
    match typ {
        OwnedType::Alias { target, .. } => underlying(&target.typ),
        other => other,
    }
}
fn named(module: &[&str], name: &str, args: Vec<OwnedType>) -> OwnedType {
    OwnedType::Named {
        reference: OwnedQualifiedName {
            module: OwnedModuleId {
                package: OwnedPackageId::Builtin,
                path: module.iter().map(|part| (*part).to_owned()).collect(),
            },
            name: name.to_owned(),
        },
        args: args.into_iter().map(at).collect(),
    }
}
fn core(name: &str, args: Vec<OwnedType>) -> OwnedType {
    named(&[], name, args)
}
fn host(name: &str) -> OwnedType {
    named(&["http"], name, vec![])
}
fn task(value: OwnedType) -> OwnedType {
    core("Task", vec![value])
}
fn function(params: Vec<OwnedType>, ret: OwnedType) -> OwnedType {
    OwnedType::Fn {
        params: params.into_iter().map(at).collect(),
        ret: Box::new(at(ret)),
    }
}
fn record(fields: Vec<(&str, OwnedType)>) -> OwnedType {
    OwnedType::Record {
        fields: fields
            .into_iter()
            .enumerate()
            .map(|(index, (name, typ))| OwnedRecordField {
                index: index as u16,
                name: name.to_owned(),
                typ: at(typ),
            })
            .collect(),
        ext: None,
    }
}
fn reporting_error() -> OwnedType {
    record(vec![
        ("name", core("String", vec![])),
        ("message", core("String", vec![])),
        ("stack", core("Option", vec![core("String", vec![])])),
    ])
}
fn fetch_type() -> OwnedType {
    function(
        vec![host("Request")],
        task(core(
            "Result",
            vec![
                host("Response"),
                OwnedType::ErrorRow {
                    tags: vec![OwnedErrorTag {
                        index: 0,
                        name: "network_error".to_owned(),
                        args: vec![at(core("String", vec![]))],
                    }],
                    ext: None,
                },
            ],
        )),
    )
}
fn event(params: OwnedType) -> OwnedType {
    record(vec![
        ("request", host("Request")),
        ("url", host("Url")),
        ("params", params),
        ("fetch", fetch_type()),
    ])
}

fn semantic(typ: &OwnedType) -> serde_json::Value {
    fn erase(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(fields) => {
                if let Some(alias) = fields.remove("Alias") {
                    let mut target = alias["target"]["typ"].clone();
                    erase(&mut target);
                    *value = target;
                    return;
                }
                fields.remove("region");
                fields.remove("index");
                for value in fields.values_mut() {
                    erase(value);
                }
                for key in ["fields", "tags"] {
                    if let Some(serde_json::Value::Array(items)) = fields.get_mut(key) {
                        items.sort_by(|left, right| {
                            left["name"].as_str().cmp(&right["name"].as_str())
                        });
                    }
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    erase(value);
                }
            }
            _ => {}
        }
    }
    let mut value = serde_json::to_value(typ).expect("owned types serialize");
    erase(&mut value);
    value
}

// The actual scheme's variables may specialize to the contract. The contract's
// opaque HookParams skolem cannot specialize to one route's concrete params.
fn specialize(
    actual: &OwnedType,
    expected: &OwnedType,
    vars: &mut BTreeMap<String, OwnedType>,
    produced: bool,
) -> bool {
    let (actual, expected) = (underlying(actual), underlying(expected));
    if let OwnedType::Var { name, args } = actual {
        if !args.is_empty() {
            return false;
        }
        if let Some(previous) = vars.get(name) {
            return semantic(previous) == semantic(expected);
        }
        vars.insert(name.clone(), expected.clone());
        return true;
    }
    match (actual, expected) {
        (
            OwnedType::Named {
                reference: a,
                args: aa,
            },
            OwnedType::Named {
                reference: b,
                args: ba,
            },
        ) => {
            a == b
                && aa.len() == ba.len()
                && aa
                    .iter()
                    .zip(ba)
                    .all(|(a, b)| specialize(&a.typ, &b.typ, vars, produced))
        }
        (OwnedType::Fn { params: a, ret: ar }, OwnedType::Fn { params: b, ret: br }) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(a, b)| specialize(&a.typ, &b.typ, vars, !produced))
                && specialize(&ar.typ, &br.typ, vars, produced)
        }
        (OwnedType::Record { fields: a, .. }, OwnedType::Record { fields: b, .. }) => {
            if produced {
                b.iter().all(|b| {
                    a.iter()
                        .find(|a| a.name == b.name)
                        .is_some_and(|a| specialize(&a.typ.typ, &b.typ.typ, vars, produced))
                })
            } else {
                a.iter().all(|a| {
                    b.iter()
                        .find(|b| a.name == b.name)
                        .is_some_and(|b| specialize(&a.typ.typ, &b.typ.typ, vars, produced))
                })
            }
        }
        (OwnedType::Tuple(a), OwnedType::Tuple(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(a, b)| specialize(&a.typ, &b.typ, vars, produced))
        }
        (OwnedType::ErrorRow { tags: a, ext }, OwnedType::ErrorRow { tags: b, .. }) => {
            let payloads = a.iter().all(|a| {
                b.iter().find(|b| a.name == b.name).is_none_or(|b| {
                    a.args.len() == b.args.len()
                        && a.args
                            .iter()
                            .zip(&b.args)
                            .all(|(a, b)| specialize(&a.typ, &b.typ, vars, produced))
                })
            });
            payloads
                && if produced {
                    a.iter().all(|a| b.iter().any(|b| a.name == b.name))
                } else {
                    ext.is_some() || b.iter().all(|b| a.iter().any(|a| a.name == b.name))
                }
        }
        (OwnedType::Unit, OwnedType::Unit) => true,
        _ => false,
    }
}

fn accepts(value: &OwnedValue, contracts: &[OwnedType]) -> bool {
    value.scheme.trait_predicates.is_empty()
        && value.scheme.projection_equalities.is_empty()
        && contracts
            .iter()
            .any(|contract| specialize(&value.scheme.typ.typ, contract, &mut BTreeMap::new(), true))
}

/// Expected loader errors keep their typed payload; generic unused error tails
/// specialize to the closed union of tags produced by this route's loads.
pub fn page_error_type(
    manifest: &RouteManifest,
    error_uri: &str,
    interfaces: &BTreeMap<String, InterfaceFile>,
) -> Result<OwnedType, Vec<String>> {
    let mut tags: BTreeMap<String, OwnedErrorTag> = BTreeMap::new();
    let mut errors = Vec::new();
    for route in manifest
        .routes
        .iter()
        .filter(|route| route.errors.iter().any(|source| source.uri == error_uri))
    {
        for source in &route.option_sources {
            let Some(interface) = interfaces.get(&source.uri) else {
                continue;
            };
            let Some(load) = interface
                .values
                .iter()
                .find(|value| value.exported_as == "load")
            else {
                continue;
            };
            let OwnedType::Fn { ret, .. } = underlying(&load.scheme.typ.typ) else {
                continue;
            };
            let mut output = underlying(&ret.typ);
            while let OwnedType::Named { reference, args } = output {
                if reference.module.package != OwnedPackageId::Builtin
                    || !reference.module.path.is_empty()
                {
                    break;
                }
                if reference.name == "Task" && args.len() == 1 {
                    output = underlying(&args[0].typ);
                    continue;
                }
                if reference.name == "Result" && args.len() == 2 {
                    match underlying(&args[1].typ) {
                        OwnedType::ErrorRow { tags: found, .. } => {
                            for tag in found {
                                if let Some(previous) = tags.get(&tag.name) {
                                    if semantic(&OwnedType::Tuple(previous.args.clone()))
                                        != semantic(&OwnedType::Tuple(tag.args.clone()))
                                    {
                                        errors.push(format!("loader error :{} has incompatible payload types across boundary {error_uri}",tag.name));
                                    }
                                } else {
                                    tags.insert(tag.name.clone(), tag.clone());
                                }
                            }
                        }
                        OwnedType::Var { args, .. } if args.is_empty() => {}
                        _ => errors.push(format!(
                            "load in {} must use a tagged error row for an error boundary",
                            source.uri
                        )),
                    }
                }
                break;
            }
        }
    }
    let expected = OwnedType::ErrorRow {
        tags: tags
            .into_values()
            .enumerate()
            .map(|(index, mut tag)| {
                tag.index = index as u16;
                tag
            })
            .collect(),
        ext: None,
    };
    if errors.is_empty() {
        Ok(named(&["http"], "PageError", vec![expected]))
    } else {
        Err(errors)
    }
}

pub fn validate(
    manifest: &RouteManifest,
    interfaces: &BTreeMap<String, InterfaceFile>,
) -> Vec<ContractDiagnostic> {
    let mut errors = Vec::new();
    let mut check = |uri: &str, value: &OwnedValue, contracts: Vec<OwnedType>, message: &str| {
        if !accepts(value, &contracts) {
            errors.push(ContractDiagnostic {
                source_uri: uri.to_owned(),
                region: value.scheme.typ.region,
                message: format!("{}: {message}", value.exported_as),
            });
        }
    };
    let request_event = event(host("$HookParams"));
    for (source, server) in [
        (manifest.hooks_server.as_ref(), true),
        (manifest.hooks_client.as_ref(), false),
    ] {
        let Some(source) = source else {
            continue;
        };
        let Some(interface) = interfaces.get(&source.uri) else {
            continue;
        };
        for value in &interface.values {
            match (value.exported_as.as_str(), server) {
                ("handle", true) => {
                    let params = vec![
                        request_event.clone(),
                        function(vec![request_event.clone()], task(host("Response"))),
                    ];
                    check(
                        &source.uri,
                        value,
                        vec![
                            function(params.clone(), task(host("Response"))),
                            function(params, host("Response")),
                        ],
                        "expected fn(RequestEvent[params], fn(RequestEvent[params]) Task[Response]) returning Response or Task[Response], valid for every route's params",
                    );
                }
                ("handleFetch", true) => check(
                    &source.uri,
                    value,
                    vec![function(
                        vec![request_event.clone(), host("Request"), fetch_type()],
                        task(core(
                            "Result",
                            vec![
                                host("Response"),
                                OwnedType::ErrorRow {
                                    tags: vec![OwnedErrorTag {
                                        index: 0,
                                        name: "network_error".to_owned(),
                                        args: vec![at(core("String", vec![]))],
                                    }],
                                    ext: None,
                                },
                            ],
                        )),
                    )],
                    "expected fn(RequestEvent[params], Request, http.Fetch) Task[Result[Response, [:network_error(String)]]]",
                ),
                ("handleError", _) => {
                    let mut params = vec![reporting_error()];
                    if server {
                        params.push(request_event.clone());
                    }
                    check(
                        &source.uri,
                        value,
                        vec![
                            function(params.clone(), OwnedType::Unit),
                            function(params, task(OwnedType::Unit)),
                        ],
                        "unexpected-error reporting must accept http.Error (plus RequestEvent on the server), returning Unit or Task[Unit]",
                    );
                }
                ("init", false) => check(
                    &source.uri,
                    value,
                    vec![
                        function(vec![], OwnedType::Unit),
                        function(vec![], task(OwnedType::Unit)),
                    ],
                    "client init must be fn() returning Unit or Task[Unit]",
                ),
                _ => {}
            }
        }
    }
    for route in &manifest.routes {
        if let Some(source) = &route.endpoint
            && let Some(interface) = interfaces.get(&source.uri)
        {
            for value in interface.values.iter().filter(|value| {
                matches!(
                    value.exported_as.as_str(),
                    "get" | "head" | "post" | "put" | "patch" | "delete" | "options"
                )
            }) {
                let params = vec![event(route.params_type())];
                check(
                    &source.uri,
                    value,
                    vec![
                        function(params.clone(), host("Response")),
                        function(params, task(host("Response"))),
                    ],
                    "endpoint must accept its typed RequestEvent and return Response or Task[Response]",
                );
            }
        }
    }
    let mut boundaries = std::collections::BTreeSet::new();
    for source in manifest.routes.iter().flat_map(|route| &route.errors) {
        if !boundaries.insert(&source.uri) {
            continue;
        }
        let Some(interface) = interfaces.get(&source.uri) else {
            continue;
        };
        let Some(value) = interface
            .values
            .iter()
            .find(|value| value.exported_as == "error")
        else {
            continue;
        };
        let contract = page_error_type(manifest, &source.uri, interfaces);
        match contract {
            Err(messages) => {
                errors.extend(messages.into_iter().map(|message| ContractDiagnostic {
                    source_uri: source.uri.clone(),
                    region: value.scheme.typ.region,
                    message,
                }))
            }
            Ok(error) => {
                let shapes = [
                    function(vec![], core("Html", vec![])),
                    function(vec![record(vec![("error", error)])], core("Html", vec![])),
                ];
                if !accepts(value, &shapes) {
                    errors.push(ContractDiagnostic{source_uri:source.uri.clone(),region:value.scheme.typ.region,message:"error component props may contain only error: PageError; load failures cannot provide complete PageData".to_owned()});
                }
            }
        }
    }
    errors
}
