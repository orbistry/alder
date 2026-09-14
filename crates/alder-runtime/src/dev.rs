//! In-process development module replacement. Only compiler-produced bundles
//! enter the loader; no filesystem/network JavaScript loader is exposed.
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use deno_core::{
    ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader, ModuleSource,
    ModuleSourceCode, ModuleSpecifier, ModuleType, OpState, ResolutionKind, op2,
};
use deno_error::JsErrorBox;
use tokio::sync::{Mutex, mpsc};

#[derive(Debug)]
pub enum DevEvent {
    Build { server: String },
    Error { message: String },
    Shutdown,
}

pub(crate) struct DevState {
    receiver: Mutex<mpsc::Receiver<DevEvent>>,
    modules: RefCell<BTreeMap<String, String>>,
    revision: std::cell::Cell<u64>,
}

#[derive(serde::Serialize)]
pub(crate) struct Message {
    kind: &'static str,
    revision: u64,
    module: Option<String>,
    message: Option<String>,
}

#[op2]
#[serde]
pub(crate) async fn op_alder_dev_event(state: Rc<RefCell<OpState>>) -> Result<Message, JsErrorBox> {
    let state = state
        .borrow()
        .try_borrow::<Rc<DevState>>()
        .cloned()
        .ok_or_else(|| JsErrorBox::generic("No Alder development session is active"))?;
    let event = state
        .receiver
        .lock()
        .await
        .recv()
        .await
        .unwrap_or(DevEvent::Shutdown);
    let mut result = Message {
        kind: "shutdown",
        revision: state.revision.get(),
        module: None,
        message: None,
    };
    match event {
        DevEvent::Build { server } => {
            let revision = state.revision.get() + 1;
            state.revision.set(revision);
            let module = format!("alder://dev/{revision}.mjs");
            let mut modules = state.modules.borrow_mut();
            modules.clear();
            modules.insert(module.clone(), server);
            result.kind = "build";
            result.revision = revision;
            result.module = Some(module);
        }
        DevEvent::Error { message } => {
            result.kind = "error";
            result.message = Some(message);
        }
        DevEvent::Shutdown => {}
    }
    Ok(result)
}

impl ModuleLoader for DevState {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _: ResolutionKind,
    ) -> Result<ModuleSpecifier, JsErrorBox> {
        deno_core::resolve_import(specifier, referrer).map_err(JsErrorBox::from_err)
    }

    fn load(
        &self,
        specifier: &ModuleSpecifier,
        _: Option<&ModuleLoadReferrer>,
        _: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        let result = self
            .modules
            .borrow()
            .get(specifier.as_str())
            .cloned()
            .ok_or_else(|| {
                JsErrorBox::generic("Development module was not supplied by the Alder compiler")
            })
            .map(|code| {
                ModuleSource::new(
                    ModuleType::JavaScript,
                    ModuleSourceCode::String(code.into()),
                    specifier,
                    None,
                )
            });
        ModuleLoadResponse::Sync(result)
    }
}

pub async fn execute_dev(
    receiver: mpsc::Receiver<DevEvent>,
    hostname: String,
    port: u16,
) -> Result<i32, deno_core::error::AnyError> {
    let state = Rc::new(DevState {
        receiver: Mutex::new(receiver),
        modules: RefCell::new(BTreeMap::new()),
        revision: std::cell::Cell::new(0),
    });
    super::execute_inner(
        include_str!("../js/dev.js").to_owned(),
        vec![hostname, port.to_string()],
        None,
        Some(state),
        None,
    )
    .await
}
