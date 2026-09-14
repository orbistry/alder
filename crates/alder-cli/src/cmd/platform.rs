//! Compiler-shipped Node support. Project code never installs or resolves its
//! own platform toolchain; the pinned compiler support directory owns it.
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use miette::{IntoDiagnostic, Result, miette};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::mpsc,
};

pub(super) fn support_directory() -> Result<PathBuf> {
    let executable = std::env::current_exe().ok();
    let override_dir = std::env::var_os("ALDER_SUPPORT_DIR").map(PathBuf::from);
    let candidates = support_candidates(
        override_dir.as_deref(),
        executable.as_deref(),
        crate::VERSION,
        crate::download::platform_target().ok(),
        Path::new(env!("CARGO_MANIFEST_DIR")),
    );
    candidates.into_iter().find(|path| path.join("package.json").is_file() && path.join("node_modules/miniflare/package.json").is_file())
        .ok_or_else(|| miette!("Alder Cloudflare support files are missing. Reinstall this compiler with its support files, or set ALDER_SUPPORT_DIR. Node.js >=22 is required. For a source checkout, run npm ci --prefix crates/alder-cli/support."))
}

fn support_candidates(
    override_dir: Option<&Path>,
    executable: Option<&Path>,
    version: &str,
    target: Option<&str>,
    source_manifest_dir: &Path,
) -> Vec<PathBuf> {
    let mut candidates = override_dir
        .map(Path::to_owned)
        .into_iter()
        .collect::<Vec<_>>();
    if let Some(parent) = executable.and_then(Path::parent) {
        if let Some(target) = target {
            candidates.push(parent.join(".alder-support").join(version).join(target));
        }
        candidates.push(parent.join("support"));
        if let Some(prefix) = parent.parent() {
            // cargo-dist's Homebrew formula installs nonbinary files in
            // pkgshare, including support. current_exe resolves the Cellar bin.
            candidates.push(prefix.join("share/alder/support"));
        }
    }
    candidates.push(source_manifest_dir.join("support"));
    candidates
}

pub(super) async fn serve_dev(
    mut receiver: mpsc::Receiver<super::dev::Event>,
    root: &Path,
    host: &str,
    port: u16,
    output: &crate::reporting::Output,
) -> Result<()> {
    let support = support_directory()?;
    let mut command = Command::new("node");
    command
        .arg(support.join("cloudflare-dev.mjs"))
        .args([
            "--root",
            &root.to_string_lossy(),
            "--host",
            host,
            "--port",
            &port.to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    // Only Alder handles terminal Ctrl-C. The bridge and workerd receive the
    // orderly protocol shutdown instead of racing the whole foreground group.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    let mut child = command.spawn().into_diagnostic()?;
    let mut stdin = child.stdin.take().expect("piped bridge input");
    let mut lines = BufReader::new(child.stdout.take().expect("piped bridge output")).lines();
    loop {
        tokio::select! {
            event = receiver.recv() => {
                let event = event.unwrap_or(super::dev::Event::Shutdown);
                let shutdown = matches!(event, super::dev::Event::Shutdown);
                let mut line = serde_json::to_vec(&event).into_diagnostic()?;
                line.push(b'\n');
                stdin.write_all(&line).await.into_diagnostic()?;
                stdin.flush().await.into_diagnostic()?;
                if shutdown {break;}
            }
            line = lines.next_line() => {
                let Some(line) = line.into_diagnostic()? else {break;};
                let message: serde_json::Value = serde_json::from_str(&line).into_diagnostic()?;
                match message["type"].as_str() {
                    Some("listening") => output.status("Alder dev", message["url"].as_str().unwrap_or("Cloudflare local runtime")),
                    Some("ready") => output.detail("Miniflare", "application updated"),
                    Some("error") => output.status("Miniflare error", message["message"].as_str().unwrap_or("Unknown platform error")),
                    _ => return Err(miette!("Unexpected Cloudflare support message")),
                }
            }
            result = child.wait() => {
                let status = result.into_diagnostic()?;
                return Err(miette!("Cloudflare development runtime exited unexpectedly ({status})"));
            }
        }
    }
    drop(stdin);
    let status = match tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
        Ok(status) => status.into_diagnostic()?,
        Err(_) => {
            child.kill().await.into_diagnostic()?;
            return Err(miette!(
                "Cloudflare runtime did not finish its shutdown within 10 seconds"
            ));
        }
    };
    if !status.success() {
        return Err(miette!(
            "Cloudflare development runtime exited with {status}"
        ));
    }
    Ok(())
}

/// Evaluate the compiler's build-only Worker in the same local platform used by
/// development. No authentication, upload, or remote bindings are involved.
pub(super) async fn render_once(
    root: &Path,
    server: String,
    config: serde_json::Value,
) -> Result<serde_json::Value> {
    let support = support_directory()?;
    let mut child = Command::new("node")
        .arg(support.join("cloudflare-dev.mjs"))
        .args([
            "--root",
            &root.to_string_lossy(),
            "--host",
            "127.0.0.1",
            "--port",
            "0",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .into_diagnostic()?;
    let mut stdin = child.stdin.take().expect("piped render input");
    let mut lines = BufReader::new(child.stdout.take().expect("piped render output")).lines();
    let mut command =
        serde_json::to_vec(&serde_json::json!({"type":"render", "server":server, "config":config}))
            .into_diagnostic()?;
    command.push(b'\n');
    stdin.write_all(&command).await.into_diagnostic()?;
    stdin.flush().await.into_diagnostic()?;
    let result = loop {
        let Some(line) = lines.next_line().await.into_diagnostic()? else {
            break Err(miette!("Cloudflare prerender host exited without a result"));
        };
        let mut message: serde_json::Value = serde_json::from_str(&line).into_diagnostic()?;
        match message["type"].as_str() {
            Some("rendered") => break Ok(message["result"].take()),
            Some("error") => {
                break Err(miette!(
                    "Cloudflare prerender failed: {}",
                    message["message"].as_str().unwrap_or("unknown error")
                ));
            }
            Some("ready" | "listening") => {}
            _ => break Err(miette!("Unexpected Cloudflare prerender message")),
        }
    };
    let _ = stdin.write_all(b"{\"type\":\"shutdown\"}\n").await;
    drop(stdin);
    let status = match tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
        Ok(status) => status.into_diagnostic()?,
        Err(_) => {
            child.kill().await.into_diagnostic()?;
            return Err(miette!("Cloudflare prerender host did not shut down"));
        }
    };
    if !status.success() {
        return Err(miette!("Cloudflare prerender host exited with {status}"));
    }
    result
}

#[cfg(test)]
mod support_path_tests {
    use super::*;

    #[test]
    fn installer_archive_homebrew_and_source_paths_are_version_aware() {
        let paths = support_candidates(
            Some(Path::new("/override")),
            Some(Path::new("/cellar/alder/1.2.3/bin/alder")),
            "1.2.3",
            Some("native-target"),
            Path::new("/checkout/crates/alder-cli"),
        );
        assert_eq!(
            paths,
            [
                "/override",
                "/cellar/alder/1.2.3/bin/.alder-support/1.2.3/native-target",
                "/cellar/alder/1.2.3/bin/support",
                "/cellar/alder/1.2.3/share/alder/support",
                "/checkout/crates/alder-cli/support",
            ]
            .map(PathBuf::from)
        );
    }
}
