use std::path::PathBuf;

use futures::StreamExt;
use miette::{IntoDiagnostic, Result, miette};

pub async fn download_version(version: &str, dest_dir: &std::path::Path) -> Result<()> {
    eprintln!("Downloading alder compiler {version}...");

    let target = platform_target()?;
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
        body.extend_from_slice(&chunk);
    }

    std::fs::create_dir_all(dest_dir).into_diagnostic()?;

    if ext == "tar.xz" {
        extract_tar_xz(&body, dest_dir)?;
    } else {
        extract_zip(&body, dest_dir)?;
    }

    eprintln!("Installed alder {version} to {}", dest_dir.display());
    Ok(())
}

fn extract_tar_xz(data: &[u8], dest_dir: &std::path::Path) -> Result<()> {
    let binary_name = if cfg!(windows) { "alder.exe" } else { "alder" };
    let xz = xz2::read::XzDecoder::new(std::io::Cursor::new(data));
    let mut archive = tar::Archive::new(xz);

    for entry in archive.entries().into_diagnostic()? {
        let mut entry = entry.into_diagnostic()?;
        let path = entry.path().into_diagnostic()?.into_owned();

        if path.file_name().and_then(|n| n.to_str()) == Some(binary_name) {
            let dest = dest_dir.join(binary_name);
            let mut out = std::fs::File::create(&dest).into_diagnostic()?;
            std::io::copy(&mut entry, &mut out).into_diagnostic()?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))
                    .into_diagnostic()?;
            }

            return Ok(());
        }
    }

    Err(miette!("Could not find {binary_name} in archive"))
}

fn extract_zip(data: &[u8], dest_dir: &std::path::Path) -> Result<()> {
    let binary_name = if cfg!(windows) { "alder.exe" } else { "alder" };
    let reader = std::io::Cursor::new(data);
    let mut zip = zip::ZipArchive::new(reader).into_diagnostic()?;

    for i in 0..zip.len() {
        let mut file = zip.by_index(i).into_diagnostic()?;
        let path = PathBuf::from(file.name());

        if path.file_name().and_then(|n| n.to_str()) == Some(binary_name) {
            let dest = dest_dir.join(binary_name);
            let mut out = std::fs::File::create(&dest).into_diagnostic()?;
            std::io::copy(&mut file, &mut out).into_diagnostic()?;
            return Ok(());
        }
    }

    Err(miette!("Could not find {binary_name} in archive"))
}

fn platform_target() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        (os, arch) => Err(miette!("Unsupported platform: {os}/{arch}")),
    }
}
