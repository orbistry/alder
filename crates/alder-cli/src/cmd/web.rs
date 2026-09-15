//! Application integration shared by build/run/dev. Compiler passes remain in
//! the driver and generated JavaScript remains direct Oxc code generation.

use alder_codegen::support::web::{Action, Application, Link, Remote, Route};
use alder_config::Target;
use alder_driver::web_routes::{Segment, SourceFile};
use miette::{IntoDiagnostic, Result, miette};

use super::build::Compiled;

mod files;
mod manifest;

pub struct Artifacts {
    pub client: String,
    pub client_output: Option<alder_bundle::BundleOutput>,
    pub manifest: Option<manifest::Manifest>,
    pub server: String,
    pub render: Option<String>,
    pub assets: Vec<alder_codegen::support::web::Asset>,
}

/// Compiler-pinned, tested platform behavior. Deployment may explicitly select
/// another date; ordinary builds remain reproducible across calendar days.
pub(super) const COMPATIBILITY_DATE: &str = "2026-09-14";

pub(super) fn cloudflare_config(
    compiled: &Compiled,
    name: Option<String>,
    account: Option<String>,
    compatibility_date: &str,
) -> Result<serde_json::Value> {
    let name = name.unwrap_or_else(|| {
        let basename = compiled
            .root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("alder-app");
        let name: String = basename
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .take(63)
            .collect();
        if name.is_empty() {
            "alder-app".to_owned()
        } else {
            name
        }
    });
    alder_driver::cloudflare::wrangler_config(
        &compiled.cloudflare,
        &alder_driver::cloudflare::ConfigOptions {
            name,
            compatibility_date: compatibility_date.to_owned(),
            main: "worker.mjs".to_owned(),
            assets_directory: "client".to_owned(),
            account_id: account,
            legacy_migrations: None,
        },
    )
    .map_err(|errors| miette!("{}", errors.join("\n")))
}

fn segments(parts: &[Segment]) -> Vec<(String, String)> {
    parts
        .iter()
        .map(|segment| match segment {
            Segment::Static(value) => ("static".into(), value.clone()),
            Segment::Param(value) => ("param".into(), value.clone()),
            Segment::Optional(value) => ("optional".into(), value.clone()),
            Segment::Rest(value) => ("rest".into(), value.clone()),
        })
        .collect()
}

pub(super) async fn bundle(compiled: &Compiled) -> Result<Artifacts> {
    bundle_mode(compiled, false).await
}

pub(super) async fn bundle_mode(compiled: &Compiled, development: bool) -> Result<Artifacts> {
    bundle_with_pages(compiled, development, &[]).await
}

async fn bundle_with_pages(
    compiled: &Compiled,
    development: bool,
    pages: &[alder_codegen::support::prerender::Page],
) -> Result<Artifacts> {
    let assets = files::read_public(&compiled.root)?;
    let manifest = compiled
        .web
        .as_ref()
        .ok_or_else(|| miette!("project has no filesystem routes"))?;
    if manifest.routes.is_empty() {
        return Err(miette!("no web routes found; create src/routes/+page.ald"));
    }
    let module = |source: &SourceFile| -> Result<String> {
        let uri = url::Url::parse(&source.uri).into_diagnostic()?;
        compiled
            .result
            .artifacts
            .get(&uri)
            .map(|artifact| artifact.module_id.clone())
            .ok_or_else(|| miette!("route module was not emitted: {}", source.path.display()))
    };
    let optional = |source: &Option<SourceFile>| source.as_ref().map(module).transpose();
    let mut routes = Vec::new();
    for route in &manifest.routes {
        routes.push(Route {
            id: route.id.clone(),
            segments: segments(&route.segments),
            page: optional(&route.page)?,
            server: optional(&route.page_server)?,
            endpoint: optional(&route.endpoint)?,
            layouts: route
                .layouts
                .iter()
                .map(|layout| Ok((optional(&layout.universal)?, optional(&layout.server)?)))
                .collect::<Result<_>>()?,
            errors: route.errors.iter().map(module).collect::<Result<_>>()?,
            error_layouts: route
                .errors
                .iter()
                .map(|error| {
                    let directory = error.path.parent().expect("route source has a parent");
                    route
                        .layouts
                        .iter()
                        .filter(|layout| {
                            let source = layout
                                .universal
                                .as_ref()
                                .or(layout.server.as_ref())
                                .expect("layout has a source");
                            directory.starts_with(
                                source.path.parent().expect("layout source has a parent"),
                            )
                        })
                        .count()
                })
                .collect(),
            options: route
                .option_sources
                .iter()
                .map(module)
                .collect::<Result<_>>()?,
        });
    }
    let mut application = Application {
        routes,
        prerendered: pages.to_vec(),
        providers: compiled.cloudflare.providers(),
        client_store_keys: compiled.client_store_keys.clone(),
        actions: compiled
            .actions
            .iter()
            .flat_map(|page| {
                page.actions.iter().map(|action| Action {
                    route: page.route_id.clone(),
                    module: page.server_module_id.clone(),
                    validators: page.validator_module_id.clone(),
                    name: action.name.clone(),
                })
            })
            .collect(),
        remotes: compiled
            .remotes
            .iter()
            .flat_map(|module| {
                module.exports.iter().map(|export| Remote {
                    module: module.esm_id.clone(),
                    validators: module.validator_module_id.clone(),
                    name: export.name.clone(),
                    kind: export.kind.as_str().to_owned(),
                })
            })
            .collect(),
        server_hook: optional(&manifest.hooks_server)?,
        client_hook: optional(&manifest.hooks_client)?,
        development,
        client_build: None,
    };
    let links = manifest
        .route_links()
        .iter()
        .map(|link| {
            let route = manifest
                .routes
                .iter()
                .find(|route| route.id == link.route_id)
                .expect("link corresponds to discovered route");
            Link {
                name: link.export_name.clone(),
                segments: segments(&route.segments),
            }
        })
        .collect::<Vec<_>>();
    let mut modules = compiled
        .result
        .artifacts
        .iter()
        .map(|(uri, module)| {
            compiled
                .server_replacements
                .get(uri)
                .unwrap_or(module)
                .clone()
        })
        .collect::<Vec<_>>();
    modules.extend(compiled.server_implementations.values().cloned());
    modules.push(alder_codegen::support::web::links(&links));
    modules.extend(compiled.web_generated.values().cloned());
    let client_entry = alder_codegen::support::web::entry_mode(&application, true, !development);
    let client_output = alder_bundle::bundle_graph(
        compiled
            .result
            .artifacts
            .iter()
            .map(|(uri, module)| {
                compiled
                    .client_replacements
                    .get(uri)
                    .unwrap_or(module)
                    .clone()
            })
            .chain(compiled.web_generated.values().cloned())
            .chain([alder_codegen::support::web::links(&links), client_entry]),
        "alder:web-client",
        if development {
            alder_bundle::BundleOptions::default()
        } else {
            alder_bundle::BundleOptions {
                sourcemap: true,
                ..alder_bundle::BundleOptions::production()
            }
        },
    )
    .await
    .map_err(|error| miette!(error.to_string()))?;
    let (client, client_output, build_manifest) = if development {
        (
            client_output
                .into_single_code("alder:web-client")
                .map_err(|error| miette!(error.to_string()))?,
            None,
            None,
        )
    } else {
        // Resolve the server graph before adding client metadata. Hashing only
        // generated module text misses changes in server-only JS dependencies;
        // hashing the final server would make the manifest identity circular.
        let mut identity_modules = modules.clone();
        // Prerendered documents already contain this identity. They are build
        // outputs, not inputs: including them changes the ID on the second
        // bundling pass and makes navigation mistake its own pages for an update.
        let identity_application = Application {
            prerendered: Vec::new(),
            ..application.clone()
        };
        identity_modules.push(alder_codegen::support::web::entry(
            &identity_application,
            false,
        ));
        if !compiled.cloudflare.adapters.is_empty() {
            identity_modules.push(alder_codegen::support::cloudflare::adapters(
                &compiled.cloudflare.adapters,
                &compiled.cloudflare.providers(),
            ));
        }
        let identity = alder_bundle::bundle_entry(identity_modules, "alder:web-server")
            .await
            .map_err(|error| miette!(error.to_string()))?;
        let build_manifest =
            manifest::Manifest::create(&client_output, &application.routes, identity.as_bytes())?;
        application.client_build = Some(build_manifest.client_build());
        (String::new(), Some(client_output), Some(build_manifest))
    };
    modules.push(alder_codegen::support::web::entry(&application, false));
    if !compiled.cloudflare.adapters.is_empty() {
        modules.push(alder_codegen::support::cloudflare::adapters(
            &compiled.cloudflare.adapters,
            &compiled.cloudflare.providers(),
        ));
    }
    let render = if !development
        && pages.is_empty()
        && compiled
            .page_options
            .values()
            .any(|options| options.options.prerender)
    {
        let mut render_modules = modules.clone();
        let mut targets = Vec::new();
        for (id, options) in &compiled.page_options {
            if !options.options.prerender {
                continue;
            }
            let route = manifest
                .routes
                .iter()
                .find(|route| route.id == *id)
                .expect("resolved route options");
            let entries = if let Some(entry) = &options.entries {
                let validator_module = format!("alder:prerender-validator/{}", targets.len());
                render_modules.push(alder_codegen::support::remote::validators(
                    &validator_module,
                    &[alder_codegen::support::remote::Validation {
                        name: "entries".to_owned(),
                        arguments: alder_codegen::support::remote::WireSchema {
                            root: 0,
                            nodes: vec![alder_codegen::support::remote::WireNode::Unit],
                        },
                        result: entry.schema.clone(),
                    }],
                ));
                Some((
                    entry.esm_id.clone(),
                    entry.kind == alder_driver::web_build::EntriesKind::Function,
                    validator_module,
                    "entriesResult".to_owned(),
                ))
            } else {
                None
            };
            targets.push(alder_codegen::support::prerender::Target {
                route: id.clone(),
                segments: segments(&route.segments),
                paths: options.prerender_paths.clone(),
                trailing_slash: format!("{:?}", options.options.trailing_slash),
                entries,
            });
        }
        render_modules.push(alder_codegen::support::prerender::entry(
            &targets,
            compiled.target == Target::Cloudflare,
            &compiled.cloudflare.adapters,
        ));
        Some(
            alder_bundle::bundle_entry(render_modules, "alder:web-prerender")
                .await
                .map_err(|error| miette!(error.to_string()))?,
        )
    } else {
        None
    };
    let mut embedded_assets = assets.clone();
    if compiled.target == Target::Standalone && !development {
        for (name, bytes) in &client_output.as_ref().expect("production graph").files {
            if !name.ends_with(".map") {
                embedded_assets.push(alder_codegen::support::web::Asset {
                    path: format!("/_alder/{name}"),
                    content_type: files::content_type(std::path::Path::new(name)).into(),
                    bytes: bytes.clone(),
                });
            }
        }
    }
    let (entry, id) = match if development {
        Target::Cloudflare
    } else {
        compiled.target
    } {
        Target::Standalone => (
            alder_codegen::support::web::standalone_entry(&client, &embedded_assets),
            "alder:web-standalone",
        ),
        Target::Cloudflare => (
            alder_codegen::support::web::worker_entry(
                &client,
                &compiled.cloudflare.adapters,
                &assets,
                development,
            ),
            "alder:web-worker",
        ),
    };
    modules.push(entry);
    let server = alder_bundle::bundle_graph(
        modules,
        id,
        alder_bundle::BundleOptions {
            minify: !development,
            ..Default::default()
        },
    )
    .await
    .map_err(|error| miette!(error.to_string()))?
    .into_single_code(id)
    .map_err(|error| miette!(error.to_string()))?;
    Ok(Artifacts {
        client,
        client_output,
        manifest: build_manifest,
        server,
        render,
        assets,
    })
}

pub(super) async fn write_build(
    compiled: &Compiled,
    output: &crate::reporting::Output,
) -> Result<()> {
    output.stage("bundling");
    output.status("Bundling", "web server and browser entries");
    let mut artifacts = bundle(compiled).await?;
    let mut pages = Vec::new();
    if let Some(render) = artifacts.render.take() {
        output.status("Prerendering", "static routes and typed entries");
        let value = match compiled.target {
            Target::Standalone => serde_json::from_str(
                &alder_runtime::execute_build(render)
                    .await
                    .map_err(|error| miette!(error.to_string()))?,
            )
            .into_diagnostic()?,
            Target::Cloudflare => {
                super::platform::render_once(
                    &compiled.root,
                    render,
                    cloudflare_config(compiled, None, None, COMPATIBILITY_DATE)?,
                )
                .await?
            }
        };
        #[derive(serde::Deserialize)]
        struct Rendered {
            path: String,
            html: String,
            data: String,
        }
        let rendered: Vec<Rendered> = serde_json::from_value(value).into_diagnostic()?;
        pages = rendered
            .into_iter()
            .map(|page| alder_codegen::support::prerender::Page {
                path: page.path,
                html: page.html,
                data: page.data,
            })
            .collect();
        artifacts = bundle_with_pages(compiled, false, &pages).await?;
    }
    let dist = compiled.root.join("dist");
    let client = dist.join("client").join(
        artifacts
            .manifest
            .as_ref()
            .map_or("_alder/client.mjs", |manifest| {
                manifest.entry.trim_start_matches('/')
            }),
    );
    let server_name = match compiled.target {
        Target::Standalone => "server.mjs",
        Target::Cloudflare => "worker.mjs",
    };
    let server = dist.join(server_name);
    let config = if compiled.target == Target::Cloudflare {
        Some(
            serde_json::to_string_pretty(&cloudflare_config(
                compiled,
                None,
                None,
                COMPATIBILITY_DATE,
            )?)
            .into_diagnostic()?,
        )
    } else {
        None
    };
    files::write_output(
        &compiled.root,
        server_name,
        &artifacts,
        &pages,
        config.as_deref(),
    )?;
    for page in &pages {
        output.status("Prerendered", &page.path);
    }
    output.status("Built", crate::reporting::display_path(&server));
    output.status("Built", crate::reporting::display_path(&client));
    if let Some(manifest) = &artifacts.manifest {
        for (name, file) in &manifest.files {
            output.status(
                "Bundle",
                format!(
                    "{} {name}: {} bytes · gzip {} · brotli {}",
                    file.kind, file.sizes.raw, file.sizes.gzip, file.sizes.brotli
                ),
            );
        }
    }
    Ok(())
}
