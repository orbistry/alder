use miette::IntoDiagnostic;

fn main() -> std::process::ExitCode {
    let output = alder_cli::reporting::Output::stderr(alder_cli::reporting::Options::from_startup(
        std::env::args_os().skip(1),
    ));
    let started = std::time::Instant::now();
    match startup(&output) {
        Ok(status) => status,
        Err(error) => {
            output.diagnostic(&error);
            output.failure("compiler selection", started.elapsed());
            std::process::ExitCode::FAILURE
        }
    }
}

fn startup(output: &alder_cli::reporting::Output) -> miette::Result<std::process::ExitCode> {
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
            Ok(if alder_cli::Cli::default().exec().await.is_ok() {
                std::process::ExitCode::SUCCESS
            } else {
                std::process::ExitCode::FAILURE
            })
        })
}
