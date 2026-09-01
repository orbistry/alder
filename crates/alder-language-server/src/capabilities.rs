use tower_lsp_server::ls_types::ServerCapabilities;

pub fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities::default()
}
