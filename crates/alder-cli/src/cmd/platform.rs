//! Compiler-controlled local runtime using explicitly installed shared tooling.
use std::{path::Path, process::Stdio, time::Duration};

use miette::{IntoDiagnostic, Result, miette};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::mpsc,
};

pub(super) async fn serve_dev(
    mut receiver: mpsc::Receiver<super::dev::Event>,
    root: &Path,
    host: &str,
    port: u16,
    output: &crate::reporting::Output,
) -> Result<()> {
    let support = super::cloudflare::resolve().await?;
    let mut command = Command::new(&support.node);
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
    let support = super::cloudflare::resolve().await?;
    let mut child = Command::new(&support.node)
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
