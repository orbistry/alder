//! A small, capability-oriented host around `deno_core`.

use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

use deno_core::{JsRuntime, ModuleCodeString, ModuleSpecifier, OpState, RuntimeOptions, op2};
use deno_permissions::{PermissionsContainer, RuntimePermissionDescriptorParser};

mod dev;
pub use dev::{DevEvent, execute_dev};

#[derive(Default)]
struct HostState {
    args: Vec<String>,
    exit_code: i32,
    test_reporter: Option<Box<dyn Fn(TestEvent)>>,
    build_output: Option<Rc<RefCell<Option<String>>>>,
}

#[op2(fast)]
fn op_alder_build_output(
    state: &mut OpState,
    #[string] output: String,
) -> Result<(), deno_error::JsErrorBox> {
    let host = state.borrow::<Rc<RefCell<HostState>>>().borrow();
    let sink = host
        .build_output
        .as_ref()
        .ok_or_else(|| deno_error::JsErrorBox::generic("No build evaluation is active"))?;
    let mut sink = sink.borrow_mut();
    if sink.is_some() {
        return Err(deno_error::JsErrorBox::generic(
            "Build evaluation reported more than one result",
        ));
    }
    *sink = Some(output);
    Ok(())
}

/// Results from the test runner, separate from the program's console streams.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TestEvent {
    Passed {
        module: String,
        name: String,
    },
    Failed {
        module: String,
        name: String,
        message: String,
    },
    Finished {
        passed: usize,
        failed: usize,
    },
}

#[op2]
fn op_alder_test_report(state: &mut OpState, #[serde] event: TestEvent) -> bool {
    let host = state.borrow::<Rc<RefCell<HostState>>>().borrow();
    if let Some(report) = &host.test_reporter {
        report(event);
        true
    } else {
        false
    }
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
        deno_telemetry,
        deno_http
    ],
    ops = [op_alder_print, op_alder_args, op_alder_exit, op_alder_sleep, op_alder_test_report, op_alder_build_output, dev::op_alder_dev_event],
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
Object.defineProperty(globalThis, "__alderHost", {
    value: Object.freeze({
        args: core.ops.op_alder_args(),
        exit: (code) => core.ops.op_alder_exit(code),
        reportTest: (event) => core.ops.op_alder_test_report(event),
        serve: globalThis.__alderServe,
        devEvent: () => core.ops.op_alder_dev_event(),
        reportBuild: (value) => core.ops.op_alder_build_output(value),
    }),
    enumerable: false,
    configurable: false,
    writable: false,
});
"#;

pub async fn execute(bundle: String, args: Vec<String>) -> Result<i32, deno_core::error::AnyError> {
    execute_inner(bundle, args, None, None, None).await
}

/// Evaluate a compiler-generated build entry without scraping program output.
pub async fn execute_build(bundle: String) -> Result<String, deno_core::error::AnyError> {
    let output = Rc::new(RefCell::new(None));
    let status = execute_inner(bundle, Vec::new(), None, None, Some(output.clone())).await?;
    if status != 0 {
        return Err(deno_error::JsErrorBox::generic(format!(
            "Build evaluation exited with {status}"
        ))
        .into());
    }
    let value = output.borrow_mut().take().ok_or_else(|| {
        deno_error::JsErrorBox::generic("Build evaluation did not return a result")
    })?;
    Ok(value)
}

/// Execute a test bundle with structured results. User stdout/stderr are unchanged.
pub async fn execute_tests(
    bundle: String,
    report: impl Fn(TestEvent) + 'static,
) -> Result<i32, deno_core::error::AnyError> {
    execute_inner(bundle, Vec::new(), Some(Box::new(report)), None, None).await
}

async fn execute_inner(
    bundle: String,
    args: Vec<String>,
    test_reporter: Option<Box<dyn Fn(TestEvent)>>,
    dev: Option<Rc<dev::DevState>>,
    build_output: Option<Rc<RefCell<Option<String>>>>,
) -> Result<i32, deno_core::error::AnyError> {
    // Workspace consumers may enable more than one rustls provider through
    // unrelated dependencies. Select the provider used by Deno's pinned TLS
    // stack explicitly so fetch never relies on feature inference.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let state = Rc::new(RefCell::new(HostState {
        args,
        exit_code: 0,
        test_reporter,
        build_output,
    }));
    let host_state = state.clone();
    let mut runtime = JsRuntime::new(RuntimeOptions {
        module_loader: dev
            .as_ref()
            .map(|state| state.clone() as Rc<dyn deno_core::ModuleLoader>),
        extension_transpiler: Some(Rc::new(|name, source| {
            if !name.ends_with(".ts") {
                return Ok((source, None));
            }
            // Only pinned upstream extension TypeScript crosses this parser;
            // Alder application code continues to arrive as emitted ESM.
            let allocator = oxc_allocator::Allocator::default();
            let parsed =
                oxc_parser::Parser::new(&allocator, &source, oxc_span::SourceType::ts()).parse();
            if !parsed.errors.is_empty() {
                return Err(deno_error::JsErrorBox::generic(format!(
                    "cannot parse runtime extension {name}: {:?}",
                    parsed.errors
                )));
            }
            let mut program = parsed.program;
            let semantic = oxc_semantic::SemanticBuilder::new().build(&program);
            if !semantic.errors.is_empty() {
                return Err(deno_error::JsErrorBox::generic(format!(
                    "cannot analyze runtime extension {name}: {:?}",
                    semantic.errors
                )));
            }
            let options = oxc_transformer::TransformOptions::default();
            let transformed = oxc_transformer::Transformer::new(
                &allocator,
                std::path::Path::new(name.as_str()),
                &options,
            )
            .build_with_scoping(semantic.semantic.into_scoping(), &mut program);
            if !transformed.errors.is_empty() {
                return Err(deno_error::JsErrorBox::generic(format!(
                    "cannot lower runtime extension {name}: {:?}",
                    transformed.errors
                )));
            }
            Ok((
                oxc_codegen::Codegen::new().build(&program).code.into(),
                None,
            ))
        })),
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
            deno_telemetry::deno_telemetry::init(),
            deno_http::deno_http::init(Default::default()),
            alder_host::init(),
        ],
        ..Default::default()
    });
    {
        let op_state_handle = runtime.op_state();
        let mut op_state = op_state_handle.borrow_mut();
        op_state.put(host_state);
        if let Some(dev) = dev {
            op_state.put(dev);
        }
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
    async fn build_report_is_structured_and_independent_of_console_output() {
        let report = execute_build(r#"console.log("not the build result"); console.error("also not the result"); await Promise.resolve(); __alderHost.reportBuild('{"pages":["/one"]}');"#.to_owned()).await.unwrap();
        assert_eq!(report, r#"{"pages":["/one"]}"#);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn build_report_requires_exactly_one_result_and_successful_exit() {
        for (code, expected) in [
            ("console.log('no result');", "did not return a result"),
            (
                "__alderHost.reportBuild('one'); __alderHost.reportBuild('two');",
                "more than one result",
            ),
            (
                "__alderHost.reportBuild('result'); __alderHost.exit(7);",
                "exited with 7",
            ),
            (
                "__alderHost.reportBuild('result'); throw new Error('later failure');",
                "later failure",
            ),
        ] {
            let error = execute_build(code.to_owned()).await.unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn build_report_sink_is_isolated_between_runs_and_disabled_otherwise() {
        assert_eq!(
            execute_build("__alderHost.reportBuild('first');".to_owned())
                .await
                .unwrap(),
            "first"
        );
        assert_eq!(
            execute_build("__alderHost.reportBuild('second');".to_owned())
                .await
                .unwrap(),
            "second"
        );
        let error = execute("__alderHost.reportBuild('not allowed');".to_owned(), vec![])
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("No build evaluation is active"),
            "{error}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn executes_esm_and_records_exit_code() {
        let code =
            "globalThis.__alderHost.exit(__alderHost.args.length); export default 0;".to_owned();
        assert_eq!(execute(code, vec!["one".to_owned()]).await.unwrap(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn structured_test_reporting_is_opt_in_and_preserves_exit_status() {
        let code = "const handled = __alderHost.reportTest({kind: 'finished', passed: 2, failed: 1}); __alderHost.exit(handled ? 7 : 3);".to_owned();
        assert_eq!(execute(code.clone(), Vec::new()).await.unwrap(), 3);
        let events = Rc::new(RefCell::new(Vec::new()));
        let received = events.clone();
        assert_eq!(
            execute_tests(code, move |event| received.borrow_mut().push(event))
                .await
                .unwrap(),
            7
        );
        assert_eq!(
            *events.borrow(),
            vec![TestEvent::Finished {
                passed: 2,
                failed: 1
            }]
        );
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

    #[tokio::test(flavor = "current_thread")]
    async fn standalone_http_serves_concurrent_requests_and_releases_listener() {
        let code = r#"
const server = __alderHost.serve(async request => {
  const url = new URL(request.url);
  return new Response(await request.text() + url.pathname, {headers: {"x-alder": "http"}});
}, {port: 0});
const base = `http://127.0.0.1:${server.addr.port}`;
const responses = await Promise.all(["one", "two"].map(value => fetch(base + "/" + value, {method: "POST", body: value})));
for (let index = 0; index < responses.length; index++) {
  const word = index === 0 ? "one" : "two";
  if (await responses[index].text() !== word + "/" + word || responses[index].headers.get("x-alder") !== "http") throw Error("HTTP response mismatch");
}
await server.shutdown();
await server.shutdown();
const reused = __alderHost.serve(() => new Response("reused"), {port: server.addr.port});
await reused.shutdown();
"#.to_owned();
        tokio::time::timeout(Duration::from_secs(10), execute(code, vec![]))
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancelled_web_timers_do_not_keep_the_runtime_alive() {
        let code = r#"
const timeout = setTimeout(() => {throw Error("cancelled timeout ran");}, 60000);
const interval = setInterval(() => {throw Error("cancelled interval ran");}, 60000);
clearTimeout(timeout); clearInterval(interval);
await new Promise(resolve => setTimeout(resolve, 1));
"#
        .to_owned();
        tokio::time::timeout(Duration::from_secs(2), execute(code, vec![]))
            .await
            .expect("cancelled timers release event-loop references")
            .unwrap();
    }
}
