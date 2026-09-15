use miette::IntoDiagnostic;

fn main() {
    // Exit only after run() drops its runtime and command resources. Keep the
    // full delegated status on Windows instead of truncating it to a u8.
    std::process::exit(run());
}

fn run() -> i32 {
    let output = alder_cli::reporting::Output::stderr(alder_cli::reporting::Options::from_startup(
        std::env::args_os().skip(1),
    ));
    let started = std::time::Instant::now();
    match startup(&output) {
        Ok(status) => status,
        Err(error) => {
            output.diagnostic(&error);
            output.failure("compiler selection", started.elapsed());
            1
        }
    }
}

fn startup(output: &alder_cli::reporting::Output) -> miette::Result<i32> {
    // The guard mutates the environment, which is only sound while the
    // process is single-threaded — so it runs before the runtime starts.
    let proxied = alder_cli::proxy::proxy_guard()?;

    // deno_core and its unsynchronized V8 futures require a current-thread
    // executor. Compiler I/O remains concurrent on that executor.
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .into_diagnostic()?
        .block_on(async {
            if !proxied {
                alder_cli::proxy::maybe_proxy_with(output).await?;
            }

            // Command dispatch already renders diagnostics and its summary.
            Ok(match alder_cli::Cli::default().exec().await {
                Ok(()) => 0,
                Err(error) => command_exit_code(&error),
            })
        })
}

fn command_exit_code(error: &miette::Report) -> i32 {
    if let Some(cancelled) = error.downcast_ref::<alder_cli::cmd::cloudflare_process::Cancelled>() {
        return cancelled.0;
    }
    if let Some(exit) = error.downcast_ref::<alder_cli::cmd::cloudflare::ToolExit>() {
        if let Some(code) = exit.0.code() {
            return code;
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            return 128 + exit.0.signal().unwrap_or(1);
        }
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_status_is_not_collapsed_into_generic_failure() {
        let error = miette::Report::new(alder_cli::cmd::cloudflare_process::Cancelled(130));
        assert_eq!(command_exit_code(&error), 130);
        assert_eq!(command_exit_code(&miette::miette!("other failure")), 1);
    }

    #[cfg(unix)]
    #[test]
    fn delegated_exit_codes_and_signals_are_preserved() {
        use std::os::unix::process::ExitStatusExt;
        for (raw, expected) in [(37 << 8, 37), (15, 143)] {
            let error = miette::Report::new(alder_cli::cmd::cloudflare::ToolExit(
                std::process::ExitStatus::from_raw(raw),
            ));
            assert_eq!(command_exit_code(&error), expected);
        }
    }

    #[cfg(windows)]
    #[test]
    fn delegated_windows_status_is_not_truncated() {
        use std::os::windows::process::ExitStatusExt;
        let raw = 0xc000013a;
        let error = miette::Report::new(alder_cli::cmd::cloudflare::ToolExit(
            std::process::ExitStatus::from_raw(raw),
        ));
        assert_eq!(command_exit_code(&error), raw as i32);
    }
}
