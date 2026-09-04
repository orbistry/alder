use miette::IntoDiagnostic;

fn main() -> miette::Result<()> {
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
                alder_cli::proxy::maybe_proxy().await?;
            }

            alder_cli::Cli::default().exec().await
        })
}
