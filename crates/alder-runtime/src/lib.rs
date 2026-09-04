//! A small, capability-oriented host around `deno_core`.

use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

use deno_core::{JsRuntime, ModuleCodeString, ModuleSpecifier, OpState, RuntimeOptions, op2};
use deno_permissions::{PermissionsContainer, RuntimePermissionDescriptorParser};

#[derive(Default)]
struct HostState {
    args: Vec<String>,
    exit_code: i32,
}

#[op2(fast)]
fn op_alder_print(#[string] text: &str, stderr: bool) {
    if stderr {
        eprintln!("{text}");
    } else {
        println!("{text}");
    }
}

#[op2]
#[serde]
fn op_alder_args(state: &mut OpState) -> Vec<String> {
    state
        .borrow::<Rc<RefCell<HostState>>>()
        .borrow()
        .args
        .clone()
}

#[op2(fast)]
fn op_alder_exit(state: &mut OpState, code: i32) {
    state
        .borrow::<Rc<RefCell<HostState>>>()
        .borrow_mut()
        .exit_code = code;
}

#[op2(async(deferred), fast)]
async fn op_alder_sleep(#[number] milliseconds: u64) {
    tokio::time::sleep(Duration::from_millis(milliseconds)).await;
}

deno_core::extension!(
    alder_host,
    deps = [
        deno_webidl,
        deno_web,
        deno_fs,
        deno_fetch,
        deno_crypto,
        deno_net,
        deno_websocket,
        deno_http
    ],
    ops = [op_alder_print, op_alder_args, op_alder_exit, op_alder_sleep],
    esm_entry_point = "ext:alder_host/bootstrap.js",
    esm = [dir "js", "bootstrap.js"],
);

const BOOTSTRAP: &str = r#"
const core = Deno.core;
const render = (value) => typeof value === "string" ? value :
    value instanceof Error ? (value.stack ?? value.message) :
    (() => { try { return JSON.stringify(value); } catch { return String(value); } })();
globalThis.console = {
    log: (...values) => core.ops.op_alder_print(values.map(render).join(" "), false),
    error: (...values) => core.ops.op_alder_print(values.map(render).join(" "), true),
};
globalThis.setTimeout = (callback, milliseconds = 0, ...args) => {
    let cancelled = false;
    core.ops.op_alder_sleep(milliseconds).then(() => { if (!cancelled) callback(...args); });
    return { cancel: () => { cancelled = true; } };
};
globalThis.clearTimeout = (timer) => timer?.cancel?.();
Object.defineProperty(globalThis, "__alderHost", {
    value: Object.freeze({
        args: core.ops.op_alder_args(),
        exit: (code) => core.ops.op_alder_exit(code),
    }),
    enumerable: false,
    configurable: false,
    writable: false,
});
"#;

pub async fn execute(bundle: String, args: Vec<String>) -> Result<i32, deno_core::error::AnyError> {
    // Workspace consumers may enable more than one rustls provider through
    // unrelated dependencies. Select the provider used by Deno's pinned TLS
    // stack explicitly so fetch never relies on feature inference.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let state = Rc::new(RefCell::new(HostState { args, exit_code: 0 }));
    let host_state = state.clone();
    let mut runtime = JsRuntime::new(RuntimeOptions {
        extensions: vec![
            deno_webidl::deno_webidl::init(),
            deno_web::deno_web::init(
                Arc::new(deno_web::BlobStore::default()),
                None,
                false,
                deno_web::InMemoryBroadcastChannel::default(),
            ),
            deno_fs::deno_fs::init(Rc::new(deno_fs::RealFs)),
            deno_fetch::deno_fetch::init(Default::default()),
            deno_crypto::deno_crypto::init(None),
            deno_net::deno_net::init(None, None),
            deno_websocket::deno_websocket::init(),
            deno_http::deno_http::init(Default::default()),
            alder_host::init(),
        ],
        ..Default::default()
    });
    {
        let op_state_handle = runtime.op_state();
        let mut op_state = op_state_handle.borrow_mut();
        op_state.put(host_state);
        op_state.put(PermissionsContainer::allow_all(Arc::new(
            RuntimePermissionDescriptorParser::new(sys_traits::impls::RealSys),
        )));
        op_state.put(Arc::new(deno_features::FeatureChecker::default()));
    }
    runtime.execute_script("alder:bootstrap", BOOTSTRAP)?;
    let specifier = ModuleSpecifier::parse("alder://bundle/main.mjs")?;
    let module = runtime
        .load_main_es_module_from_code(&specifier, ModuleCodeString::from(bundle))
        .await?;
    let evaluation = runtime.mod_evaluate(module);
    runtime.run_event_loop(Default::default()).await?;
    evaluation.await?;
    let exit_code = state.borrow().exit_code;
    Ok(exit_code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn executes_esm_and_records_exit_code() {
        let code =
            "globalThis.__alderHost.exit(__alderHost.args.length); export default 0;".to_owned();
        assert_eq!(execute(code, vec!["one".to_owned()]).await.unwrap(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn installs_web_fetch_url_and_crypto_globals() {
        let code = r#"
const response = await fetch("data:text/plain,web-ok");
const valid = await response.text() === "web-ok"
    && new URL("https://example.com/path").hostname === "example.com"
    && typeof crypto.randomUUID() === "string";
__alderHost.exit(valid ? 0 : 1);
"#
        .to_owned();
        assert_eq!(execute(code, Vec::new()).await.unwrap(), 0);
    }
}
