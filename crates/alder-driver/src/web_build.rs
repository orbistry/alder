//! Staged, source-preserving web compilation. No filesystem or CLI subprocesses.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::Path;

use alder_region::{Located, Region};
use alder_report::{Diagnostic, Source};
use bumpalo::Bump;
use url::Url;

use crate::compile::{BuildDependencies, BuildMode, BuildResult, ModuleResult, build_sync_for_web};
use crate::interface::*;
use crate::progress::{Reporter, Silent};
use crate::web_routes::{self, Route, RouteDiagnostic, RouteManifest, RouteTypes, SourceFile};

mod remote;
pub use remote::{RemoteExport, RemoteModule, wire_schema};
mod options;
pub use options::{EntriesExport, EntriesKind, ResolvedRouteOptions};

#[derive(Clone, Debug)]
pub(crate) struct ModuleOptions {
    header: Option<OwnedModuleId>,
    loads_only: bool,
    remote: bool,
    actions: bool,
    page_error: bool,
}

pub(crate) fn prepare_interface<'a>(
    bump: &'a Bump,
    mut interface: alder_ast::Interface<'a>,
    options: Option<&ModuleOptions>,
) -> alder_ast::Interface<'a> {
    if options.is_some_and(|options| options.remote) {
        // Remote calls are an effect boundary. Their server-local captured
        // stores are absent from client stubs; resources track module queries.
        interface.values = bump.alloc_slice_fill_iter(interface.values.iter().map(|value| {
            alder_ast::InterfaceValue {
                store_dependencies: &[],
                ..*value
            }
        }));
    }
    interface
}

pub(crate) fn prepare_module<'a>(
    bump: &'a Bump,
    module: alder_source::Module<'a>,
    options: Option<&ModuleOptions>,
) -> alder_source::Module<'a> {
    let Some(options) = options else {
        return module;
    };
    let mut items = Vec::new();
    if let Some(header) = &options.header {
        let path = alder_source::ModulePath {
            root: alder_source::ModuleRoot::Local(Region::zero()),
            segments: bump.alloc_slice_fill_iter(
                header
                    .path
                    .iter()
                    .map(|part| Located::at_zero(bump.alloc_str(part) as &str)),
            ),
        };
        let import = bump.alloc(alder_source::Import {
            path: Located::at_zero(path),
            tail: alder_source::ImportTail::Names(
                bump.alloc_slice_fill_iter(
                    ["Params", "LoadEvent", "PageData"]
                        .into_iter()
                        .chain(options.actions.then_some("actions"))
                        .chain(options.page_error.then_some("PageError"))
                        .map(|name| alder_source::ImportName {
                            name: Located::at_zero(name),
                            alias: None,
                        })
                        .collect::<Vec<_>>(),
                ),
            ),
        });
        let import = bump.alloc(Located::at_zero(alder_source::Item {
            attributes: &[],
            visibility: alder_source::Visibility::Private,
            kind: alder_source::ItemKind::Import(import),
        }));
        items.push(&*import);
        let import = bump.alloc(alder_source::Import {
            path: Located::at_zero(alder_source::ModulePath {
                root: alder_source::ModuleRoot::Local(Region::zero()),
                segments: bump.alloc_slice_copy(&[Located::at_zero("routes")]),
            }),
            tail: alder_source::ImportTail::Names(bump.alloc_slice_copy(&[
                alder_source::ImportName {
                    name: Located::at_zero("TrailingSlash"),
                    alias: None,
                },
            ])),
        });
        items.push(bump.alloc(Located::at_zero(alder_source::Item {
            attributes: &[],
            visibility: alder_source::Visibility::Private,
            kind: alder_source::ItemKind::Import(import),
        })));
    }
    for item in module.items.iter().copied().filter(|item| {
        !options.loads_only || !matches!(item.value.kind, alder_source::ItemKind::Component(_))
    }) {
        items.push(if options.remote {
            remote::lift_function(bump, item)
        } else {
            options::annotate(bump, item, options.header.is_some())
        });
    }
    alder_source::Module {
        items: bump.alloc_slice_fill_iter(items),
        comments: module.comments,
    }
}

#[derive(Debug)]
pub struct WebBuildResult {
    pub manifest: Option<RouteManifest>,
    pub build: BuildResult,
    pub route_types: BTreeMap<String, RouteTypes>,
    /// Generated source participates in the same compiler pipeline as user code.
    pub routes_module: Option<OwnedModuleId>,
    pub remotes: Vec<RemoteModule>,
    /// Replace corresponding original modules with these before client bundling.
    pub client_replacements: HashMap<Url, alder_codegen::EmittedModule>,
    pub server_replacements: HashMap<Url, alder_codegen::EmittedModule>,
    pub server_implementations: HashMap<String, alder_codegen::EmittedModule>,
    pub validation_artifacts: HashMap<String, alder_codegen::EmittedModule>,
    pub actions: Vec<crate::web_actions::PageActions>,
    pub action_artifacts: HashMap<String, alder_codegen::EmittedModule>,
    pub page_options: BTreeMap<String, ResolvedRouteOptions>,
    pub client_store_keys: Vec<String>,
}

/// Synchronous entry for a host's blocking compilation worker. Identity metadata
/// for user modules has the same explicit contract as ordinary driver builds.
pub fn build_sources(
    source_root: &Path,
    sources: Vec<(Url, Result<String, String>)>,
    mode: BuildMode,
    dependencies: BuildDependencies,
) -> WebBuildResult {
    build_sources_with_reporter(source_root, sources, mode, dependencies, &Silent)
}

pub fn build_sources_with_reporter(
    source_root: &Path,
    mut sources: Vec<(Url, Result<String, String>)>,
    mode: BuildMode,
    mut dependencies: BuildDependencies,
    reporter: &dyn Reporter,
) -> WebBuildResult {
    let files: Vec<_> = sources
        .iter()
        .filter_map(|(uri, _)| {
            uri.to_file_path().ok().map(|path| SourceFile {
                path,
                uri: uri.to_string(),
            })
        })
        .collect();
    let manifest = match web_routes::discover(source_root, &files) {
        Ok(manifest) => manifest,
        Err(errors) => {
            return failed(
                None,
                &sources,
                errors
                    .into_iter()
                    .map(|error| route_diagnostic(&sources, error))
                    .collect(),
            );
        }
    };
    // `.remote` is a boundary marker, not part of Alder's slash-separated
    // import spelling: lib/users.remote.ald is imported as ~/lib/users.
    for source in &manifest.remotes {
        if let Ok(uri) = Url::parse(&source.uri)
            && let Some(path) = dependencies.module_paths.get_mut(&uri)
            && let Some(last) = path.last_mut()
            && let Some(name) = last.strip_suffix(".remote")
        {
            *last = name.to_owned();
        }
    }
    let boundary_errors = boundary_diagnostics(&manifest, &sources, &dependencies);
    if !boundary_errors.is_empty() {
        return failed(Some(manifest), &sources, boundary_errors);
    }
    let owner = manifest
        .routes
        .first()
        .and_then(|route| Url::parse(&route.source().uri).ok())
        .and_then(|uri| dependencies.module_packages.get(&uri))
        .cloned()
        .unwrap_or(OwnedPackageId::Application);
    let routes_module = OwnedModuleId {
        package: owner.clone(),
        path: vec!["routes".into()],
    };
    if dependencies.module_paths.iter().any(|(uri, path)| {
        path == &routes_module.path && dependencies.module_packages.get(uri) == Some(&owner)
    }) {
        return failed(
            Some(manifest),
            &sources,
            vec![
                Diagnostic::error(
                    Source::new("routes", ""),
                    "the generated ~/routes module conflicts with a source module",
                )
                .with_code("alder::web::routes_module_conflict"),
            ],
        );
    }
    let routes_uri = Url::parse("alder-generated:///routes.ald").expect("constant URL");
    dependencies
        .module_paths
        .insert(routes_uri.clone(), routes_module.path.clone());
    dependencies
        .module_packages
        .insert(routes_uri.clone(), owner);
    sources.push((routes_uri, Ok(routes_source(&manifest))));

    let mut module_routes: BTreeMap<Url, &Route> = BTreeMap::new();
    for route in &manifest.routes {
        for source in route
            .option_sources
            .iter()
            .chain(route.errors.iter())
            .chain(route.endpoint.iter())
        {
            if let Ok(uri) = Url::parse(&source.uri) {
                module_routes.entry(uri).or_insert(route);
            }
        }
    }
    let mut module_options = BTreeMap::new();
    for uri in module_routes.keys() {
        let Some(package) = dependencies.module_packages.get(uri) else {
            continue;
        };
        let mut name = String::from("source");
        for byte in uri.as_str().bytes() {
            use std::fmt::Write;
            write!(&mut name, "_{byte:02x}").expect("string write");
        }
        module_options.insert(
            uri.clone(),
            ModuleOptions {
                header: Some(OwnedModuleId {
                    package: package.clone(),
                    path: vec!["__alder_web_types".into(), name],
                }),
                loads_only: true,
                remote: false,
                actions: false,
                page_error: manifest
                    .routes
                    .iter()
                    .any(|route| route.errors.iter().any(|source| source.uri == uri.as_str())),
            },
        );
    }
    for source in &manifest.remotes {
        if let Ok(uri) = Url::parse(&source.uri) {
            module_options
                .entry(uri)
                .or_insert(ModuleOptions {
                    header: None,
                    loads_only: false,
                    remote: true,
                    actions: false,
                    page_error: false,
                })
                .remote = true;
        }
    }
    let base_interfaces = dependencies.interfaces.clone();
    let mut solved_by_uri = BTreeMap::new();
    // Each productive pass can make at least one more ancestor/universal load
    // available. No polling or arbitrary external retry is involved.
    for _ in 0..=module_routes.len() {
        dependencies.interfaces = base_interfaces.clone();
        let (actions, _) = collect_page_actions(&manifest, &solved_by_uri, &base_interfaces);
        for (uri, options) in &mut module_options {
            let Some(header) = &options.header else {
                continue;
            };
            let types = header_types(source_root, module_routes[uri], uri, &solved_by_uri, false);
            let action = contextual_actions(&manifest, &actions, uri);
            options.actions = action.is_some();
            let page_error = options
                .page_error
                .then(|| {
                    crate::web_hooks::page_error_type(&manifest, uri.as_str(), &solved_by_uri).ok()
                })
                .flatten();
            dependencies.interfaces.push(header_interface(
                header,
                &types,
                action,
                page_error.as_ref(),
            ));
        }
        let provisional = build_sync_for_web(
            sources.clone(),
            BuildMode::Check,
            dependencies.clone(),
            &Silent,
            &module_options,
            true,
        );
        let previous = solved_by_uri.clone();
        for interface in provisional.interfaces {
            if let Some((uri, _)) = dependencies.module_paths.iter().find(|(uri, path)| {
                *path == &interface.module.path
                    && dependencies.module_packages.get(*uri) == Some(&interface.module.package)
            }) {
                solved_by_uri.insert(uri.to_string(), interface);
            }
        }
        if previous == solved_by_uri {
            break;
        }
    }
    let (actions, action_errors) =
        collect_page_actions(&manifest, &solved_by_uri, &base_interfaces);
    dependencies.interfaces = base_interfaces;
    let mut headers = BTreeMap::new();
    let mut page_error_diagnostics = Vec::new();
    for (uri, options) in &mut module_options {
        options.loads_only = false;
        let Some(header) = &options.header else {
            continue;
        };
        let types = header_types(source_root, module_routes[uri], uri, &solved_by_uri, true);
        let action = contextual_actions(&manifest, &actions, uri);
        options.actions = action.is_some();
        let page_error = if options.page_error {
            match crate::web_hooks::page_error_type(&manifest, uri.as_str(), &solved_by_uri) {
                Ok(typ) => Some(typ),
                Err(messages) => {
                    page_error_diagnostics.extend(messages.into_iter().map(|message| {
                        route_diagnostic(
                            &sources,
                            RouteDiagnostic {
                                source_uri: uri.to_string(),
                                related_uri: None,
                                message,
                            },
                        )
                    }));
                    None
                }
            }
        } else {
            None
        };
        dependencies.interfaces.push(header_interface(
            header,
            &types,
            action,
            page_error.as_ref(),
        ));
        headers.insert(uri.clone(), types);
    }
    let mut route_types = BTreeMap::new();
    let mut type_errors = Vec::new();
    for route in &manifest.routes {
        match route.generate_types(&solved_by_uri) {
            Ok(types) => {
                route_types.insert(route.id.clone(), types);
            }
            Err(error) if !error.message.starts_with("missing solved interface") => {
                type_errors.push(route_diagnostic(&sources, error))
            }
            Err(_) => {} // final compile reports the source's actual failure
        }
    }
    let mut build = build_sync_for_web(
        sources.clone(),
        mode,
        dependencies.clone(),
        reporter,
        &module_options,
        false,
    );
    build.diagnostics.extend(type_errors);
    build.diagnostics.extend(page_error_diagnostics);
    build
        .diagnostics
        .extend(action_errors.into_iter().map(|(uri, message)| {
            route_diagnostic(
                &sources,
                RouteDiagnostic {
                    source_uri: uri,
                    related_uri: None,
                    message,
                },
            )
        }));
    if build.is_success() {
        build.diagnostics.extend(validate_component_exports(
            &manifest,
            &sources,
            &dependencies,
            &build.interfaces,
            &headers,
        ));
        let interfaces = build
            .interfaces
            .iter()
            .filter_map(|interface| {
                dependencies
                    .module_paths
                    .iter()
                    .find(|(uri, path)| {
                        *path == &interface.module.path
                            && dependencies.module_packages.get(*uri)
                                == Some(&interface.module.package)
                    })
                    .map(|(uri, _)| (uri.to_string(), interface.clone()))
            })
            .collect();
        build.diagnostics.extend(
            crate::web_hooks::validate(&manifest, &interfaces)
                .into_iter()
                .map(|error| {
                    let text = sources
                        .iter()
                        .find(|(uri, _)| uri.as_str() == error.source_uri)
                        .and_then(|(_, text)| text.as_ref().ok())
                        .cloned()
                        .unwrap_or_default();
                    Diagnostic::error(Source::new(error.source_uri, text), error.message)
                        .with_code("alder::web::hook_contract")
                        .with_primary_label(error.region, "web export contract")
                }),
        );
    }
    let mut remotes = Vec::new();
    let mut client_replacements = HashMap::new();
    let mut server_replacements = HashMap::new();
    let mut server_implementations = HashMap::new();
    let mut validation_artifacts = HashMap::new();
    let mut action_artifacts = HashMap::new();
    let mut page_options = BTreeMap::new();
    if build.is_success() {
        let (resolved, errors) =
            options::collect(&manifest, &sources, &dependencies, &build.interfaces);
        page_options = resolved;
        build.diagnostics.extend(errors);
    }
    if build.is_success() {
        for action in &actions {
            action_artifacts.insert(action.stub_module_id.clone(), action.client_stub());
            validation_artifacts.insert(action.validator_module_id.clone(), action.validators());
        }
    }
    if build.is_success() {
        match remote::collect(&manifest, &sources, &dependencies, &build.interfaces) {
            Ok(modules) => {
                for module in &modules {
                    validation_artifacts.insert(
                        module.validator_module_id.clone(),
                        alder_codegen::support::remote::validators(
                            &module.validator_module_id,
                            &module
                                .exports
                                .iter()
                                .map(|export| alder_codegen::support::remote::Validation {
                                    name: export.name.clone(),
                                    arguments: export.args_schema.clone(),
                                    result: export.result_schema.clone(),
                                })
                                .collect::<Vec<_>>(),
                        ),
                    );
                    let stub = alder_codegen::support::remote::client_stub(
                        &module.esm_id,
                        &module
                            .exports
                            .iter()
                            .map(|export| alder_codegen::support::remote::Function {
                                name: export.name.clone(),
                                arity: export.params.len(),
                                kind: export.kind,
                            })
                            .collect::<Vec<_>>(),
                    );
                    client_replacements.insert(module.source_uri.clone(), stub);
                    let implementation_id = format!("{}.__server", module.esm_id);
                    if let Some(original) = build.artifacts.get(&module.source_uri) {
                        let mut implementation = original.clone();
                        implementation.module_id = implementation_id.clone();
                        server_implementations.insert(implementation_id.clone(), implementation);
                        server_replacements.insert(
                            module.source_uri.clone(),
                            alder_codegen::support::remote::server_stub(
                                &module.esm_id,
                                &implementation_id,
                                &module
                                    .exports
                                    .iter()
                                    .map(|export| alder_codegen::support::remote::Function {
                                        name: export.name.clone(),
                                        arity: export.params.len(),
                                        kind: export.kind,
                                    })
                                    .collect::<Vec<_>>(),
                            ),
                        );
                    }
                }
                remotes = modules;
            }
            Err(errors) => build.diagnostics.extend(errors),
        }
    }
    if !build.is_success() {
        build.artifacts.clear();
        build.interfaces.clear();
        build.package_instance_indexes.clear();
        client_replacements.clear();
        server_replacements.clear();
        server_implementations.clear();
        validation_artifacts.clear();
        action_artifacts.clear();
    }
    let client_store_keys = browser_store_keys(&manifest, &build.artifacts, &client_replacements);
    WebBuildResult {
        manifest: Some(manifest),
        build,
        route_types,
        routes_module: Some(routes_module),
        remotes,
        client_replacements,
        server_replacements,
        server_implementations,
        validation_artifacts,
        actions,
        action_artifacts,
        page_options,
        client_store_keys,
    }
}

fn browser_store_keys(
    manifest: &RouteManifest,
    artifacts: &HashMap<Url, alder_codegen::EmittedModule>,
    remote_stubs: &HashMap<Url, alder_codegen::EmittedModule>,
) -> Vec<String> {
    let mut roots = BTreeSet::new();
    for route in &manifest.routes {
        roots.extend(route.page.iter().chain(&route.errors).map(|file| &file.uri));
        roots.extend(
            route
                .layouts
                .iter()
                .filter_map(|layout| layout.universal.as_ref())
                .map(|file| &file.uri),
        );
    }
    roots.extend(manifest.hooks_client.iter().map(|file| &file.uri));
    let modules = artifacts
        .values()
        .map(|artifact| (artifact.module_id.as_str(), artifact))
        .collect::<BTreeMap<_, _>>();
    let remote = remote_stubs
        .values()
        .map(|artifact| artifact.module_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut pending = roots
        .into_iter()
        .filter_map(|uri| artifacts.get(&Url::parse(uri).ok()?))
        .map(|artifact| artifact.module_id.as_str())
        .collect::<Vec<_>>();
    let mut visited = BTreeSet::new();
    let mut stores = BTreeSet::new();
    while let Some(module) = pending.pop() {
        if !visited.insert(module) || remote.contains(module) {
            continue;
        }
        if let Some(artifact) = modules.get(module) {
            stores.extend(artifact.store_keys.iter().cloned());
            pending.extend(artifact.dependencies.iter().map(String::as_str));
        }
    }
    stores.into_iter().collect()
}

fn collect_page_actions(
    manifest: &RouteManifest,
    interfaces: &BTreeMap<String, InterfaceFile>,
    dependencies: &[InterfaceFile],
) -> (Vec<crate::web_actions::PageActions>, Vec<(String, String)>) {
    let all = dependencies
        .iter()
        .chain(interfaces.values())
        .cloned()
        .collect::<Vec<_>>();
    let mut actions = Vec::new();
    let mut errors = Vec::new();
    for route in &manifest.routes {
        let Some(server) = &route.page_server else {
            continue;
        };
        let Some(interface) = interfaces.get(&server.uri) else {
            continue;
        };
        match crate::web_actions::collect(route, interface, &all) {
            Ok(Some(action)) => actions.push(action),
            Ok(None) => {}
            Err(messages) => errors.extend(
                messages
                    .into_iter()
                    .map(|message| (server.uri.clone(), message)),
            ),
        }
    }
    (actions, errors)
}

fn contextual_actions<'a>(
    manifest: &RouteManifest,
    actions: &'a [crate::web_actions::PageActions],
    uri: &Url,
) -> Option<&'a OwnedValue> {
    let route = manifest.routes.iter().find(|route| {
        route
            .page
            .as_ref()
            .is_some_and(|page| page.uri == uri.as_str())
    })?;
    actions
        .iter()
        .find(|actions| actions.route_id == route.id)
        .map(|actions| &actions.value)
}

fn underlying(mut typ: &OwnedType) -> &OwnedType {
    while let OwnedType::Alias { target, .. } = typ {
        typ = &target.typ;
    }
    typ
}

fn accepts_data(expected: &OwnedType, actual: &OwnedType) -> bool {
    match (underlying(expected), underlying(actual)) {
        (
            OwnedType::Named {
                reference: left,
                args: a,
            },
            OwnedType::Named {
                reference: right,
                args: b,
            },
        ) => {
            left == right
                && a.len() == b.len()
                && a.iter().zip(b).all(|(a, b)| accepts_data(&a.typ, &b.typ))
        }
        (OwnedType::Record { fields: a, .. }, OwnedType::Record { fields: b, .. }) => {
            a.iter().all(|a| {
                b.iter()
                    .find(|b| a.name == b.name)
                    .is_some_and(|b| accepts_data(&a.typ.typ, &b.typ.typ))
            })
        }
        (OwnedType::Tuple(a), OwnedType::Tuple(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| accepts_data(&a.typ, &b.typ))
        }
        (a, b) => a == b,
    }
}

fn validate_component_exports(
    manifest: &RouteManifest,
    sources: &[(Url, Result<String, String>)],
    dependencies: &BuildDependencies,
    interfaces: &[InterfaceFile],
    headers: &BTreeMap<Url, RouteTypes>,
) -> Vec<Diagnostic> {
    let mut components = BTreeMap::new();
    for route in &manifest.routes {
        if let Some(source) = &route.page {
            components.insert(source.uri.clone(), ("page", true));
        }
        for source in &route.errors {
            components.insert(source.uri.clone(), ("error", true));
        }
        for source in route
            .layouts
            .iter()
            .filter_map(|layout| layout.universal.as_ref())
        {
            components.insert(source.uri.clone(), ("layout", false));
        }
    }
    let mut errors = Vec::new();
    for (source_uri, (name, required)) in components {
        let Ok(uri) = Url::parse(&source_uri) else {
            continue;
        };
        let Some(interface) = interfaces.iter().find(|interface| {
            dependencies.module_paths.get(&uri) == Some(&interface.module.path)
                && dependencies.module_packages.get(&uri) == Some(&interface.module.package)
        }) else {
            continue;
        };
        let source = sources
            .iter()
            .find(|(candidate, _)| candidate == &uri)
            .and_then(|(_, source)| source.as_ref().ok())
            .cloned()
            .unwrap_or_default();
        let arena = Bump::new();
        let parsed = alder_parse::parse_module(&arena, arena.alloc_str(&source)).ok();
        let region = parsed
            .as_ref()
            .and_then(|module| {
                module.items.iter().find_map(|item| match item.value.kind {
                    alder_source::ItemKind::Component(component)
                        if component.name.value == name =>
                    {
                        Some(component.name.region)
                    }
                    alder_source::ItemKind::Fn(function) if function.name.value == name => {
                        Some(function.name.region)
                    }
                    _ => None,
                })
            })
            .unwrap_or(Region::one());
        let diagnostic = |message| {
            Diagnostic::error(Source::new(&uri, source.clone()), message)
                .with_code("alder::web::component_contract")
                .with_primary_label(region, "route component contract")
        };
        let Some(value) = interface
            .values
            .iter()
            .find(|value| value.exported_as == name)
        else {
            if required {
                errors.push(diagnostic(format!(
                    "route file must export a `{name}` component"
                )));
            }
            continue;
        };
        if value.kind != OwnedValueKind::Component {
            errors.push(diagnostic(format!("`{name}` must be a component")));
            continue;
        }
        let OwnedType::Fn { params, .. } = underlying(&value.scheme.typ.typ) else {
            errors.push(diagnostic(format!("`{name}` must be a callable component")));
            continue;
        };
        if params.is_empty() {
            continue;
        }
        if params.len() != 1 {
            errors.push(diagnostic(format!(
                "`{name}` must accept zero arguments or one props record"
            )));
            continue;
        }
        let OwnedType::Record { fields, .. } = underlying(&params[0].typ) else {
            errors.push(diagnostic(format!("`{name}` must accept a props record")));
            continue;
        };
        for field in fields {
            match field.name.as_str() {
                "data" => {
                    if let Some(header) = headers.get(&uri)
                        && !accepts_data(&field.typ.typ, &header.page_data)
                    {
                        errors.push(diagnostic(
                            "component data props do not match the generated PageData".into(),
                        ));
                    }
                }
                "children" if name == "layout" => {
                    if !matches!(underlying(&field.typ.typ), OwnedType::Named { reference, args } if reference.name == "Html" && reference.module.package == OwnedPackageId::Builtin && args.is_empty())
                    {
                        errors.push(diagnostic("layout children must have type Html".into()));
                    }
                }
                "error" if name == "error" => {}
                _ => errors.push(diagnostic(format!(
                    "the runtime does not provide the `{}` prop to `{name}`",
                    field.name
                ))),
            }
        }
    }
    errors
}

fn empty_record() -> OwnedType {
    OwnedType::Record {
        fields: vec![],
        ext: None,
    }
}
fn at(typ: OwnedType) -> OwnedLocatedType {
    OwnedLocatedType {
        region: Region::zero(),
        typ,
    }
}

fn header_types(
    source_root: &Path,
    route: &Route,
    uri: &Url,
    interfaces: &BTreeMap<String, InterfaceFile>,
    final_pass: bool,
) -> RouteTypes {
    let mut types = route
        .generate_types_before(interfaces, Some(uri.as_str()))
        .unwrap_or_else(|_| RouteTypes {
            route_id: route.id.clone(),
            params: route.params_type(),
            load_event: empty_record(),
            page_data: empty_record(),
        });
    // A shared layout has only its own directory's params, not params belonging
    // to whichever descendant happened to be first in the manifest.
    if let Ok(path) = uri.to_file_path() {
        let own_page = path.with_file_name("+page.ald");
        if let Ok(local) = web_routes::discover(
            source_root,
            &[SourceFile {
                path: own_page,
                uri: uri.to_string(),
            }],
        ) && let Some(local) = local.routes.first()
        {
            types.params = local.params_type();
        }
    }
    let input_data = types.page_data.clone();
    if final_pass {
        // Include this module's universal load, but never child layout/page data
        // in an ancestor layout's own component contract.
        let stop = route
            .option_sources
            .iter()
            .position(|source| source.uri == uri.as_str())
            .map(|index| index + 1);
        let mut local = route.clone();
        if let Some(stop) = stop {
            local.option_sources.truncate(stop);
        }
        if let Ok(complete) = local.generate_types(interfaces) {
            types.page_data = complete.page_data;
        }
    }
    types.load_event = OwnedType::Record {
        fields: vec![
            OwnedRecordField {
                index: 0,
                name: "params".into(),
                typ: at(types.params.clone()),
            },
            OwnedRecordField {
                index: 1,
                name: "data".into(),
                typ: at(input_data),
            },
            OwnedRecordField {
                index: 2,
                name: "request".into(),
                typ: at(OwnedType::Named {
                    reference: OwnedQualifiedName {
                        module: OwnedModuleId {
                            package: OwnedPackageId::Builtin,
                            path: vec!["http".into()],
                        },
                        name: "Request".into(),
                    },
                    args: vec![],
                }),
            },
            OwnedRecordField {
                index: 3,
                name: "url".into(),
                typ: at(OwnedType::Named {
                    reference: OwnedQualifiedName {
                        module: OwnedModuleId {
                            package: OwnedPackageId::Builtin,
                            path: vec!["http".into()],
                        },
                        name: "Url".into(),
                    },
                    args: vec![],
                }),
            },
            OwnedRecordField {
                index: 4,
                name: "fetch".into(),
                typ: at(http_fetch_type()),
            },
        ],
        ext: None,
    };
    types
}

fn http_fetch_type() -> OwnedType {
    let named = |path: Vec<String>, name: &str, args: Vec<OwnedLocatedType>| OwnedType::Named {
        reference: OwnedQualifiedName {
            module: OwnedModuleId {
                package: OwnedPackageId::Builtin,
                path,
            },
            name: name.into(),
        },
        args,
    };
    OwnedType::Fn {
        params: vec![at(named(vec!["http".into()], "Request", vec![]))],
        ret: Box::new(at(named(
            vec![],
            "Task",
            vec![at(named(
                vec![],
                "Result",
                vec![
                    at(named(vec!["http".into()], "Response", vec![])),
                    at(OwnedType::ErrorRow {
                        tags: vec![OwnedErrorTag {
                            index: 0,
                            name: "network_error".into(),
                            args: vec![at(named(vec![], "String", vec![]))],
                        }],
                        ext: None,
                    }),
                ],
            ))],
        ))),
    }
}

fn header_interface(
    module: &OwnedModuleId,
    types: &RouteTypes,
    actions: Option<&OwnedValue>,
    page_error: Option<&OwnedType>,
) -> InterfaceFile {
    let mut aliases = types.aliases(module);
    if let Some(typ) = page_error {
        aliases.push(OwnedTypeDecl {
            exported_as: "PageError".into(),
            reference: OwnedQualifiedName {
                module: module.clone(),
                name: "PageError".into(),
            },
            params: vec![],
            result_kind: OwnedKind::Type,
            body: OwnedPublicTypeBody::Alias(Box::new(at(typ.clone()))),
        });
    }
    let file = InterfaceFile {
        format_version: crate::interface::INTERFACE_FORMAT_VERSION,
        compiler_version: env!("CARGO_PKG_VERSION").into(),
        module: module.clone(),
        values: actions.cloned().into_iter().collect(),
        types: aliases,
        traits: vec![],
        instances: vec![],
        modules: vec![],
        private_names: vec![],
        fingerprint: [0; 32],
    };
    let arena = Bump::new();
    InterfaceFile::dehydrate(&file.hydrate(&arena)).expect("generated semantic interface")
}

/// These are real extern functions in an ordinary compiled module. The web
/// bundler provides `__alder:web/routes` from the manifest's route_links mapping.
pub fn routes_source(manifest: &RouteManifest) -> String {
    let mut source = String::from("pub enum TrailingSlash { Never, Always, Ignore }\n");
    let links = manifest.route_links();
    let mut fields = Vec::new();
    for link in &links {
        let OwnedType::Record { fields: params, .. } = &link.params else {
            unreachable!("route params record");
        };
        let params = params.iter().map(|field| {
            let optional = matches!(&field.typ.typ, OwnedType::Named { reference, .. } if reference.name == "Option");
            format!("{}{}: String", field.name, if optional { "?" } else { "" })
        }).collect::<Vec<_>>().join(", ");
        let typ = format!("{{ {params} }}");
        use std::fmt::Write;
        writeln!(
            &mut source,
            "#[extern(\"__alder:web/routes\", \"{}\")]\npub fn {}(params: {}) String",
            link.export_name, link.export_name, typ
        )
        .expect("string write");
        fields.push(format!("{}: fn({typ}) String", link.export_name));
    }
    use std::fmt::Write;
    writeln!(&mut source, "pub type Routes = {{ {} }}", fields.join(", ")).expect("string write");
    writeln!(
        &mut source,
        "pub let routes: Routes = {{ {} }}",
        links
            .iter()
            .map(|link| link.export_name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    )
    .expect("string write");
    source
}

fn route_diagnostic(
    sources: &[(Url, Result<String, String>)],
    error: RouteDiagnostic,
) -> Diagnostic {
    let source = sources
        .iter()
        .find(|(uri, _)| uri.as_str() == error.source_uri)
        .and_then(|(_, source)| source.as_ref().ok())
        .cloned()
        .unwrap_or_default();
    let mut diagnostic = Diagnostic::error(Source::new(error.source_uri, source), error.message)
        .with_code("alder::web::route")
        .with_primary_label(Region::one(), "route source");
    if let Some(related) = error.related_uri {
        diagnostic = diagnostic.with_help(format!("Conflicting source: {related}"));
    }
    diagnostic
}

fn failed(
    manifest: Option<RouteManifest>,
    sources: &[(Url, Result<String, String>)],
    diagnostics: Vec<Diagnostic>,
) -> WebBuildResult {
    WebBuildResult {
        manifest,
        route_types: BTreeMap::new(),
        routes_module: None,
        remotes: vec![],
        client_replacements: HashMap::new(),
        server_replacements: HashMap::new(),
        server_implementations: HashMap::new(),
        validation_artifacts: HashMap::new(),
        actions: vec![],
        action_artifacts: HashMap::new(),
        page_options: BTreeMap::new(),
        client_store_keys: Vec::new(),
        build: BuildResult {
            diagnostics,
            modules: sources
                .iter()
                .map(|(uri, _)| (uri.clone(), ModuleResult::Blocked))
                .collect(),
            total: sources.len(),
            success: 0,
            failed: sources.len(),
            warnings: vec![],
            artifacts: HashMap::new(),
            interfaces: vec![],
            package_instance_indexes: vec![],
        },
    }
}

/// Traverse actual parsed imports from browser roots. Remote modules terminate
/// traversal because the client consumes their generated HTTP stubs. The host
/// must replace their emitted modules before bundling client artifacts.
pub fn boundary_diagnostics(
    manifest: &RouteManifest,
    sources: &[(Url, Result<String, String>)],
    dependencies: &BuildDependencies,
) -> Vec<Diagnostic> {
    let mut identities = BTreeMap::new();
    for (uri, package) in &dependencies.module_packages {
        if let Some(path) = dependencies.module_paths.get(uri) {
            identities.insert(
                OwnedModuleId {
                    package: package.clone(),
                    path: path.clone(),
                },
                uri.clone(),
            );
        }
    }
    let mut edges: BTreeMap<Url, Vec<(Url, Region)>> = BTreeMap::new();
    let mut forbidden_builtins: BTreeMap<Url, Vec<(String, Region)>> = BTreeMap::new();
    for (uri, source) in sources {
        let Ok(source) = source else {
            continue;
        };
        let Some(package) = dependencies.module_packages.get(uri) else {
            continue;
        };
        let arena = Bump::new();
        let source = arena.alloc_str(source);
        let Ok(module) = alder_parse::parse_module(&arena, source) else {
            continue;
        };
        for (_, region, import) in module.import_entries() {
            let path: Vec<String> = import
                .path
                .value
                .segments
                .iter()
                .map(|part| part.value.into())
                .collect();
            let package = match import.path.value.root {
                alder_source::ModuleRoot::Local(_) => package.clone(),
                alder_source::ModuleRoot::StandardLibrary => {
                    if path.first().is_some_and(|name| {
                        matches!(
                            name.as_str(),
                            "fs" | "process" | "cloudflare" | "db" | "kv" | "net"
                        )
                    }) {
                        forbidden_builtins
                            .entry(uri.clone())
                            .or_default()
                            .push((path.join("/"), region));
                    }
                    OwnedPackageId::Builtin
                }
                alder_source::ModuleRoot::Package { author, package } => OwnedPackageId::Named {
                    author: author.value.into(),
                    project: package.value.into(),
                },
            };
            if let Some(target) = identities.get(&OwnedModuleId { package, path }) {
                edges
                    .entry(uri.clone())
                    .or_default()
                    .push((target.clone(), region));
            }
        }
    }
    let remote: BTreeSet<_> = manifest
        .remotes
        .iter()
        .map(|file| file.uri.as_str())
        .collect();
    let mut roots = BTreeSet::new();
    let mut server = BTreeSet::new();
    for route in &manifest.routes {
        roots.extend(
            route
                .page
                .iter()
                .chain(route.errors.iter())
                .map(|file| file.uri.clone()),
        );
        roots.extend(
            route
                .layouts
                .iter()
                .filter_map(|layout| layout.universal.as_ref())
                .map(|file| file.uri.clone()),
        );
        server.extend(
            route
                .page_server
                .iter()
                .chain(route.endpoint.iter())
                .map(|file| file.uri.clone()),
        );
        server.extend(
            route
                .layouts
                .iter()
                .filter_map(|layout| layout.server.as_ref())
                .map(|file| file.uri.clone()),
        );
    }
    roots.extend(manifest.hooks_client.iter().map(|file| file.uri.clone()));
    server.extend(manifest.hooks_server.iter().map(|file| file.uri.clone()));
    let mut diagnostics = Vec::new();
    for root in roots {
        let Ok(root) = Url::parse(&root) else {
            continue;
        };
        let mut queue = VecDeque::from([(root.clone(), vec![root.clone()])]);
        let mut visited = BTreeSet::new();
        while let Some((uri, path)) = queue.pop_front() {
            if !visited.insert(uri.clone()) || remote.contains(uri.as_str()) {
                continue;
            }
            let source_text = sources
                .iter()
                .find(|(candidate, _)| candidate == &uri)
                .and_then(|(_, source)| source.as_ref().ok())
                .cloned()
                .unwrap_or_default();
            for (name, region) in forbidden_builtins.get(&uri).into_iter().flatten() {
                diagnostics.push(
                    Diagnostic::error(
                        Source::new(&uri, source_text.clone()),
                        format!("server-only module `{name}` is reachable from browser code"),
                    )
                    .with_code("alder::web::server_boundary")
                    .with_primary_label(*region, "server-only import")
                    .with_help(format!(
                        "Import path: {} -> {name}",
                        path.iter()
                            .map(Url::as_str)
                            .collect::<Vec<_>>()
                            .join(" -> ")
                    )),
                );
            }
            for (target, region) in edges.get(&uri).into_iter().flatten() {
                let mut path = path.clone();
                path.push(target.clone());
                if server.contains(target.as_str()) {
                    diagnostics.push(
                        Diagnostic::error(
                            Source::new(&uri, source_text.clone()),
                            "server-only source is reachable from browser code",
                        )
                        .with_code("alder::web::server_boundary")
                        .with_primary_label(*region, "import crosses the server boundary")
                        .with_help(format!(
                            "Import path: {}",
                            path.iter()
                                .map(Url::as_str)
                                .collect::<Vec<_>>()
                                .join(" -> ")
                        )),
                    );
                } else {
                    queue.push_back((target.clone(), path));
                }
            }
        }
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;

    type Inputs = (Vec<(Url, Result<String, String>)>, BuildDependencies);

    fn inputs(files: &[(&str, &str)]) -> Inputs {
        let mut dependencies = BuildDependencies::default();
        let sources = files
            .iter()
            .map(|(path, source)| {
                let uri = Url::from_file_path(Path::new("/app/src").join(path)).unwrap();
                let path = path
                    .strip_suffix(".ald")
                    .unwrap()
                    .split('/')
                    .map(String::from)
                    .collect();
                dependencies.module_paths.insert(uri.clone(), path);
                dependencies
                    .module_packages
                    .insert(uri.clone(), OwnedPackageId::Application);
                (uri, Ok((*source).into()))
            })
            .collect();
        (sources, dependencies)
    }

    fn build(files: &[(&str, &str)]) -> WebBuildResult {
        let (sources, dependencies) = inputs(files);
        build_sources(
            Path::new("/app/src"),
            sources,
            BuildMode::Check,
            dependencies,
        )
    }

    fn errors(result: &WebBuildResult) -> String {
        format!("{:?}\n{:?}", result.build.diagnostics, result.build.modules)
    }

    #[test]
    fn generated_load_event_checks_route_params_before_inference() {
        let result = build(&[(
            "routes/users/[id]/+page.server.ald",
            "pub fn load(event: LoadEvent) { { id: event.params.id } }",
        )]);
        assert!(result.build.is_success(), "{}", errors(&result));
        let OwnedType::Record { fields, .. } = &result.route_types["/users/[id]"].page_data else {
            panic!("data record");
        };
        assert_eq!(fields[0].name, "id");
        let OwnedType::Named { reference, .. } = &fields[0].typ.typ else {
            panic!("String");
        };
        assert_eq!(reference.name, "String");
    }

    #[test]
    fn nonexistent_load_parameter_is_rejected() {
        let result = build(&[(
            "routes/users/[id]/+page.server.ald",
            "pub fn load(event: LoadEvent) { { id: event.params.unknown } }",
        )]);
        assert!(!result.build.is_success());
        assert!(result.build.artifacts.is_empty());
        assert!(errors(&result).contains("unknown"));
    }

    #[test]
    fn page_data_is_solved_before_component_in_same_module() {
        let result = build(&[(
            "routes/+page.ald",
            "pub fn load(event: LoadEvent) { { title: \"hello\" } }\npub component page(props: { data: PageData }) { <h1>{props.data.title}</h1> }",
        )]);
        assert!(result.build.is_success(), "{}", errors(&result));
        let OwnedType::Record { fields, .. } = &result.route_types["/"].page_data else {
            panic!("data record");
        };
        assert_eq!(fields[0].name, "title");
    }

    #[test]
    fn page_data_merges_nested_layout_and_server_and_universal_loads() {
        let result = build(&[
            (
                "routes/+layout.server.ald",
                "pub fn load(event: LoadEvent) { { root: \"root\" } }",
            ),
            (
                "routes/users/+layout.ald",
                "pub fn load(event: LoadEvent) { { inherited: event.data.root } }",
            ),
            (
                "routes/users/[id]/+page.server.ald",
                "pub fn load(event: LoadEvent) { { id: event.params.id } }",
            ),
            (
                "routes/users/[id]/+page.ald",
                "pub fn load(event: LoadEvent) { { title: event.data.id } }\npub component page(props: { data: PageData }) { <h1>{props.data.root}{props.data.inherited}{props.data.id}{props.data.title}</h1> }",
            ),
        ]);
        assert!(result.build.is_success(), "{}", errors(&result));
        let OwnedType::Record { fields, .. } = &result.route_types["/users/[id]"].page_data else {
            panic!("data record");
        };
        assert_eq!(
            fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            ["id", "inherited", "root", "title"]
        );
    }

    #[test]
    fn layout_params_do_not_include_descendant_params() {
        let result = build(&[
            (
                "routes/+layout.ald",
                "pub fn load(event: LoadEvent) { { wrong: event.params.id } }",
            ),
            (
                "routes/[id]/+page.ald",
                "pub component page() { <h1>hello</h1> }",
            ),
        ]);
        assert!(!result.build.is_success());
    }

    #[test]
    fn generated_routes_compile_and_check_link_parameter_records() {
        let (source, _) = inputs(&[("routes/[id]/+page.server.ald", "")]);
        let manifest = web_routes::discover(
            Path::new("/app/src"),
            &source
                .iter()
                .map(|(uri, _)| SourceFile {
                    path: uri.to_file_path().unwrap(),
                    uri: uri.to_string(),
                })
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let export = &manifest.route_links()[0].export_name;
        for (argument, succeeds) in [
            ("{ id: \"one\" }", true),
            ("{}", false),
            ("{ id: 42 }", false),
        ] {
            let code = format!(
                "import ~/routes\npub fn load(event: LoadEvent) {{ {{ href: routes.{export}({argument}) }} }}"
            );
            let result = build(&[("routes/[id]/+page.server.ald", &code)]);
            assert_eq!(result.build.is_success(), succeeds, "{}", errors(&result));
        }
    }

    #[test]
    fn ordinary_type_error_keeps_original_source_coordinates() {
        let code = "pub fn load(event: LoadEvent) {\n    let value: String = 42\n    { value }\n}";
        let result = build(&[("routes/+page.server.ald", code)]);
        assert!(!result.build.is_success());
        let diagnostics = result
            .build
            .modules
            .values()
            .find_map(|result| match result {
                ModuleResult::Failed { diagnostics } => Some(diagnostics),
                _ => None,
            })
            .unwrap();
        let labels = miette::Diagnostic::labels(&diagnostics[0])
            .unwrap()
            .collect::<Vec<_>>();
        assert!(
            labels
                .iter()
                .any(|label| label.offset() == code.find("42").unwrap())
        );
        assert!(errors(&result).contains("+page.server.ald"));
    }

    #[test]
    fn all_browser_roots_reject_transitive_platform_bindings_with_source_paths() {
        for root in [
            "routes/+page.ald",
            "routes/+layout.ald",
            "routes/+error.ald",
            "hooks.client.ald",
        ] {
            let mut files = vec![
                (root, "import ~/lib/bindings\npub let value = 1"),
                (
                    "lib/bindings.ald",
                    "import cloudflare.{Kv}\n#[binding(\"PRIVATE_CACHE\", \"kv\", \"private-resource-id\")]\npub type Cache = Kv",
                ),
            ];
            if root != "routes/+page.ald" {
                files.push(("routes/+page.ald", "pub component page() { <span /> }"));
            }
            let result = build(&files);
            assert!(!result.build.is_success(), "{root}");
            let rendered = errors(&result);
            assert!(
                rendered.contains("server_boundary")
                    && rendered.contains(root)
                    && rendered.contains("lib/bindings.ald")
                    && rendered.contains("cloudflare"),
                "{rendered}"
            );
            assert!(
                result.build.artifacts.is_empty(),
                "no client artifact is published after a boundary failure"
            );
        }
    }

    #[test]
    fn browser_import_of_server_companion_reports_the_original_import_edge() {
        let (sources, mut dependencies) = inputs(&[
            (
                "routes/+page.ald",
                "import ~/shared\npub component page() { <span /> }",
            ),
            ("shared.ald", "import ~/server\npub let value = 1"),
            (
                "routes/+page.server.ald",
                "pub fn load(event: LoadEvent) { { secret: \"SERVER_SECRET\" } }",
            ),
        ]);
        dependencies
            .module_paths
            .insert(sources[2].0.clone(), vec!["server".into()]);
        let result = build_sources(
            Path::new("/app/src"),
            sources,
            BuildMode::Build,
            dependencies,
        );
        let rendered = errors(&result);
        assert!(
            !result.build.is_success() && rendered.contains("server_boundary"),
            "{rendered}"
        );
        assert!(
            rendered.contains("shared.ald")
                && rendered.contains("+page.server.ald")
                && rendered.contains("+page.ald"),
            "{rendered}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bundled_browser_entry_excludes_remote_implementations_server_loads_and_hooks() {
        use alder_codegen::support::web::{Application, Route};
        let (sources, dependencies) = inputs(&[
            (
                "secret.remote.ald",
                "let secret: String = state(\"REMOTE_PRIVATE_SENTINEL\")\npub fn read() Result[String, [:missing]] { Ok(secret) }",
            ),
            (
                "routes/+page.ald",
                "import html\nimport ~/secret\npub component page() { let data = html.resource(() -> secret.read())\n<span>client-visible</span> }",
            ),
            (
                "routes/+page.server.ald",
                "pub fn load(event: LoadEvent) { { message: \"SERVER_LOAD_SENTINEL\" } }",
            ),
            (
                "hooks.server.ald",
                "import http.{RequestEvent, Response}\npub async fn handle(event: RequestEvent[p], resolve: fn(RequestEvent[p]) Task[Response]) Response { assert \"SERVER_HOOK_SENTINEL\" != \"\"\nresolve(event).await }",
            ),
        ]);
        let result = build_sources(
            Path::new("/app/src"),
            sources,
            BuildMode::Build,
            dependencies,
        );
        assert!(result.build.is_success(), "{}", errors(&result));
        let id = |path: &str| {
            result.build.artifacts[&Url::parse(&format!("file:///app/src/{path}")).unwrap()]
                .module_id
                .clone()
        };
        let app = Application {
            routes: vec![Route {
                id: "/".into(),
                segments: vec![],
                page: Some(id("routes/+page.ald")),
                server: Some(id("routes/+page.server.ald")),
                endpoint: None,
                layouts: vec![],
                errors: vec![],
                error_layouts: vec![],
                options: vec![],
            }],
            server_hook: Some(id("hooks.server.ald")),
            ..Application::default()
        };
        let original = result
            .build
            .artifacts
            .values()
            .map(|artifact| artifact.code())
            .collect::<Vec<_>>()
            .join("\n");
        for marker in [
            "REMOTE_PRIVATE_SENTINEL",
            "SERVER_LOAD_SENTINEL",
            "SERVER_HOOK_SENTINEL",
        ] {
            assert!(original.contains(marker));
        }
        let modules = result.build.artifacts.into_iter().map(|(uri, artifact)| {
            result
                .client_replacements
                .get(&uri)
                .cloned()
                .unwrap_or(artifact)
        });
        let entry = alder_codegen::support::web::entry(&app, true);
        let links = alder_codegen::support::web::links(&[alder_codegen::support::web::Link {
            name: "route_2f".into(),
            segments: vec![],
        }]);
        let code = alder_bundle::bundle_entry(modules.chain([entry, links]), "alder:web-client")
            .await
            .unwrap();
        assert!(code.contains("$webRemote") && code.contains("client-visible"));
        for marker in [
            "REMOTE_PRIVATE_SENTINEL",
            "SERVER_LOAD_SENTINEL",
            "SERVER_HOOK_SENTINEL",
            "$cloudflare",
            "$store$secret",
            ".__server",
            "hooks.server",
            "+page.server",
        ] {
            assert!(!code.contains(marker), "browser bundle retained {marker}");
        }
    }

    #[test]
    fn transitive_server_only_import_reports_path_and_original_import() {
        let result = build(&[
            (
                "routes/+page.ald",
                "import ~/lib/secret\npub component page() { <h1>hello</h1> }",
            ),
            ("lib/secret.ald", "import fs\npub let value = 42"),
        ]);
        assert!(!result.build.is_success());
        let rendered = errors(&result);
        assert!(rendered.contains("server_boundary"), "{rendered}");
        assert!(rendered.contains("lib/secret.ald"));
        assert!(rendered.contains("+page.ald"));
    }

    #[test]
    fn remote_boundary_stops_client_reachability() {
        let (sources, mut dependencies) = inputs(&[
            (
                "routes/+page.ald",
                "import ~/lib/remote\npub component page() { <h1>hello</h1> }",
            ),
            ("lib/users.remote.ald", "import fs\npub fn query() { 42 }"),
        ]);
        let remote_uri = sources[1].0.clone();
        dependencies
            .module_paths
            .insert(remote_uri, vec!["lib".into(), "remote".into()]);
        let files = sources
            .iter()
            .map(|(uri, _)| SourceFile {
                path: uri.to_file_path().unwrap(),
                uri: uri.to_string(),
            })
            .collect::<Vec<_>>();
        let manifest = web_routes::discover(Path::new("/app/src"), &files).unwrap();
        assert!(boundary_diagnostics(&manifest, &sources, &dependencies).is_empty());
    }

    #[test]
    fn remote_functions_are_implicitly_async_and_await_preserves_result() {
        let (sources, dependencies) = inputs(&[
            (
                "lib/users.remote.ald",
                "pub fn getUser(id: String) Result[{ id: String }] { Ok({ id }) }",
            ),
            (
                "routes/[id]/+page.server.ald",
                "import ~/lib/users\npub async fn load(event: LoadEvent) { users.getUser(event.params.id).await }",
            ),
        ]);
        let result = build_sources(
            Path::new("/app/src"),
            sources,
            BuildMode::Build,
            dependencies,
        );
        assert!(result.build.is_success(), "{}", errors(&result));
        assert_eq!(result.remotes.len(), 1);
        let remote = &result.remotes[0];
        assert_eq!(remote.esm_id, "alder://app/lib/users.mjs");
        let OwnedType::Named { reference, args } = &remote.exports[0].result.typ else {
            panic!("Result");
        };
        assert_eq!(reference.name, "Result");
        assert_eq!(args.len(), 2);
        let stub = &result.client_replacements[&remote.source_uri];
        let server = &result.server_replacements[&remote.source_uri];
        let implementation_id = format!("{}.__server", remote.esm_id);
        assert!(server.code().contains("$webRemoteServer"));
        assert_eq!(server.module_id, remote.esm_id);
        assert_eq!(
            result.server_implementations[&implementation_id].module_id,
            implementation_id
        );
        assert_eq!(stub.module_id, remote.esm_id);
        assert!(stub.code().contains("$webRemote"));
        assert!(stub.dependencies.is_empty());
    }

    #[test]
    fn remote_sync_call_cannot_masquerade_as_completed_value() {
        let result = build(&[
            (
                "lib/users.remote.ald",
                "pub fn getUser(id: String) String { id }",
            ),
            (
                "routes/+page.server.ald",
                "import ~/lib/users\npub fn load(event: LoadEvent) { let value: String = users.getUser(\"id\")\n{ value } }",
            ),
        ]);
        assert!(!result.build.is_success());
        assert!(errors(&result).contains("Task"));
        assert!(result.client_replacements.is_empty());
    }

    #[test]
    fn remote_commands_and_queries_have_explicit_cache_metadata() {
        let result = build(&[
            (
                "lib/users.remote.ald",
                "#[command]\npub fn deleteUser(id: String) () { () }\n#[query]\npub async fn getUser(id: String) String { id }",
            ),
            (
                "routes/+page.server.ald",
                "pub fn load(event: LoadEvent) { {} }",
            ),
        ]);
        assert!(result.build.is_success(), "{}", errors(&result));
        let exports = &result.remotes[0].exports;
        assert_eq!(
            exports[0].kind,
            alder_codegen::support::remote::Kind::Command
        );
        assert_eq!(exports[0].result.typ, OwnedType::Unit);
        assert_eq!(exports[1].kind, alder_codegen::support::remote::Kind::Query);
    }

    #[test]
    fn remote_wire_rejects_functions_unknown_generic_values_and_public_state() {
        for code in [
            "pub fn callback() { () -> 42 }",
            "pub fn identity(value) { value }",
            "pub let secret = 42",
        ] {
            let result = build(&[
                ("lib/users.remote.ald", code),
                (
                    "routes/+page.server.ald",
                    "pub fn load(event: LoadEvent) { {} }",
                ),
            ]);
            assert!(!result.build.is_success(), "{code}");
            assert!(result.client_replacements.is_empty());
            assert!(
                errors(&result).contains("remote_contract"),
                "{}",
                errors(&result)
            );
        }
    }

    #[test]
    fn hydration_store_allowlist_stops_at_remote_and_keeps_event_only_shared_dependencies() {
        let (sources, dependencies) = inputs(&[
            (
                "shared.ald",
                "let count: Number = state(0)\npub fn increment() { count += 1 }",
            ),
            (
                "only_server.ald",
                "let secret: String = state(\"PRIVATE\")\npub fn read() String { secret }",
            ),
            (
                "remote.remote.ald",
                "import ~/only_server\nlet remoteSecret: String = state(\"REMOTE\")\npub fn read() Result[String, [:missing]] { Ok(remoteSecret) }",
            ),
            (
                "routes/+page.server.ald",
                "import ~/only_server\npub fn load(event: LoadEvent) { let secret = only_server.read()\n{} }",
            ),
            (
                "routes/+page.ald",
                "import ~/shared\nimport ~/remote\nimport html\npub component page() { let value = html.resource(() -> remote.read())\n<button onClick={() -> shared.increment()}>safe</button> }",
            ),
        ]);
        let result = build_sources(
            Path::new("/app/src"),
            sources,
            BuildMode::Build,
            dependencies,
        );
        assert!(result.build.is_success(), "{}", errors(&result));
        assert_eq!(result.client_store_keys, ["alder://app/shared.mjs#count"]);
    }

    #[test]
    fn remote_private_store_captures_do_not_become_browser_component_dependencies() {
        let (sources, dependencies) = inputs(&[
            (
                "lib/counter.remote.ald",
                "let secret: Number = state(7)\nfn read() Number { secret }\npub fn current() Result[Number, [:missing]] { Ok(read()) }",
            ),
            (
                "routes/+page.ald",
                "import html\nimport ~/lib/counter\npub component page() { let value = html.resource(() -> counter.current())\n<span /> }",
            ),
        ]);
        let result = build_sources(
            Path::new("/app/src"),
            sources,
            BuildMode::Build,
            dependencies,
        );
        assert!(result.build.is_success(), "{}", errors(&result));
        let page =
            &result.build.artifacts[&Url::parse("file:///app/src/routes/+page.ald").unwrap()];
        assert!(
            !page.code().contains("$store$") && !page.code().contains("$webStoreCell"),
            "{}",
            page.code()
        );
        let remote = &result.remotes[0];
        assert!(
            result.build.artifacts[&remote.source_uri]
                .code()
                .contains("$webStoreCell"),
            "server implementation retains scoped store reads"
        );
        assert!(
            !result.client_replacements[&remote.source_uri]
                .code()
                .contains("secret")
        );
        let interface = result
            .build
            .interfaces
            .iter()
            .find(|interface| interface.module.path == ["lib", "counter"])
            .unwrap();
        assert!(
            interface
                .values
                .iter()
                .all(|value| value.store_dependencies.is_empty()),
            "remote interface caches cannot leak server store capture edges"
        );
    }

    #[test]
    fn remote_server_body_is_replaced_without_copying_implementation() {
        let (sources, dependencies) = inputs(&[
            (
                "lib/users.remote.ald",
                "pub fn getUser() String { \"SERVER_ONLY_SECRET\" }",
            ),
            (
                "routes/+page.server.ald",
                "import ~/lib/users\npub async fn load(event: LoadEvent) { { value: users.getUser().await } }",
            ),
        ]);
        let result = build_sources(
            Path::new("/app/src"),
            sources,
            BuildMode::Build,
            dependencies,
        );
        assert!(result.build.is_success(), "{}", errors(&result));
        let remote = &result.remotes[0];
        let original = &result.build.artifacts[&remote.source_uri];
        let replacement = &result.client_replacements[&remote.source_uri];
        assert!(original.code().contains("SERVER_ONLY_SECRET"));
        assert!(!replacement.code().contains("SERVER_ONLY_SECRET"));
        assert_eq!(original.module_id, replacement.module_id);
    }

    #[test]
    fn route_component_exports_are_checked_against_runtime_props() {
        for code in [
            "pub fn page() { 42 }",
            "pub component notPage() { <h1>hello</h1> }",
            "pub component page(props: { absent: String }) { <h1>{props.absent}</h1> }",
        ] {
            let result = build(&[("routes/+page.ald", code)]);
            assert!(!result.build.is_success(), "{code}");
            assert!(
                errors(&result).contains("component_contract"),
                "{}",
                errors(&result)
            );
        }
        let result = build(&[(
            "routes/+page.ald",
            "pub component page() { <h1>hello</h1> }",
        )]);
        assert!(result.build.is_success(), "{}", errors(&result));
    }

    #[test]
    fn generated_type_headers_have_no_runtime_imports_or_unused_warnings() {
        let (sources, dependencies) = inputs(&[(
            "routes/+page.ald",
            "pub component page() { <h1>hello</h1> }",
        )]);
        let result = build_sources(
            Path::new("/app/src"),
            sources,
            BuildMode::Build,
            dependencies,
        );
        assert!(result.build.is_success(), "{}", errors(&result));
        assert!(!format!("{:?}", result.build.warnings).contains("unused import"));
        for artifact in result.build.artifacts.values() {
            assert!(!artifact.code().contains("__alder_web_types"));
            assert!(
                artifact
                    .dependencies
                    .iter()
                    .all(|dependency| !dependency.contains("__alder_web_types"))
            );
        }
    }

    #[test]
    fn remote_schema_resolves_shared_generic_recursive_enums() {
        let result = build(&[
            ("types.ald", "pub enum List[a] { Empty, More(a, List[a]) }"),
            (
                "lib/values.remote.ald",
                "import ~/types.{List}\npub fn echo(value: List[Number]) List[Number] { value }",
            ),
            (
                "routes/+page.server.ald",
                "pub fn load(event: LoadEvent) { {} }",
            ),
        ]);
        assert!(result.build.is_success(), "{}", errors(&result));
        let export = &result.remotes[0].exports[0];
        assert_eq!(export.result_schema.nodes.len(), 2);
        let alder_codegen::support::remote::WireNode::Enum(variants) =
            &export.result_schema.nodes[0]
        else {
            panic!("enum schema");
        };
        assert_eq!(variants[1].fields, [("_0".into(), 1), ("_1".into(), 0)]);
        assert_eq!(export.args_validator, "echoArgs");
        assert_eq!(export.result_validator, "echoResult");
        assert!(
            result
                .validation_artifacts
                .contains_key(&result.remotes[0].validator_module_id)
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn generated_remote_validators_execute_through_oxc_bundle_and_v8() {
        for valid in [true, false] {
            let main = if valid {
                "import ~/types.{List}\n#[extern(\"alder://app/lib/values.mjs.__validators\", \"echoArgs\")]\nfn validate(value: Array[List[Number]]) Array[List[Number]]\npub fn main() { validate([List::More(1, List::Empty)]) }"
            } else {
                // This FFI declaration deliberately supplies an untrusted wire
                // shape to the generated validator, as HTTP decoding would.
                "#[extern(\"alder://app/lib/values.mjs.__validators\", \"echoArgs\")]\nfn validate(value: Array[Number]) Array[Number]\npub fn main() { validate([42]) }"
            };
            let (sources, dependencies) = inputs(&[
                ("types.ald", "pub enum List[a] { Empty, More(a, List[a]) }"),
                (
                    "lib/values.remote.ald",
                    "import ~/types.{List}\npub fn echo(value: List[Number]) List[Number] { value }",
                ),
                (
                    "routes/+page.server.ald",
                    "pub fn load(event: LoadEvent) { {} }",
                ),
                ("main.ald", main),
            ]);
            let result = build_sources(
                Path::new("/app/src"),
                sources,
                BuildMode::Build,
                dependencies,
            );
            assert!(result.build.is_success(), "{}", errors(&result));
            let links = alder_codegen::support::web::links(&[alder_codegen::support::web::Link {
                name: "route_2f".into(),
                segments: vec![],
            }]);
            let modules = result
                .build
                .artifacts
                .into_values()
                .chain(result.validation_artifacts.into_values())
                .chain([links]);
            let code = alder_bundle::bundle(
                modules,
                "alder://app/main.mjs",
                alder_bundle::EntryKind::Standalone,
            )
            .await
            .unwrap();
            let executed = alder_runtime::execute(code, vec![]).await;
            if valid {
                assert_eq!(executed.unwrap(), 0);
            } else {
                assert!(
                    executed
                        .unwrap_err()
                        .to_string()
                        .contains("Invalid remote value")
                );
            }
        }
    }

    #[test]
    fn contextual_actions_are_typed_and_import_only_the_generated_stub() {
        let server = "async fn save(input: { name: String }) Result[{ saved: String }, [:invalid]] { Ok({ saved: input.name }) }\npub let actions = { save }";
        for (input, valid) in [
            ("{name:\"Ada\"}", true),
            ("{name:42}", false),
            ("{}", false),
        ] {
            let page = format!(
                "pub async fn submit() {{ actions.save({input}).await }}\npub component page() {{ <h1>hello</h1> }}"
            );
            let (sources, dependencies) = inputs(&[
                ("routes/+page.server.ald", server),
                ("routes/+page.ald", &page),
            ]);
            let result = build_sources(
                Path::new("/app/src"),
                sources,
                BuildMode::Build,
                dependencies,
            );
            assert_eq!(result.build.is_success(), valid, "{}", errors(&result));
            if valid {
                assert_eq!(result.actions.len(), 1);
                let action = &result.actions[0];
                assert!(result.action_artifacts.contains_key(&action.stub_module_id));
                assert!(
                    result
                        .validation_artifacts
                        .contains_key(&action.validator_module_id)
                );
                let page_uri = Url::parse("file:///app/src/routes/+page.ald").unwrap();
                let page = &result.build.artifacts[&page_uri];
                assert!(page.dependencies.contains(&action.stub_module_id));
                assert!(!page.dependencies.contains(&action.server_module_id));
                assert!(!page.code().contains("__alder_web_types"));
            }
        }
    }
    #[test]
    fn page_options_inherit_and_static_entries_generate_encoded_paths() {
        let result = build(&[
            (
                "routes/+layout.server.ald",
                "pub let csr = false\npub let trailingSlash = TrailingSlash::Always",
            ),
            (
                "routes/users/[id]/+page.server.ald",
                "let enabled = true\npub let prerender = enabled\npub let entries = [{ id: \"a b\" }]",
            ),
        ]);
        assert!(result.build.is_success(), "{}", errors(&result));
        let options = &result.page_options["/users/[id]"];
        assert!(options.options.ssr);
        assert!(!options.options.csr);
        assert_eq!(options.prerender_paths, ["/users/a%20b/"]);
        assert_eq!(options.entries.as_ref().unwrap().kind, EntriesKind::Value);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn compiled_async_entries_prerender_through_structured_runtime_sink() {
        use alder_codegen::support::{prerender, remote, web};
        let (sources, dependencies) = inputs(&[
            (
                "routes/[id]/+page.server.ald",
                "pub let prerender = true\npub async fn entries() { [{ id: \"z\" }, { id: \"a b\" }] }\npub fn load(event: LoadEvent) { { message: event.params.id } }",
            ),
            (
                "routes/[id]/+page.ald",
                "pub component page(props: { data: PageData }) { <h1>{props.data.message}</h1> }",
            ),
        ]);
        let result = build_sources(
            Path::new("/app/src"),
            sources,
            BuildMode::Build,
            dependencies,
        );
        assert!(result.build.is_success(), "{}", errors(&result));
        let artifact_id = |path: &str| {
            result.build.artifacts[&Url::parse(&format!("file:///app/src/{path}")).unwrap()]
                .module_id
                .clone()
        };
        let server = artifact_id("routes/[id]/+page.server.ald");
        let segments = vec![("param".into(), "id".into())];
        let application = web::Application {
            routes: vec![web::Route {
                id: "/[id]".into(),
                segments: segments.clone(),
                page: Some(artifact_id("routes/[id]/+page.ald")),
                server: Some(server.clone()),
                endpoint: None,
                layouts: vec![],
                errors: vec![],
                error_layouts: vec![],
                options: vec![server],
            }],
            ..web::Application::default()
        };
        let entries = result.page_options["/[id]"].entries.as_ref().unwrap();
        assert!(entries.is_async);
        let validator = remote::validators(
            "alder:test-entries",
            &[remote::Validation {
                name: "entries".into(),
                arguments: remote::WireSchema {
                    root: 0,
                    nodes: vec![remote::WireNode::Unit],
                },
                result: entries.schema.clone(),
            }],
        );
        let render = prerender::entry(
            &[prerender::Target {
                route: "/[id]".into(),
                segments,
                paths: vec![],
                trailing_slash: "Never".into(),
                entries: Some((
                    entries.esm_id.clone(),
                    true,
                    "alder:test-entries".into(),
                    "entriesResult".into(),
                )),
            }],
            false,
            &[],
        );
        let modules = result.build.artifacts.into_values().chain([
            web::entry(&application, false),
            web::links(&[]),
            validator,
            render,
        ]);
        let code = alder_bundle::bundle_entry(modules, "alder:web-prerender")
            .await
            .unwrap();
        let report = alder_runtime::execute_build(code).await.unwrap();
        let pages: serde_json::Value = serde_json::from_str(&report).unwrap();
        let pages = pages.as_array().unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0]["path"], "/a%20b");
        assert_eq!(pages[1]["path"], "/z");
        let html = pages[0]["html"].as_str().unwrap();
        assert!(
            html.contains("<h1>") && html.contains("a b") && html.contains("</h1>"),
            "{html}"
        );
        assert!(pages[1]["data"].as_str().unwrap().contains('z'));
    }

    #[test]
    fn empty_and_async_entries_have_contextual_params_types() {
        for source in [
            "pub let prerender = true\npub let entries = []",
            "pub let prerender = true\npub async fn entries() { [] }",
            "pub let prerender = true\npub fn entries() { [{ id: \"one\" }] }",
        ] {
            let result = build(&[("routes/users/[id]/+page.server.ald", source)]);
            assert!(result.build.is_success(), "{}", errors(&result));
            assert!(result.page_options["/users/[id]"].entries.is_some());
        }
    }

    #[test]
    fn load_event_fetch_has_the_native_http_task_contract() {
        let result = build(&[(
            "routes/+page.server.ald",
            "pub async fn load(event: LoadEvent) {\nlet response = event.fetch(event.request).await\n{ ok: true }\n}",
        )]);
        assert!(result.build.is_success(), "{}", errors(&result));
    }

    #[test]
    fn error_components_receive_contextual_typed_error_union() {
        let result = build(&[
            (
                "routes/+page.server.ald",
                "pub fn load(event: LoadEvent) Result[{ title: String }, [:missing(String)]] { Err(:missing(\"gone\")) }",
            ),
            (
                "routes/+error.ald",
                "import http.{PageError as ErrorKind}\nfn message(problem: PageError) String { match problem { ErrorKind::Expected(value) => match value { :missing(detail) => detail }, ErrorKind::Unexpected(value) => value.message } }\npub component error(props: { error: PageError }) { <h1>{message(props.error)}</h1> }",
            ),
        ]);
        assert!(result.build.is_success(), "{}", errors(&result));
    }

    #[test]
    fn page_option_contract_errors_are_diagnostics() {
        for source in [
            "pub let prerender = true",
            "pub let ssr = false\npub let csr = false",
            "fn enabled() { true }\npub let ssr = enabled()",
            "pub let entries = [{ bad: \"one\" }]",
            "pub fn entries(arg: Number) { [{ id: \"one\" }] }",
            "pub let prerender = true\npub let entries = [{ id: \"same\" }, { id: \"same\" }]",
        ] {
            let result = build(&[("routes/users/[id]/+page.server.ald", source)]);
            assert!(!result.build.is_success(), "unexpected success: {source}");
            assert!(result.build.artifacts.is_empty());
        }
    }
}
