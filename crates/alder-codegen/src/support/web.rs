//! Isomorphic application entries. Route discovery supplies owned descriptors;
//! all imports and configuration are emitted directly as Oxc nodes.

use std::collections::{BTreeMap, BTreeSet};

use oxc_ast::ast::VariableDeclarationKind;

use crate::EmittedModule;

#[derive(Clone, Debug)]
pub struct Route {
    pub id: String,
    /// Each pair is (static/param/optional/rest, literal or parameter name).
    pub segments: Vec<(String, String)>,
    pub page: Option<String>,
    pub server: Option<String>,
    pub endpoint: Option<String>,
    pub layouts: Vec<(Option<String>, Option<String>)>,
    pub errors: Vec<String>,
    pub error_layouts: Vec<usize>,
    pub options: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Application {
    pub routes: Vec<Route>,
    pub remotes: Vec<Remote>,
    pub actions: Vec<Action>,
    pub providers: Vec<super::cloudflare::ProviderBinding>,
    pub client_store_keys: Vec<String>,
    pub prerendered: Vec<super::prerender::Page>,
    pub server_hook: Option<String>,
    pub client_hook: Option<String>,
    pub development: bool,
}

#[derive(Clone, Debug)]
pub struct Remote {
    pub module: String,
    pub validators: String,
    pub name: String,
    pub kind: String,
}

#[derive(Clone, Debug)]
pub struct Link {
    pub name: String,
    pub segments: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub struct Action {
    pub route: String,
    pub module: String,
    pub validators: String,
    pub name: String,
}

/// Implementation of the generated, normally type-checked `~/routes` facade.
pub fn links(links: &[Link]) -> EmittedModule {
    super::generated_module("__alder:web/routes", |js| {
        let mut body = js.vec();
        body.push(js.import(
            "alder:kernel",
            &[("$webHref".to_owned(), "$webHref".to_owned())],
        ));
        for link in links {
            let segments = js.array(
                link.segments
                    .iter()
                    .map(|(kind, name)| js.array([js.string(kind), js.string(name)])),
            );
            let call = js.call(
                js.identifier("$webHref"),
                [segments, js.identifier("params")],
            );
            body.push(js.function(
                &link.name,
                &["params".to_owned()],
                js.builder.vec1(js.return_statement(call)),
                false,
            ));
        }
        body.push(
            js.export(
                &links
                    .iter()
                    .map(|link| (link.name.clone(), link.name.clone()))
                    .collect::<Vec<_>>(),
            ),
        );
        js.program(body)
    })
}

#[derive(Clone, Debug)]
pub struct Asset {
    pub path: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

fn asset_values<'a>(
    js: &crate::js_ast::JsAst<'a>,
    assets: &[Asset],
) -> oxc_ast::ast::Expression<'a> {
    js.array(assets.iter().map(|asset| {
        js.array([
            js.string(&asset.path),
            js.string(&asset.content_type),
            js.array(asset.bytes.iter().map(|byte| js.number(f64::from(*byte)))),
        ])
    }))
}

pub fn standalone_entry(client: &str, assets: &[Asset]) -> EmittedModule {
    super::generated_module("alder:web-standalone", |js| {
        let mut body = js.vec();
        body.push(js.namespace_import("alder:web-server", "$server"));
        body.push(js.import(
            "alder:kernel",
            &[("$webServe".to_owned(), "$webServe".to_owned())],
        ));
        body.push(js.expression_statement(js.await_expression(js.call(
            js.identifier("$webServe"),
            [
                js.member(js.identifier("$server"), "default"),
                js.string(client),
                asset_values(js, assets),
            ],
        ))));
        js.program(body)
    })
}

pub fn worker_entry(
    client: &str,
    adapters: &[super::cloudflare::Adapter],
    assets: &[Asset],
    development: bool,
) -> EmittedModule {
    super::generated_module("alder:web-worker", |js| {
        let mut body = js.vec();
        body.push(js.namespace_import("alder:web-server", "$server"));
        let mut exports = Vec::new();
        let mut queue = None;
        if !adapters.is_empty() {
            body.push(js.namespace_import("alder:cloudflare-adapters", "$adapters"));
            for (index, adapter) in adapters.iter().enumerate() {
                if adapter.kind == super::cloudflare::AdapterKind::Queue {
                    queue = Some(js.member(js.identifier("$adapters"), "queue"));
                } else {
                    let local = format!("$adapter{index}");
                    body.push(js.variable(
                        VariableDeclarationKind::Const,
                        &local,
                        Some(js.member(js.identifier("$adapters"), &adapter.name)),
                    ));
                    exports.push((local, adapter.name.clone()));
                }
            }
        }
        body.push(js.import(
            "alder:kernel",
            &[("$webWorker".to_owned(), "$webWorker".to_owned())],
        ));
        body.push(js.export_default(js.call(
            js.identifier("$webWorker"),
            [
                js.member(js.identifier("$server"), "default"),
                js.string(client),
                queue.unwrap_or_else(|| js.undefined()),
                asset_values(js, assets),
                js.boolean(!development),
            ],
        )));
        if !exports.is_empty() {
            body.push(js.export(&exports));
        }
        js.program(body)
    })
}

/// Browser entries never import server companions, endpoints, or server hooks.
/// The driver separately verifies the transitive client dependency boundary.
pub fn entry(application: &Application, browser: bool) -> EmittedModule {
    super::generated_module(
        if browser {
            "alder:web-client"
        } else {
            "alder:web-server"
        },
        |js| {
            let mut body = js.vec();
            let mut modules = BTreeSet::new();
            for route in &application.routes {
                modules.extend(route.page.iter().cloned());
                modules.extend(route.errors.iter().cloned());
                for (layout, server) in &route.layouts {
                    modules.extend(layout.iter().cloned());
                    if !browser {
                        modules.extend(server.iter().cloned());
                    }
                }
                if !browser {
                    modules.extend(route.server.iter().cloned());
                    modules.extend(route.endpoint.iter().cloned());
                    modules.extend(route.options.iter().cloned());
                }
            }
            let hook = if browser {
                &application.client_hook
            } else {
                &application.server_hook
            };
            modules.extend(hook.iter().cloned());
            if !browser {
                for remote in &application.remotes {
                    modules.insert(remote.module.clone());
                    modules.insert(remote.validators.clone());
                }
                for action in &application.actions {
                    modules.insert(action.module.clone());
                    modules.insert(action.validators.clone());
                }
            }
            let aliases: BTreeMap<_, _> = modules
                .into_iter()
                .enumerate()
                .map(|(index, module)| (module, format!("$module{index}")))
                .collect();
            for (module, alias) in &aliases {
                body.push(js.namespace_import(module, alias));
            }
            let reference = |module: &Option<String>| {
                module
                    .as_ref()
                    .map_or_else(|| js.undefined(), |module| js.identifier(&aliases[module]))
            };
            let routes =
                application.routes.iter().map(|route| {
                    let mut properties = js.vec();
                    properties.push(js.property("id", js.string(&route.id)));
                    properties.push(
                        js.property(
                            "segments",
                            js.array(route.segments.iter().map(|(kind, value)| {
                                js.array([js.string(kind), js.string(value)])
                            })),
                        ),
                    );
                    properties.push(js.property("page", reference(&route.page)));
                    properties.push(js.property(
                        "layouts",
                        js.array(route.layouts.iter().map(|(layout, server)| {
                            js.array([
                                reference(layout),
                                if browser {
                                    js.undefined()
                                } else {
                                    reference(server)
                                },
                            ])
                        })),
                    ));
                    properties.push(
                        js.property(
                            "errors",
                            js.array(
                                route
                                    .errors
                                    .iter()
                                    .map(|module| js.identifier(&aliases[module])),
                            ),
                        ),
                    );
                    properties.push(
                        js.property(
                            "errorLayouts",
                            js.array(
                                route
                                    .error_layouts
                                    .iter()
                                    .map(|count| js.number(*count as f64)),
                            ),
                        ),
                    );
                    if !browser {
                        properties.push(js.property("server", reference(&route.server)));
                        properties.push(js.property("endpoint", reference(&route.endpoint)));
                        properties.push(
                            js.property(
                                "options",
                                js.array(
                                    route
                                        .options
                                        .iter()
                                        .map(|module| js.identifier(&aliases[module])),
                                ),
                            ),
                        );
                    }
                    js.object(properties)
                });
            let mut config = js.vec();
            config.push(js.property("routes", js.array(routes)));
            config.push(js.property("hook", reference(hook)));
            config.push(js.property("development", js.boolean(application.development)));
            if !browser {
                config.push(
                    js.property(
                        "clientStoreKeys",
                        js.array(
                            application
                                .client_store_keys
                                .iter()
                                .map(|key| js.string(key)),
                        ),
                    ),
                );
                config.push(js.property(
                    "prerendered",
                    js.array(application.prerendered.iter().map(|page| {
                        js.object(js.builder.vec_from_iter([
                            js.property("path", js.string(&page.path)),
                            js.property("html", js.string(&page.html)),
                            js.property("data", js.string(&page.data)),
                        ]))
                    })),
                ));
                config.push(js.property(
                    "actions",
                    js.array(application.actions.iter().map(|action| {
                        js.object(js.builder.vec_from_iter([
                            js.property("route", js.string(&action.route)),
                            js.property("name", js.string(&action.name)),
                            js.property(
                                "call",
                                js.member(
                                    js.member(js.identifier(&aliases[&action.module]), "actions"),
                                    &action.name,
                                ),
                            ),
                            js.property(
                                "args",
                                js.member(
                                    js.identifier(&aliases[&action.validators]),
                                    &format!("{}Args", action.name),
                                ),
                            ),
                            js.property(
                                "result",
                                js.member(
                                    js.identifier(&aliases[&action.validators]),
                                    &format!("{}Result", action.name),
                                ),
                            ),
                        ]))
                    })),
                ));
                config.push(
                    js.property(
                        "providers",
                        js.array(
                            application.providers.iter().map(|(key, binding)| {
                                js.array([js.string(key), js.string(binding)])
                            }),
                        ),
                    ),
                );
                config.push(js.property(
                    "remotes",
                    js.array(application.remotes.iter().map(|remote| {
                        js.object(js.builder.vec_from_iter([
                            js.property("module", js.string(&remote.module)),
                            js.property("name", js.string(&remote.name)),
                            js.property("kind", js.string(&remote.kind)),
                            js.property(
                                "call",
                                js.member(js.identifier(&aliases[&remote.module]), &remote.name),
                            ),
                            js.property(
                                "args",
                                js.member(
                                    js.identifier(&aliases[&remote.validators]),
                                    &format!("{}Args", remote.name),
                                ),
                            ),
                            js.property(
                                "result",
                                js.member(
                                    js.identifier(&aliases[&remote.validators]),
                                    &format!("{}Result", remote.name),
                                ),
                            ),
                        ]))
                    })),
                ));
            }
            let runtime = if browser {
                "$webStartClient"
            } else {
                "$webApplication"
            };
            body.push(js.import("alder:kernel", &[(runtime.to_owned(), runtime.to_owned())]));
            body.push(js.variable(
                VariableDeclarationKind::Const,
                "$application",
                Some(js.call(js.identifier(runtime), [js.object(config)])),
            ));
            body.push(js.export_default(js.identifier("$application")));
            js.program(body)
        },
    )
}
