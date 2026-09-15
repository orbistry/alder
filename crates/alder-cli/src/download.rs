use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};

use futures::StreamExt;
use miette::{IntoDiagnostic, Result, miette};

pub async fn download_version(
    version: &str,
    dest_dir: &std::path::Path,
    output: &crate::reporting::Output,
) -> Result<()> {
    let target = platform_target()?;
    output.status("Downloading", format!("alder {version} ({target})"));
    let tag = format!("alder-cli-v{version}");

    let octocrab = octocrab::instance();

    let release = octocrab
        .repos("orbistry", "alder")
        .releases()
        .get_by_tag(&tag)
        .await
        .into_diagnostic()?;

    let ext = if cfg!(windows) { "zip" } else { "tar.xz" };
    let asset_name = format!("alder-cli-{target}.{ext}");

    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .ok_or_else(|| {
            miette!("No release asset found for platform {target} (looked for {asset_name})")
        })?;

    let mut stream = octocrab
        .repos("orbistry", "alder")
        .release_assets()
        .stream(*asset.id)
        .await
        .into_diagnostic()?;

    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.into_diagnostic()?;
        if body.len().saturating_add(chunk.len()) > 512 * 1024 * 1024 {
            return Err(miette!(
                "Compiler release archive exceeds the download size limit"
            ));
        }
        body.extend_from_slice(&chunk);
    }

    install_archive(version, dest_dir, ext, &body, output)
}

fn install_archive(
    version: &str,
    dest_dir: &std::path::Path,
    ext: &str,
    body: &[u8],
    output: &crate::reporting::Output,
) -> Result<()> {
    let parent = dest_dir
        .parent()
        .ok_or_else(|| miette!("Compiler cache has no parent directory"))?;
    std::fs::create_dir_all(parent).into_diagnostic()?;
    let staging = tempfile::Builder::new()
        .prefix(".alder-install-")
        .tempdir_in(parent)
        .into_diagnostic()?;
    let payload = staging.path().join("payload");
    std::fs::create_dir(&payload).into_diagnostic()?;

    if ext == "tar.xz" {
        extract_tar_xz(body, &payload)?;
    } else {
        extract_zip(body, &payload)?;
    }
    // Publish the whole version directory together. A failed/truncated archive
    // can never leave an executable that the proxy mistakes for a ready cache.
    if dest_dir.join(binary_name()).is_file() {
        return Ok(());
    }
    if dest_dir.exists() {
        // Only remove an empty directory left by an older failed downloader;
        // never overwrite an unknown or partially populated cache directory.
        std::fs::remove_dir(dest_dir).map_err(|err| miette!("Cannot replace incomplete compiler cache {}: {err}. Move this directory aside and retry.", dest_dir.display()))?;
    }
    if let Err(err) = std::fs::rename(&payload, dest_dir)
        && !dest_dir.join(binary_name()).is_file()
    {
        return Err(err).into_diagnostic();
    }

    output.status(
        "Installed",
        format!(
            "alder {version} ({})",
            crate::reporting::display_path(dest_dir)
        ),
    );
    Ok(())
}

fn extract_tar_xz(data: &[u8], dest_dir: &std::path::Path) -> Result<()> {
    let xz = xz2::read::XzDecoder::new(std::io::Cursor::new(data));
    let mut archive = tar::Archive::new(xz);

    let mut layout = ArchiveLayout::default();
    for entry in archive.entries().into_diagnostic()? {
        let mut entry = entry.into_diagnostic()?;
        let path = entry.path().into_diagnostic()?.into_owned();

        let Some(relative) = layout.select(&path)? else {
            continue;
        };
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            std::fs::create_dir_all(dest_dir.join(relative)).into_diagnostic()?;
            continue;
        }
        if !kind.is_file() {
            return Err(miette!(
                "Compiler archive contains a non-regular binary entry: {}",
                path.display()
            ));
        }
        let mode = entry.header().mode().into_diagnostic()?;
        copy_entry(&mut entry, dest_dir, &relative, mode, &mut layout)?;
    }
    layout.finish()
}

fn extract_zip(data: &[u8], dest_dir: &std::path::Path) -> Result<()> {
    let reader = std::io::Cursor::new(data);
    let mut zip = zip::ZipArchive::new(reader).into_diagnostic()?;

    let mut layout = ArchiveLayout::default();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).into_diagnostic()?;
        let path = PathBuf::from(file.name());
        let Some(relative) = layout.select(&path)? else {
            continue;
        };
        if file.is_dir() {
            std::fs::create_dir_all(dest_dir.join(relative)).into_diagnostic()?;
            continue;
        }
        let mode = file.unix_mode().unwrap_or(0o644);
        if mode & 0o170000 != 0 && mode & 0o170000 != 0o100000 {
            return Err(miette!(
                "Compiler archive contains a non-regular binary entry: {}",
                path.display()
            ));
        }
        copy_entry(&mut file, dest_dir, &relative, mode, &mut layout)?;
    }
    layout.finish()
}

fn binary_name() -> &'static str {
    if cfg!(windows) { "alder.exe" } else { "alder" }
}

#[derive(Default)]
struct ArchiveLayout {
    prefix: Option<PathBuf>,
    files: BTreeSet<PathBuf>,
    bytes: u64,
}

impl ArchiveLayout {
    /// cargo-dist tarballs have one enclosing directory; zip and historical
    /// archives may be flat. Only the compiler binary
    /// are installed, never arbitrary files from a release asset.
    fn select(&mut self, path: &Path) -> Result<Option<PathBuf>> {
        // Archive names use forward slashes on every platform. Check the raw
        // name before Windows components consume backslashes as separators.
        if path.as_os_str().to_string_lossy().contains(['\\', ':']) {
            return Err(miette!(
                "Unsafe path in compiler archive: {}",
                path.display()
            ));
        }
        let mut parts = Vec::new();
        for component in path.components() {
            match component {
                Component::Normal(part) if !part.to_string_lossy().contains(['\\', ':']) => {
                    parts.push(part)
                }
                Component::CurDir => {}
                _ => {
                    return Err(miette!(
                        "Unsafe path in compiler archive: {}",
                        path.display()
                    ));
                }
            }
        }
        let index = if parts.first().is_some_and(|part| *part == binary_name()) {
            0
        } else if parts.get(1).is_some_and(|part| *part == binary_name()) {
            1
        } else {
            return Ok(None);
        };
        if parts[index] == binary_name() && parts.len() != index + 1 {
            return Err(miette!("Compiler binary is not a regular archive path"));
        }
        let prefix: PathBuf = parts[..index].iter().collect();
        if self
            .prefix
            .as_ref()
            .is_some_and(|previous| previous != &prefix)
        {
            return Err(miette!("Compiler archive mixes different root directories"));
        }
        self.prefix = Some(prefix);
        Ok(Some(parts[index..].iter().collect()))
    }

    fn finish(self) -> Result<()> {
        if !self.files.contains(Path::new(binary_name())) {
            return Err(miette!("Could not find {} in archive", binary_name()));
        }
        Ok(())
    }
}

fn copy_entry(
    reader: &mut impl std::io::Read,
    dest_dir: &Path,
    relative: &Path,
    mode: u32,
    layout: &mut ArchiveLayout,
) -> Result<()> {
    const MAX_FILES: usize = 100_000;
    const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
    if layout.files.len() >= MAX_FILES || !layout.files.insert(relative.to_owned()) {
        return Err(miette!(
            "Duplicate or excessive entries in compiler archive"
        ));
    }
    let destination = dest_dir.join(relative);
    std::fs::create_dir_all(destination.parent().expect("archive file parent"))
        .into_diagnostic()?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .into_diagnostic()?;
    let copied = std::io::copy(
        &mut std::io::Read::take(reader, MAX_BYTES.saturating_sub(layout.bytes) + 1),
        &mut file,
    )
    .into_diagnostic()?;
    layout.bytes += copied;
    if layout.bytes > MAX_BYTES {
        return Err(miette!(
            "Compiler archive exceeds the extraction size limit"
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if relative == Path::new(binary_name()) {
            0o755
        } else {
            mode & 0o777
        };
        std::fs::set_permissions(destination, std::fs::Permissions::from_mode(mode))
            .into_diagnostic()?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    Ok(())
}

pub(crate) fn platform_target() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        (os, arch) => Err(miette!("Unsupported platform: {os}/{arch}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn files(support: bool) -> Vec<(String, Vec<u8>, u32)> {
        let mut files = vec![(binary_name().to_owned(), b"compiler".to_vec(), 0o755)];
        if support {
            for name in [
                "package.json",
                "cloudflare-dev.mjs",
                "node_modules/miniflare/package.json",
                "node_modules/wrangler/bin/wrangler.js",
                "node_modules/workerd/package.json",
            ] {
                files.push((format!("support/{name}"), b"support data".to_vec(), 0o644));
            }
            files.push((
                "support/node_modules/workerd/bin/workerd".into(),
                b"native runtime".to_vec(),
                0o755,
            ));
            files.push((
                "support/release-support.json".into(),
                serde_json::to_vec(&serde_json::json!({"target": platform_target().unwrap()}))
                    .unwrap(),
                0o644,
            ));
        }
        files.push((
            "README.md".into(),
            b"not installed in cache".to_vec(),
            0o644,
        ));
        files
    }

    fn tar(files: &[(String, Vec<u8>, u32)], root: &str) -> Vec<u8> {
        let xz = xz2::write::XzEncoder::new(Vec::new(), 1);
        let mut archive = tar::Builder::new(xz);
        for (name, data, mode) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(*mode);
            header.set_cksum();
            archive
                .append_data(&mut header, format!("{root}{name}"), Cursor::new(data))
                .unwrap();
        }
        archive.into_inner().unwrap().finish().unwrap()
    }

    fn zip(files: &[(String, Vec<u8>, u32)], root: &str) -> Vec<u8> {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data, mode) in files {
            archive
                .start_file(
                    format!("{root}{name}"),
                    zip::write::SimpleFileOptions::default().unix_permissions(*mode),
                )
                .unwrap();
            archive.write_all(data).unwrap();
        }
        archive.finish().unwrap().into_inner()
    }

    #[test]
    fn archive_installs_only_binary_and_preserves_executable_permissions() {
        for root in ["", "alder-cli-platform/"] {
            for (ext, data) in [
                ("tar.xz", tar(&files(true), root)),
                ("zip", zip(&files(true), root)),
            ] {
                let temp = tempfile::tempdir().unwrap();
                let dest = temp.path().join("version");
                install_archive(
                    "test",
                    &dest,
                    ext,
                    &data,
                    &crate::reporting::Output::default(),
                )
                .unwrap();
                assert_eq!(
                    std::fs::read(dest.join(binary_name())).unwrap(),
                    b"compiler"
                );
                assert!(!dest.join("support").exists());
                assert!(!dest.join("README.md").exists());
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    assert_eq!(
                        std::fs::metadata(dest.join(binary_name()))
                            .unwrap()
                            .permissions()
                            .mode()
                            & 0o777,
                        0o755
                    );
                }
            }
        }
    }

    #[test]
    fn historical_binary_only_archives_remain_installable() {
        for (ext, data) in [
            ("tar.xz", tar(&files(false), "old-release/")),
            ("zip", zip(&files(false), "")),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let dest = temp.path().join("version");
            install_archive(
                "old",
                &dest,
                ext,
                &data,
                &crate::reporting::Output::default(),
            )
            .unwrap();
            assert!(dest.join(binary_name()).is_file());
            assert!(!dest.join("support").exists());
        }
    }

    #[test]
    fn missing_and_duplicate_binaries_are_not_published() {
        let incomplete = vec![("README.md".into(), b"no compiler".to_vec(), 0o644)];
        let mut duplicate = files(false);
        duplicate.push((format!("./{}", binary_name()), b"duplicate".to_vec(), 0o755));
        for files in [incomplete, duplicate] {
            for (ext, data) in [("tar.xz", tar(&files, "")), ("zip", zip(&files, ""))] {
                let temp = tempfile::tempdir().unwrap();
                let dest = temp.path().join("version");
                assert!(
                    install_archive(
                        "bad",
                        &dest,
                        ext,
                        &data,
                        &crate::reporting::Output::default()
                    )
                    .is_err()
                );
                assert!(!dest.exists());
                assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
            }
        }
    }

    #[test]
    fn archive_paths_reject_traversal_absolute_paths_and_mixed_roots() {
        for path in [
            "../alder",
            "/alder",
            "support/../../escape",
            "C:/alder",
            "support\\evil",
        ] {
            assert!(
                ArchiveLayout::default().select(Path::new(path)).is_err(),
                "{path}"
            );
        }
        let mut layout = ArchiveLayout::default();
        layout
            .select(Path::new(&format!("one/{}", binary_name())))
            .unwrap();
        assert!(
            layout
                .select(Path::new(&format!("two/{}", binary_name())))
                .is_err()
        );
    }

    #[test]
    fn binary_symlinks_are_rejected_before_publication() {
        let xz = xz2::write::XzEncoder::new(Vec::new(), 1);
        let mut archive = tar::Builder::new(xz);
        let mut header = tar::Header::new_gnu();
        header.set_size(0);
        header.set_mode(0o777);
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_link_name("../../outside").unwrap();
        header.set_cksum();
        archive
            .append_data(&mut header, binary_name(), std::io::empty())
            .unwrap();
        let data = archive.into_inner().unwrap().finish().unwrap();
        let temp = tempfile::tempdir().unwrap();
        assert!(
            install_archive(
                "bad",
                &temp.path().join("version"),
                "tar.xz",
                &data,
                &crate::reporting::Output::default()
            )
            .is_err()
        );
    }

    #[test]
    fn legacy_support_is_ignored_without_modifying_existing_installations() {
        let mut archive_files = files(true);
        archive_files
            .iter_mut()
            .find(|(name, _, _)| name == "support/release-support.json")
            .unwrap()
            .1 = br#"{"target":"wrong-platform"}"#.to_vec();
        let temporary = tempfile::tempdir().unwrap();
        let destination = temporary.path().join("version");
        install_archive(
            "bad",
            &destination,
            "zip",
            &zip(&archive_files, ""),
            &crate::reporting::Output::default(),
        )
        .unwrap();
        assert!(destination.join(binary_name()).is_file());
        assert!(!destination.join("support").exists());
        std::fs::create_dir(destination.join("support")).unwrap();
        std::fs::write(destination.join("support/legacy"), "keep").unwrap();
        install_archive(
            "same",
            &destination,
            "zip",
            &zip(&files(false), ""),
            &crate::reporting::Output::silent(),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(destination.join("support/legacy")).unwrap(),
            "keep"
        );
    }

    #[test]
    fn installation_preserves_nonempty_incomplete_cache_directories() {
        let temporary = tempfile::tempdir().unwrap();
        let destination = temporary.path().join("version");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("unknown-file"), b"preserve me").unwrap();
        assert!(
            install_archive(
                "test",
                &destination,
                "zip",
                &zip(&files(true), ""),
                &crate::reporting::Output::default()
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read(destination.join("unknown-file")).unwrap(),
            b"preserve me"
        );
        assert!(!destination.join(binary_name()).exists());
    }
}
