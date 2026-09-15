//! Keep tooling leases alive until delegated processes have stopped, including
//! on Ctrl-C. Callers select output handling; interactive login inherits the
//! terminal streams without capturing credentials.
use std::{future::Future, process::ExitStatus, time::Duration};

use miette::{IntoDiagnostic, Result};
use tokio::process::Command;

#[derive(Debug)]
pub struct Cancelled(pub i32);

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cloudflare command cancelled")
    }
}
impl std::error::Error for Cancelled {}
impl miette::Diagnostic for Cancelled {}

pub(super) async fn run(command: &mut Command, installation: bool) -> Result<ExitStatus> {
    // Register before spawning so an immediate interrupt cannot release the
    // parent's installation lease while its child continues running.
    #[cfg(unix)]
    let cancellation = {
        use tokio::signal::unix::{SignalKind, signal};
        let mut interrupt = signal(SignalKind::interrupt()).into_diagnostic()?;
        let mut terminate = signal(SignalKind::terminate()).into_diagnostic()?;
        async move {
            tokio::select! {
                _ = interrupt.recv() => libc::SIGINT,
                _ = terminate.recv() => libc::SIGTERM,
            }
        }
    };
    #[cfg(windows)]
    let cancellation = {
        let mut interrupt = tokio::signal::windows::ctrl_c().into_diagnostic()?;
        let mut terminate = tokio::signal::windows::ctrl_break().into_diagnostic()?;
        async move {
            tokio::select! {
                _ = interrupt.recv() => 2,
                _ = terminate.recv() => 2,
            }
        }
    };
    run_until(command, installation, cancellation).await
}

async fn run_until(
    command: &mut Command,
    installation: bool,
    cancellation: impl Future<Output = i32>,
) -> Result<ExitStatus> {
    #[cfg(unix)]
    if installation {
        // npm ci is noninteractive. Its own process group lets us stop npm
        // shims and descendants without signalling Alder or its caller.
        command.process_group(0);
    }
    if installation {
        command.stdin(std::process::Stdio::null());
    }
    let mut child = command.kill_on_drop(true).spawn().into_diagnostic()?;
    let id = child.id().expect("new child has a process ID");
    let signal = tokio::select! {
        biased;
        signal = cancellation => signal,
        status = child.wait() => return status.into_diagnostic(),
    };
    #[cfg(unix)]
    send_signal(id, installation, signal)?;

    // Foreground Wrangler receives terminal Ctrl-C itself; on Unix also
    // forward parent-only signals to its launcher, which forwards to its CLI.
    // Give it time to close its login listener and persist its normal state.
    let stopped = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
    #[cfg(unix)]
    if installation || stopped.is_err() {
        send_signal(id, installation, libc::SIGKILL)?;
    }
    #[cfg(windows)]
    if stopped.is_err() {
        // npm.cmd can introduce an extra process, so kill the tree rather than
        // only the shell. This path is only reached after cancellation.
        let status = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &id.to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .into_diagnostic()?;
        if !status.success() && child.try_wait().into_diagnostic()?.is_none() {
            child.kill().await.into_diagnostic()?;
        }
    }
    child.wait().await.into_diagnostic()?;
    Err(miette::Report::new(Cancelled(128 + signal)))
}

#[cfg(unix)]
fn send_signal(id: u32, group: bool, signal: i32) -> Result<()> {
    let pid = i32::try_from(id).into_diagnostic()?;
    // SAFETY: a positive child ID (or its explicitly created process group)
    // identifies only the subprocess owned by this invocation; no pointers.
    let result = unsafe { libc::kill(if group { -pid } else { pid }, signal) };
    if result == -1 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            return Err(error).into_diagnostic();
        }
    }
    Ok(())
}
