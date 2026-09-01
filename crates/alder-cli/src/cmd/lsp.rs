use miette::Result;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tower_lsp_server::{LspService, Server};

#[derive(clap::Args, Debug)]
pub struct Args;

pub async fn exec(_: Args) -> Result<()> {
    let (service, socket) = LspService::new(alder_language_server::Server::new);
    Server::new(
        tokio::io::stdin().compat(),
        tokio::io::stdout().compat_write(),
        socket,
    )
    .serve(service)
    .await;

    Ok(())
}
