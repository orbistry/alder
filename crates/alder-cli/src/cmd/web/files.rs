//! Public files and transactional publication of compiler-owned web artifacts.
use std::{fs, io::Write, path::Path};

use alder_codegen::support::{prerender::Page, web::Asset};
use miette::{IntoDiagnostic, Result, miette};

use super::Artifacts;

pub(super) fn read_public(root: &Path) -> Result<Vec<Asset>> {
    let public = root.join("public");
    match fs::symlink_metadata(&public) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        result => {
            let kind = result.into_diagnostic()?.file_type();
            if !kind.is_dir() || kind.is_symlink() {
                return Err(miette!("public must be a directory, not a symlink"));
            }
        }
    }
    let mut pending = vec![public.clone()];
    let mut assets = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).into_diagnostic()? {
            let entry = entry.into_diagnostic()?;
            let path = entry.path();
            let kind = entry.file_type().into_diagnostic()?;
            let relative = path.strip_prefix(&public).expect("public descendant");
            let parts = relative
                .iter()
                .map(|part| {
                    part.to_str()
                        .ok_or_else(|| miette!("Public asset paths must be UTF-8"))
                })
                .collect::<Result<Vec<_>>>()?;
            if parts[0] == "_alder" || parts.iter().any(|part| part.contains(['\\', '\0'])) {
                return Err(miette!(
                    "Invalid or reserved public asset path {}",
                    relative.display()
                ));
            }
            if kind.is_symlink() || (!kind.is_file() && !kind.is_dir()) {
                return Err(miette!(
                    "Public assets must be regular files or directories: {}",
                    path.display()
                ));
            }
            if kind.is_dir() {
                pending.push(path);
            } else {
                assets.push(Asset {
                    path: format!("/{}", parts.join("/")),
                    content_type: content_type(&path).to_owned(),
                    bytes: fs::read(&path).into_diagnostic()?,
                });
            }
        }
    }
    assets.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(assets)
}

pub(super) fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json",
        "txt" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "pdf" => "application/pdf",
        "wasm" => "application/wasm",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
}

fn write_new(root: &Path, relative: &Path, bytes: &[u8]) -> Result<()> {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("artifact parent")).into_diagnostic()?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| {
            miette!(
                "Cannot create web artifact {} (public/prerender collision?): {error}",
                relative.display()
            )
        })?;
    file.write_all(bytes).into_diagnostic()?;
    Ok(())
}

fn directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            Err(miette!("Expected a real directory: {}", path.display()))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).into_diagnostic()
        }
        Err(error) => Err(error).into_diagnostic(),
    }
}

pub(super) fn write_output(
    root: &Path,
    server_name: &str,
    artifacts: &Artifacts,
    pages: &[Page],
    config: Option<&str>,
) -> Result<()> {
    let cache = root.join(".alder");
    directory(&cache)?;
    let stage = tempfile::Builder::new()
        .prefix("web-build-")
        .tempdir_in(&cache)
        .into_diagnostic()?;
    if let Some(bundle) = &artifacts.client_output {
        for (name, bytes) in &bundle.files {
            let directory = if name.ends_with(".map") {
                "maps/client"
            } else {
                "client/_alder"
            };
            write_new(stage.path(), &Path::new(directory).join(name), bytes)?;
        }
    } else {
        write_new(
            stage.path(),
            Path::new("client/_alder/client.mjs"),
            artifacts.client.as_bytes(),
        )?;
    }
    if let Some(manifest) = &artifacts.manifest {
        write_new(
            stage.path(),
            Path::new("manifest.json"),
            &serde_json::to_vec_pretty(manifest).into_diagnostic()?,
        )?;
    }
    write_new(
        stage.path(),
        Path::new(server_name),
        artifacts.server.as_bytes(),
    )?;
    for asset in &artifacts.assets {
        write_new(
            stage.path(),
            &Path::new("client").join(asset.path.trim_start_matches('/')),
            &asset.bytes,
        )?;
    }
    for page in pages {
        let relative = page.path.trim_matches('/');
        if !page.path.starts_with('/')
            || page.path.starts_with("//")
            || Path::new(relative)
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
            || relative
                .split('/')
                .any(|part| matches!(part, "." | "..") || part.contains(['\\', '\0']))
            || relative.split('/').next() == Some("_alder")
        {
            return Err(miette!("Invalid prerender artifact path {}", page.path));
        }
        write_new(
            stage.path(),
            &Path::new("client").join(relative).join("index.html"),
            page.html.as_bytes(),
        )?;
        write_new(
            stage.path(),
            &Path::new("client/_alder/data")
                .join(relative)
                .join("index.json"),
            page.data.as_bytes(),
        )?;
    }
    if let Some(config) = config {
        write_new(stage.path(), Path::new("wrangler.jsonc"), config.as_bytes())?;
    }
    publish(root, stage.path())
}

/// Only these generated entries are replaced. Unrelated dist files survive.
/// Each rename is atomic on the same filesystem. A publication error restores
/// the previous entries; generation/validation errors never touch dist.
fn publish(root: &Path, stage: &Path) -> Result<()> {
    const MANAGED: [&str; 6] = [
        "client",
        "server.mjs",
        "worker.mjs",
        "wrangler.jsonc",
        "manifest.json",
        "maps",
    ];
    let dist = root.join("dist");
    directory(&dist)?;
    let backup = tempfile::Builder::new()
        .prefix("web-previous-")
        .tempdir_in(root.join(".alder"))
        .into_diagnostic()?;
    let mut moved = Vec::new();
    let mut installed = Vec::new();
    let result = (|| -> std::io::Result<()> {
        for name in MANAGED {
            match fs::symlink_metadata(dist.join(name)) {
                Ok(_) => {
                    fs::rename(dist.join(name), backup.path().join(name))?;
                    moved.push(name);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        for name in MANAGED {
            if stage.join(name).exists() {
                fs::rename(stage.join(name), dist.join(name))?;
                installed.push(name);
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        let rollback = (|| -> std::io::Result<()> {
            for name in installed.into_iter().rev() {
                fs::rename(dist.join(name), stage.join(name))?;
            }
            for name in moved.into_iter().rev() {
                fs::rename(backup.path().join(name), dist.join(name))?;
            }
            Ok(())
        })();
        if let Err(rollback) = rollback {
            let saved = backup.keep();
            return Err(miette!(
                "Web output publication failed: {error}; rollback failed: {rollback}. Previous artifacts preserved at {}",
                saved.display()
            ));
        }
        return Err(error).into_diagnostic();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifacts() -> Artifacts {
        Artifacts {
            client: "client".into(),
            client_output: None,
            manifest: None,
            server: "server".into(),
            render: None,
            assets: Vec::new(),
        }
    }
    fn page(path: &str) -> Page {
        Page {
            path: path.into(),
            html: "html".into(),
            data: "data".into(),
        }
    }

    #[test]
    fn public_bytes_types_and_reserved_namespace() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("public")).unwrap();
        fs::write(root.path().join("public/test.png"), [0, 255, 128]).unwrap();
        fs::write(root.path().join("public/style.css"), "body {}").unwrap();
        let assets = read_public(root.path()).unwrap();
        assert_eq!(assets[0].content_type, "text/css; charset=utf-8");
        assert_eq!(assets[1].bytes, [0, 255, 128]);
        fs::create_dir(root.path().join("public/_alder")).unwrap();
        assert!(read_public(root.path()).is_err());
    }

    #[test]
    fn split_outputs_publish_all_bytes_and_remove_old_chunks_maps_and_manifest() {
        let root = tempfile::tempdir().unwrap();
        let mut split = artifacts();
        split.client_output = Some(alder_bundle::BundleOutput {
            files: [
                ("entry-old.mjs".into(), b"entry".to_vec()),
                ("chunk-old.mjs".into(), b"chunk".to_vec()),
                ("entry-old.mjs.map".into(), b"map".to_vec()),
                ("nested/data.bin".into(), vec![0, 128, 255]),
            ]
            .into(),
            ..Default::default()
        });
        split.manifest = Some(super::super::manifest::Manifest {
            version: 1,
            build: "old".into(),
            entry: "/_alder/entry-old.mjs".into(),
            files: Default::default(),
            routes: Default::default(),
            source_maps: "hidden-generated-javascript",
        });
        write_output(root.path(), "server.mjs", &split, &[], None).unwrap();
        for (path, expected) in [
            ("client/_alder/entry-old.mjs", b"entry".as_slice()),
            ("client/_alder/chunk-old.mjs", b"chunk".as_slice()),
            ("maps/client/entry-old.mjs.map", b"map".as_slice()),
            ("client/_alder/nested/data.bin", &[0, 128, 255]),
        ] {
            assert_eq!(
                fs::read(root.path().join("dist").join(path)).unwrap(),
                expected
            );
        }
        assert!(
            !root
                .path()
                .join("dist/client/_alder/entry-old.mjs.map")
                .exists()
        );
        assert!(root.path().join("dist/manifest.json").is_file());

        write_output(root.path(), "server.mjs", &artifacts(), &[], None).unwrap();
        for stale in [
            "client/_alder/entry-old.mjs",
            "client/_alder/chunk-old.mjs",
            "client/_alder/nested",
            "maps",
            "manifest.json",
        ] {
            assert!(!root.path().join("dist").join(stale).exists(), "{stale}");
        }
        assert!(root.path().join("dist/client/_alder/client.mjs").is_file());
    }

    #[test]
    fn rebuild_removes_stale_generated_files_but_keeps_unrelated_dist_files() {
        let root = tempfile::tempdir().unwrap();
        let mut old = artifacts();
        old.assets.push(Asset {
            path: "/removed.png".into(),
            content_type: "image/png".into(),
            bytes: vec![0, 255],
        });
        write_output(root.path(), "worker.mjs", &old, &[page("/old")], Some("{}")).unwrap();
        fs::write(root.path().join("dist/keep.txt"), "user").unwrap();
        write_output(
            root.path(),
            "server.mjs",
            &artifacts(),
            &[page("/new")],
            None,
        )
        .unwrap();
        assert!(!root.path().join("dist/client/old").exists());
        assert!(!root.path().join("dist/client/removed.png").exists());
        assert!(!root.path().join("dist/client/_alder/data/old").exists());
        assert!(!root.path().join("dist/worker.mjs").exists());
        assert!(!root.path().join("dist/wrangler.jsonc").exists());
        assert!(root.path().join("dist/client/new/index.html").is_file());
        assert_eq!(
            fs::read_to_string(root.path().join("dist/keep.txt")).unwrap(),
            "user"
        );
    }

    #[test]
    fn invalid_paths_and_asset_collisions_preserve_last_good_build() {
        let root = tempfile::tempdir().unwrap();
        write_output(
            root.path(),
            "server.mjs",
            &artifacts(),
            &[page("/old")],
            None,
        )
        .unwrap();
        for path in ["/../escape", "/_alder", "//absolute", "/a\\b"] {
            assert!(
                write_output(root.path(), "server.mjs", &artifacts(), &[page(path)], None).is_err()
            );
        }
        let mut collision = artifacts();
        collision.assets.push(Asset {
            path: "/old/index.html".into(),
            content_type: "text/html".into(),
            bytes: vec![],
        });
        assert!(
            write_output(root.path(), "server.mjs", &collision, &[page("/old")], None).is_err()
        );
        assert_eq!(
            fs::read_to_string(root.path().join("dist/client/old/index.html")).unwrap(),
            "html"
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_symlinks_are_rejected_without_reading_targets() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("public")).unwrap();
        std::os::unix::fs::symlink("/missing-private-file", root.path().join("public/link"))
            .unwrap();
        assert!(read_public(root.path()).is_err());
    }
}
