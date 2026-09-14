//! Source-AST option evaluation and typed build-time entries contracts.
use super::*;
use crate::web_routes::{PageOptionExports, PageOptions, Params, Segment, TrailingSlash};
use alder_codegen::support::remote::WireSchema;
use alder_source::{Expr, ItemKind, Pattern, Visibility};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntriesKind {
    Value,
    Function,
}

#[derive(Clone, Debug)]
pub struct EntriesExport {
    pub source_uri: String,
    pub esm_id: String,
    pub kind: EntriesKind,
    pub is_async: bool,
    /// Validate the completed Array[Params], not the Task/function wrapper.
    pub schema: WireSchema,
}

#[derive(Clone, Debug)]
pub struct ResolvedRouteOptions {
    pub options: PageOptions,
    /// Statically known paths. Dynamic entries are evaluated by the build host.
    pub prerender_paths: Vec<String>,
    pub entries: Option<EntriesExport>,
}

pub(super) fn annotate<'a>(
    bump: &'a Bump,
    item: &'a Located<alder_source::Item<'a>>,
    route: bool,
) -> &'a Located<alder_source::Item<'a>> {
    if !route || !matches!(item.value.visibility, Visibility::Pub(_)) {
        return item;
    }
    let is_entries = match item.value.kind {
        ItemKind::Let(decl) => {
            matches!(decl.pattern.value, Pattern::Var("entries")) && decl.annotation.is_none()
        }
        ItemKind::Fn(decl) => decl.name.value == "entries" && decl.ret.is_none(),
        _ => false,
    };
    if !is_entries {
        return item;
    }
    let named = |name| alder_source::Path {
        segments: bump.alloc_slice_copy(&[Located::at(item.region, name)]),
    };
    let params = bump.alloc(Located::at(
        item.region,
        alder_source::Type::Named {
            path: named("Params"),
            args: &[],
        },
    ));
    let typ = bump.alloc(Located::at(
        item.region,
        alder_source::Type::Named {
            path: named("Array"),
            args: bump.alloc_slice_copy(&[&*params]),
        },
    ));
    let kind = match item.value.kind {
        ItemKind::Let(decl) => ItemKind::Let(bump.alloc(alder_source::LetDecl {
            pattern: decl.pattern,
            annotation: Some(typ),
            value: decl.value,
        })),
        ItemKind::Fn(decl) => ItemKind::Fn(bump.alloc(alder_source::FnDecl {
            is_async: decl.is_async,
            name: decl.name,
            params: decl.params,
            ret: Some(typ),
            where_clause: decl.where_clause,
            body: decl.body,
        })),
        _ => unreachable!(),
    };
    bump.alloc(Located::at(
        item.region,
        alder_source::Item {
            attributes: item.value.attributes,
            visibility: item.value.visibility,
            kind,
        },
    ))
}

fn resolve<'a>(
    expr: &'a Located<Expr<'a>>,
    lets: &BTreeMap<&str, &'a Located<Expr<'a>>>,
    depth: usize,
) -> Option<&'a Located<Expr<'a>>> {
    if depth > lets.len() {
        return None;
    }
    match expr.value {
        Expr::Var(name) => resolve(lets.get(name)?, lets, depth + 1),
        _ => Some(expr),
    }
}

fn boolean(
    expr: &Located<Expr<'_>>,
    lets: &BTreeMap<&str, &Located<Expr<'_>>>,
    depth: usize,
) -> Option<bool> {
    if depth > 128 {
        return None;
    }
    match resolve(expr, lets, 0)?.value {
        Expr::Bool(value) => Some(value),
        Expr::Not(value) => Some(!boolean(value, lets, depth + 1)?),
        _ => None,
    }
}

fn static_entries(
    expr: &Located<Expr<'_>>,
    lets: &BTreeMap<&str, &Located<Expr<'_>>>,
) -> Option<Vec<Params>> {
    let Expr::Array(items) = resolve(expr, lets, 0)?.value else {
        return None;
    };
    items
        .iter()
        .map(|item| {
            let Expr::Record(fields) = resolve(item, lets, 0)?.value else {
                return None;
            };
            let mut params = Params::new();
            for field in fields {
                let alder_source::RecordField::Field { name, value } = field else {
                    return None;
                };
                let expr = match value {
                    Some(expr) => expr,
                    None => lets.get(name.value)?,
                };
                match resolve(expr, lets, 0)?.value {
                    Expr::Str(value) => {
                        params.insert(name.value.to_owned(), value.to_owned());
                    }
                    Expr::Path(path)
                        if path
                            .segments
                            .last()
                            .is_some_and(|part| part.value == "None") => {}
                    _ => return None,
                }
            }
            Some(params)
        })
        .collect()
}

pub(super) fn collect(
    manifest: &RouteManifest,
    sources: &[(Url, Result<String, String>)],
    dependencies: &BuildDependencies,
    interfaces: &[InterfaceFile],
) -> (BTreeMap<String, ResolvedRouteOptions>, Vec<Diagnostic>) {
    let mut exports = BTreeMap::new();
    let mut entries = BTreeMap::new();
    let mut errors = Vec::new();
    let all = dependencies
        .interfaces
        .iter()
        .chain(interfaces)
        .cloned()
        .collect::<Vec<_>>();
    let option_uris = manifest
        .routes
        .iter()
        .flat_map(|route| &route.option_sources)
        .map(|source| source.uri.as_str())
        .collect::<BTreeSet<_>>();
    for (uri, text) in sources {
        if !option_uris.contains(uri.as_str()) {
            continue;
        }
        let Ok(text) = text else {
            continue;
        };
        let bump = Bump::new();
        let Ok(module) = alder_parse::parse_module(&bump, bump.alloc_str(text)) else {
            continue;
        };
        let lets = module
            .items
            .iter()
            .filter_map(|item| match item.value.kind {
                ItemKind::Let(decl) => match decl.pattern.value {
                    Pattern::Var(name) => Some((name, decl.value)),
                    _ => None,
                },
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        let mut options = PageOptionExports::default();
        for item in module.items {
            if !matches!(item.value.visibility, Visibility::Pub(_)) {
                continue;
            }
            let (name, expression, kind, is_async, arity) = match item.value.kind {
                ItemKind::Let(decl) => match decl.pattern.value {
                    Pattern::Var(name) => (name, Some(decl.value), EntriesKind::Value, false, 0),
                    _ => continue,
                },
                ItemKind::Fn(decl) => (
                    decl.name.value,
                    None,
                    EntriesKind::Function,
                    decl.is_async,
                    decl.params.len(),
                ),
                _ => continue,
            };
            let error = |message: &str| {
                Diagnostic::error(Source::new(uri, text), message)
                    .with_code("alder::web::page_option")
                    .with_primary_label(item.region, "page option export")
            };
            match name {
                "ssr" | "csr" | "prerender" => {
                    let value = expression.and_then(|expr| boolean(expr, &lets, 0));
                    if value.is_none() {
                        errors.push(error(&format!(
                            "{name} must be a statically known Bool value"
                        )));
                    }
                    match name {
                        "ssr" => options.ssr = value,
                        "csr" => options.csr = value,
                        _ => options.prerender = value,
                    }
                }
                "trailingSlash" => {
                    let typed = interfaces.iter().find(|interface| dependencies.module_paths.get(uri) == Some(&interface.module.path) && dependencies.module_packages.get(uri) == Some(&interface.module.package))
                        .and_then(|interface| interface.values.iter().find(|value| value.exported_as == "trailingSlash"))
                        .is_some_and(|value| matches!(underlying(&value.scheme.typ.typ), OwnedType::Named { reference, args } if reference.name == "TrailingSlash" && reference.module.path == ["routes"] && Some(&reference.module.package) == dependencies.module_packages.get(uri) && args.is_empty()));
                    let value = expression
                        .and_then(|expr| resolve(expr, &lets, 0))
                        .and_then(|expr| {
                            let Expr::Path(path) = expr.value else {
                                return None;
                            };
                            match path.segments.last()?.value {
                                "Never" => Some(TrailingSlash::Never),
                                "Always" => Some(TrailingSlash::Always),
                                "Ignore" => Some(TrailingSlash::Ignore),
                                _ => None,
                            }
                        });
                    if value.is_none() || !typed {
                        errors.push(error(
                            "trailingSlash must be TrailingSlash::Never, Always, or Ignore",
                        ));
                    }
                    options.trailing_slash = value;
                }
                "entries" => {
                    if arity != 0 {
                        errors.push(error(
                            "entries must be an Array[Params] value or a zero-argument function",
                        ));
                        continue;
                    }
                    let Some(route) = manifest.routes.iter().find(|route| {
                        route
                            .page
                            .iter()
                            .chain(route.page_server.iter())
                            .any(|source| source.uri == uri.as_str())
                    }) else {
                        errors.push(error("entries belongs to a page, not a layout"));
                        continue;
                    };
                    let Some(interface) = interfaces.iter().find(|interface| {
                        dependencies.module_paths.get(uri) == Some(&interface.module.path)
                            && dependencies.module_packages.get(uri)
                                == Some(&interface.module.package)
                    }) else {
                        continue;
                    };
                    let Some(value) = interface
                        .values
                        .iter()
                        .find(|value| value.exported_as == "entries")
                    else {
                        continue;
                    };
                    let mut typ = underlying(&value.scheme.typ.typ);
                    if kind == EntriesKind::Function {
                        let OwnedType::Fn { params, ret } = typ else {
                            errors.push(error("entries must be a function"));
                            continue;
                        };
                        if !params.is_empty() {
                            errors.push(error("entries requires zero arguments"));
                            continue;
                        }
                        typ = underlying(&ret.typ);
                    }
                    let task = matches!(typ, OwnedType::Named { reference, .. } if reference.name == "Task" && reference.module.package == OwnedPackageId::Builtin);
                    if task {
                        if kind == EntriesKind::Value {
                            errors.push(error(
                                "entries must be an array or a function, not a Task value",
                            ));
                            continue;
                        }
                        if let OwnedType::Named { args, .. } = typ {
                            typ = underlying(&args[0].typ);
                        }
                    }
                    // Array has a builtin nominal identity; compare its member
                    // independently of the builtin module's implementation path.
                    let valid = matches!(typ, OwnedType::Named { reference, args } if reference.name == "Array" && reference.module.package == OwnedPackageId::Builtin && args.len() == 1 && accepts_data(&route.params_type(), &args[0].typ) && accepts_data(&args[0].typ, &route.params_type()));
                    if !valid {
                        errors.push(error("entries must produce Array[Params] for this route"));
                        continue;
                    }
                    let schema = match wire_schema(typ, &all) {
                        Ok(schema) => schema,
                        Err(reason) => {
                            errors.push(error(&reason));
                            continue;
                        }
                    };
                    options.entries = expression
                        .and_then(|expr| static_entries(expr, &lets))
                        .or(Some(Vec::new()));
                    entries.insert(
                        uri.to_string(),
                        EntriesExport {
                            source_uri: uri.to_string(),
                            esm_id: alder_codegen::support::remote::module_id(
                                interface.hydrate(&bump).home,
                            ),
                            kind,
                            is_async: is_async || task,
                            schema,
                        },
                    );
                }
                _ => {}
            }
        }
        exports.insert(uri.to_string(), options);
    }
    let mut result = BTreeMap::new();
    for route in &manifest.routes {
        match route.resolve_options(&exports) {
            Ok(options) => {
                let entry = route
                    .option_sources
                    .iter()
                    .rev()
                    .find_map(|source| entries.get(&source.uri))
                    .cloned();
                let prerender_paths = if !options.prerender {
                    Vec::new()
                } else if let Some(entries) = &options.entries {
                    entries
                        .iter()
                        .filter_map(|params| route.href(params, options.trailing_slash).ok())
                        .collect()
                } else if route
                    .segments
                    .iter()
                    .all(|segment| matches!(segment, Segment::Static(_)))
                {
                    route
                        .href(&Params::new(), options.trailing_slash)
                        .into_iter()
                        .collect()
                } else {
                    Vec::new()
                };
                result.insert(
                    route.id.clone(),
                    ResolvedRouteOptions {
                        options,
                        prerender_paths,
                        entries: entry,
                    },
                );
            }
            Err(error) => errors.push(route_diagnostic(sources, error)),
        }
    }
    (result, errors)
}
