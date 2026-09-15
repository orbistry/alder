//! Deterministic browser graph metadata. This does not parse or rewrite chunks.
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
};

use alder_bundle::BundleOutput;
use alder_codegen::support::web::{ClientBuild, Route, RoutePreloads};
use miette::{IntoDiagnostic, Result, miette};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize)]
pub struct Sizes {
    pub raw: usize,
    pub gzip: usize,
    pub brotli: usize,
}

#[derive(Debug, Serialize)]
pub struct File {
    pub kind: &'static str,
    pub imports: Vec<String>,
    pub dynamic_imports: Vec<String>,
    pub modules: Vec<String>,
    pub sizes: Sizes,
    pub map: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RouteFiles {
    pub normal: Vec<String>,
    pub errors: Vec<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct Manifest {
    pub version: u8,
    pub build: String,
    pub entry: String,
    pub files: BTreeMap<String, File>,
    pub routes: BTreeMap<String, RouteFiles>,
    pub source_maps: &'static str,
}

fn url(file: &str) -> String {
    format!("/_alder/{file}")
}

fn closure(output: &BundleOutput, roots: impl IntoIterator<Item = String>) -> Result<Vec<String>> {
    let mut pending: Vec<_> = roots.into_iter().collect();
    let mut seen = BTreeSet::new();
    while let Some(file) = pending.pop() {
        if !seen.insert(file.clone()) {
            continue;
        }
        let chunk = output
            .chunks
            .get(&file)
            .ok_or_else(|| miette!("browser chunk reference is not emitted: {file}"))?;
        pending.extend(chunk.imports.iter().cloned());
    }
    Ok(seen.into_iter().map(|file| url(&file)).collect())
}

fn module_chunk(output: &BundleOutput, module: &str) -> Result<String> {
    let chunk = output
        .chunks
        .values()
        .find(|chunk| chunk.facade_module_id.as_deref() == Some(module))
        .or_else(|| {
            output
                .chunks
                .values()
                .find(|chunk| chunk.module_ids.iter().any(|id| id == module))
        })
        .ok_or_else(|| miette!("browser module has no emitted chunk: {module}"))?;
    Ok(chunk.filename.clone())
}

impl Manifest {
    pub fn create(output: &BundleOutput, routes: &[Route], server_identity: &[u8]) -> Result<Self> {
        let entry = output
            .entry("alder:web-client")
            .map_err(|error| miette!(error.to_string()))?
            .filename
            .clone();
        let mut identity = Sha256::new();
        identity.update(server_identity);
        let mut files = BTreeMap::new();
        for (name, bytes) in &output.files {
            identity.update((name.len() as u64).to_le_bytes());
            identity.update(name.as_bytes());
            identity.update((bytes.len() as u64).to_le_bytes());
            identity.update(bytes);
            if name.ends_with(".map") {
                continue;
            }
            let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
            gzip.write_all(bytes).into_diagnostic()?;
            let gzip = gzip.finish().into_diagnostic()?.len();
            let mut compressed = Vec::new();
            {
                let mut writer = brotli::CompressorWriter::new(&mut compressed, 4096, 11, 22);
                writer.write_all(bytes).into_diagnostic()?;
            }
            let chunk = output.chunks.get(name);
            files.insert(
                url(name),
                File {
                    kind: chunk.map_or("asset", |chunk| {
                        if chunk.is_entry {
                            "entry"
                        } else if chunk.is_dynamic_entry {
                            "route"
                        } else {
                            "shared"
                        }
                    }),
                    imports: chunk.map_or_else(Vec::new, |chunk| {
                        chunk.imports.iter().map(|file| url(file)).collect()
                    }),
                    dynamic_imports: chunk.map_or_else(Vec::new, |chunk| {
                        chunk.dynamic_imports.iter().map(|file| url(file)).collect()
                    }),
                    modules: chunk.map_or_else(Vec::new, |chunk| chunk.module_ids.clone()),
                    sizes: Sizes {
                        raw: bytes.len(),
                        gzip,
                        brotli: compressed.len(),
                    },
                    map: chunk.and_then(|chunk| chunk.sourcemap_filename.clone()),
                },
            );
        }
        let mut route_files = BTreeMap::new();
        for route in routes.iter().filter(|route| route.page.is_some()) {
            let preload = |boundary: Option<usize>| -> Result<Vec<String>> {
                let count = boundary.map_or(route.layouts.len(), |index| {
                    route
                        .error_layouts
                        .get(index)
                        .copied()
                        .unwrap_or(route.layouts.len())
                });
                let mut roots = vec![entry.clone()];
                for (layout, _) in route.layouts.iter().take(count) {
                    if let Some(module) = layout {
                        roots.push(module_chunk(output, module)?);
                    }
                }
                if let Some(index) = boundary {
                    roots.push(module_chunk(output, &route.errors[index])?);
                } else if let Some(module) = &route.page {
                    roots.push(module_chunk(output, module)?);
                }
                closure(output, roots)
            };
            route_files.insert(
                route.id.clone(),
                RouteFiles {
                    normal: preload(None)?,
                    errors: (0..route.errors.len())
                        .map(|index| preload(Some(index)))
                        .collect::<Result<_>>()?,
                },
            );
        }
        Ok(Self {
            version: 1,
            build: format!("{:x}", identity.finalize()),
            entry: url(&entry),
            files,
            routes: route_files,
            source_maps: "hidden-generated-javascript",
        })
    }

    pub fn client_build(&self) -> ClientBuild {
        ClientBuild {
            entry: self.entry.clone(),
            build: self.build.clone(),
            files: self.files.keys().cloned().collect(),
            routes: self
                .routes
                .iter()
                .map(|(id, route)| {
                    (
                        id.clone(),
                        RoutePreloads {
                            normal: route.normal.clone(),
                            errors: route.errors.clone(),
                        },
                    )
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test(flavor = "current_thread")]
    async fn server_only_javascript_dependency_changes_build_identity() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path();
        std::fs::create_dir_all(path.join("src/routes")).unwrap();
        std::fs::write(
            path.join("alder.jsonc"),
            r#"{"type":"application","target":"standalone"}"#,
        )
        .unwrap();
        std::fs::write(
            path.join("src/routes/+page.ald"),
            "pub component page() { <main>Initial</main> }\n",
        )
        .unwrap();
        std::fs::write(
            path.join("src/routes/+page.server.ald"),
            r#"
#[extern("./message.js", "message")]
fn message() String
pub fn load(event: LoadEvent) Result[{ message: String }, [:missing(String)]] {
    Ok({ message: message() })
}
"#,
        )
        .unwrap();
        let output = crate::reporting::Output::silent();
        let mut builds = Vec::new();
        for value in ["first", "second"] {
            std::fs::write(
                path.join("src/routes/message.js"),
                format!("export function message() {{ return '{value}'; }}"),
            )
            .unwrap();
            let compiled = super::super::super::build::compile(
                &path.to_path_buf(),
                alder_driver::BuildMode::Build,
                &output,
            )
            .await
            .unwrap();
            builds.push(super::super::bundle(&compiled).await.unwrap());
        }
        assert_ne!(builds[0].server, builds[1].server);
        assert_ne!(
            builds[0].manifest.as_ref().unwrap().build,
            builds[1].manifest.as_ref().unwrap().build
        );
        assert_eq!(builds[0].client_output, builds[1].client_output);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn real_compiler_build_links_lazy_routes_preloads_maps_and_prerender_output() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path();
        std::fs::create_dir_all(path.join("src/routes/other")).unwrap();
        std::fs::write(
            path.join("alder.jsonc"),
            r#"{"type":"application","target":"standalone"}"#,
        )
        .unwrap();
        std::fs::write(
            path.join("src/routes/+page.ald"),
            "pub let prerender = true\npub component page() { <main>Initial page</main> }\n",
        )
        .unwrap();
        std::fs::write(
            path.join("src/routes/other/+page.ald"),
            "pub component page() { <main>Other route</main> }\n",
        )
        .unwrap();
        let output = crate::reporting::Output::silent();
        let compiled = super::super::super::build::compile(
            &path.to_path_buf(),
            alder_driver::BuildMode::Build,
            &output,
        )
        .await
        .unwrap();
        let artifacts = super::super::bundle(&compiled).await.unwrap();
        let manifest = artifacts.manifest.as_ref().unwrap();
        let bundle = artifacts.client_output.as_ref().unwrap();
        assert!(manifest.files[&manifest.entry].dynamic_imports.len() >= 2);
        let initial = &manifest.routes["/"].normal;
        let other = &manifest.routes["/other"].normal;
        assert!(other.iter().any(|file| !initial.contains(file)));
        for file in manifest.files.values() {
            for reference in file.imports.iter().chain(&file.dynamic_imports) {
                assert!(manifest.files.contains_key(reference), "{reference}");
            }
        }
        assert!(bundle.files.keys().any(|file| file.ends_with(".map")));
        let repeated = super::super::bundle(&compiled).await.unwrap();
        assert_eq!(
            serde_json::to_string(manifest).unwrap(),
            serde_json::to_string(repeated.manifest.as_ref().unwrap()).unwrap()
        );
        super::super::write_build(&compiled, &output).await.unwrap();
        let html = std::fs::read_to_string(path.join("dist/client/index.html")).unwrap();
        assert!(html.contains(&manifest.entry));
        assert!(!html.contains("/_alder/client.mjs"));
        for file in initial {
            assert!(html.contains(file));
        }
        for file in other.iter().filter(|file| !initial.contains(file)) {
            assert!(!html.contains(file));
        }
        for (file, info) in &manifest.files {
            assert!(
                path.join("dist/client")
                    .join(file.trim_start_matches('/'))
                    .is_file()
            );
            if let Some(map) = &info.map {
                assert!(path.join("dist/maps/client").join(map).is_file());
                assert!(!path.join("dist/client/_alder").join(map).exists());
            }
        }
        std::fs::write(
            path.join("src/routes/other/+page.ald"),
            "pub component page() { <main>Changed other route</main> }\n",
        )
        .unwrap();
        let changed = super::super::super::build::compile(
            &path.to_path_buf(),
            alder_driver::BuildMode::Build,
            &output,
        )
        .await
        .unwrap();
        let changed = super::super::bundle(&changed).await.unwrap();
        let changed = changed.manifest.unwrap();
        let independent = initial
            .iter()
            .find(|file| manifest.files[*file].kind == "route")
            .unwrap();
        assert!(changed.files.contains_key(independent));
        assert_ne!(manifest.build, changed.build);
    }
}
