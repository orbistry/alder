use miette::Result;
use tower_lsp_server::{LspService, Server};

#[derive(clap::Args, Debug)]
pub struct Args;

pub async fn exec(_: Args) -> Result<()> {
    let (service, socket) = LspService::new(alder_language_server::Server::new);
    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;

    Ok(())
}
