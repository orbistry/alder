use std::collections::{BTreeMap, BTreeSet};
use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    InitializeParams, InitializeResult, InitializedParams, MessageType, ServerInfo, Uri,
};
use tower_lsp_server::{Client, LanguageServer};

use crate::capabilities::server_capabilities;
use crate::diagnostics::{Document, check, file_url};

pub const SERVER_NAME: &str = "alder-language-server";

pub struct Server {
    client: Client,
    // Serialize edits through publication so an older check cannot overwrite
    // diagnostics from a newer document version. Compiler work uses the
    // driver's blocking task; the async transport remains responsive.
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    documents: BTreeMap<String, Document>,
    published: BTreeMap<String, Uri>,
}

impl Server {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            state: Mutex::new(State::default()),
        }
    }

    pub fn server_info() -> ServerInfo {
        ServerInfo {
            name: SERVER_NAME.to_owned(),
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
        }
    }

    async fn publish(&self, state: &mut State) {
        let mut projects = BTreeSet::new();
        let mut diagnostics = BTreeMap::<String, (Uri, Vec<Diagnostic>, Option<i32>)>::new();
        for document in state.documents.values() {
            diagnostics.insert(
                document.uri.as_str().to_owned(),
                (document.uri.clone(), vec![], Some(document.version)),
            );
        }
        for document in state.documents.values() {
            let path = document.url.to_file_path().expect("validated file URI");
            let result = match alder_driver::Project::load(&path).await {
                Ok(project) => {
                    if !projects.insert(project.root.clone()) {
                        continue;
                    }
                    check(&project, &state.documents).await
                }
                Err(error) => Err(error),
            };
            match result {
                Ok(output) => {
                    for (url, reports) in output {
                        let open = state.documents.values().find(|doc| doc.url == url);
                        let uri = open
                            .map(|doc| doc.uri.clone())
                            .or_else(|| url.as_str().parse().ok());
                        if let Some(uri) = uri {
                            diagnostics.insert(
                                uri.as_str().to_owned(),
                                (uri, reports, open.map(|doc| doc.version)),
                            );
                        }
                    }
                }
                Err(error) => {
                    diagnostics
                        .get_mut(document.uri.as_str())
                        .expect("open document")
                        .1
                        .push(Diagnostic {
                            severity: Some(DiagnosticSeverity::ERROR),
                            source: Some("alder".to_owned()),
                            message: error.to_string(),
                            ..Diagnostic::default()
                        });
                }
            }
        }
        for (key, uri) in &state.published {
            if !diagnostics.contains_key(key) {
                self.client
                    .publish_diagnostics(uri.clone(), vec![], None)
                    .await;
            }
        }
        state.published.clear();
        for (key, (uri, reports, version)) in diagnostics {
            state.published.insert(key, uri.clone());
            self.client.publish_diagnostics(uri, reports, version).await;
        }
    }
}

impl LanguageServer for Server {
    async fn did_save(&self, _: DidSaveTextDocumentParams) {
        self.publish(&mut *self.state.lock().await).await;
    }

    async fn did_change_watched_files(&self, _: DidChangeWatchedFilesParams) {
        self.publish(&mut *self.state.lock().await).await;
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let doc = params.text_document;
        let Some(url) = file_url(&doc.uri) else {
            return;
        };
        let mut state = self.state.lock().await;
        state.documents.insert(
            doc.uri.as_str().to_owned(),
            Document {
                uri: doc.uri,
                url,
                version: doc.version,
                text: doc.text,
            },
        );
        self.publish(&mut state).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let mut state = self.state.lock().await;
        let Some(doc) = state.documents.get_mut(params.text_document.uri.as_str()) else {
            return;
        };
        if params.text_document.version <= doc.version {
            return;
        }
        // FULL sync is advertised; never apply a ranged update as a full file.
        if params
            .content_changes
            .iter()
            .any(|change| change.range.is_some())
        {
            return;
        }
        let Some(change) = params.content_changes.into_iter().last() else {
            return;
        };
        doc.text = change.text;
        doc.version = params.text_document.version;
        self.publish(&mut state).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let mut state = self.state.lock().await;
        state.documents.remove(params.text_document.uri.as_str());
        self.publish(&mut state).await;
    }

    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: server_capabilities(),
            server_info: Some(Self::server_info()),
            ..InitializeResult::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "alder language server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}
