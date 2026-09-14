use super::*;

#[tokio::test(flavor = "current_thread")]
async fn cloudflare_owned_native_handle_equality_executes_as_identity() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/e2e/web/src/platform.ald");
    let uri = Url::from_file_path(fixture).unwrap();
    let source = indoc::indoc! {r#"
        import cloudflare.{DurableObjectState, WorkflowStep}
        enum ObjectState { ObjectState(DurableObjectState) }
        enum Checkpoint { Checkpoint(WorkflowStep) }
        #[extern("../../../support/cloudflare-handles.js", "checkHandleIdentity")]
        fn checkHandleIdentity(states: fn(DurableObjectState, DurableObjectState) Bool, steps: fn(WorkflowStep, WorkflowStep) Bool) ()
        pub fn main() {
            checkHandleIdentity(
                (left, right) -> ObjectState::ObjectState(left) == ObjectState::ObjectState(right),
                (left, right) -> Checkpoint::Checkpoint(left) == Checkpoint::Checkpoint(right),
            )
        }
    "#};
    let result = build_fixture_sync(
        vec![(uri.clone(), Ok(source.to_owned()))],
        BuildMode::Build,
        BuildDependencies::default(),
    );
    assert!(result.is_success(), "{result:?}");
    let entry = result.artifacts[&uri].module_id.clone();
    let code = alder_bundle::bundle(
        result.artifacts.into_values(),
        &entry,
        alder_bundle::EntryKind::Standalone,
    )
    .await
    .unwrap();
    assert_eq!(alder_runtime::execute(code, vec![]).await.unwrap(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn cloudflare_traits_produce_solved_adapter_metadata() {
    let uri = url("project/src/main.ald");
    let source = indoc::indoc! {r#"
        import cloudflare.{DurableObject, DurableObjectState, Queue, Workflow, WorkflowEvent, WorkflowStep, Kv, DurableObjectNamespace, WorkflowBinding}
        import http
        import http.{Request, Response}

        #[binding("CACHE", "kv", "0123456789abcdef0123456789abcdef")]
        pub type Cache = Kv
        #[binding("COUNTERS", "durable_object", "Counter")]
        pub type Counters = DurableObjectNamespace
        #[binding("JOBS", "workflow", "process-jobs", "Job")]
        pub type Jobs = WorkflowBinding

        #[durable_object("Counter")]
        pub enum Counter { Counter(DurableObjectState) }
        impl DurableObject[Counter] {
            async fn initObject(storage: DurableObjectState) Counter { Counter::Counter(storage) }
            async fn fetch(object: Counter, request: Request) Response { http.json(42) }
        }

        #[queue("messages")]
        pub enum Consumer { Consumer }
        impl Queue[Consumer, Number] {
            fn initConsumer() Consumer { Consumer::Consumer }
            async fn consume(consumer: Consumer, messages: Array[Number]) { () }
        }

        #[workflow("Job")]
        pub enum Job { Job }
        pub enum Checkpoint { Checkpoint(WorkflowStep) }
        impl Workflow[Job, Number, Number] {
            fn initWorkflow() Job { Job::Job }
            async fn run(workflow: Job, event: WorkflowEvent[Number], step: WorkflowStep) Number { event.payload }
        }
        pub fn main() { () }
    "#};
    let result = build_fixture_sync(
        vec![(uri.clone(), Ok(source.to_owned()))],
        BuildMode::Build,
        BuildDependencies::default(),
    );
    assert!(result.is_success(), "{result:?}");
    let interface = result
        .interfaces
        .iter()
        .find(|interface| interface.module.path == ["main"])
        .unwrap();
    let metadata = crate::cloudflare::extract(uri.as_str(), source, interface).unwrap();
    let joined = crate::cloudflare::extract_build(&result).unwrap();
    assert_eq!(joined.adapters, metadata.adapters);
    assert_eq!(joined.bindings, metadata.bindings);
    assert_eq!(metadata.adapters.len(), 3);
    assert_eq!(metadata.bindings.len(), 3);
    for adapter in &metadata.adapters {
        assert!(
            interface
                .instances
                .iter()
                .any(|implementation| implementation.dictionary_symbol == adapter.dictionary)
        );
    }
    let emitted =
        alder_codegen::support::cloudflare::adapters(&metadata.adapters, &metadata.providers());
    assert_eq!(emitted.module_id, "alder:cloudflare-adapters");
    let options = crate::cloudflare::ConfigOptions {
        name: "fixture".to_owned(),
        compatibility_date: "2026-09-14".to_owned(),
        main: "worker.js".to_owned(),
        assets_directory: "client".to_owned(),
        account_id: None,
        legacy_migrations: None,
    };
    let config = crate::cloudflare::wrangler_config(&metadata, &options).unwrap();
    assert_eq!(config["exports"]["Counter"]["storage"], "sqlite");
    assert_eq!(config["queues"]["consumers"][0]["queue"], "messages");
    assert_eq!(config["workflows"][0]["class_name"], "Job");
    assert!(config.get("migrations").is_none());
    let code = alder_bundle::bundle_entry(
        result.artifacts.into_values().chain([emitted]),
        "alder:cloudflare-adapters",
    )
    .await
    .unwrap();
    assert!(code.contains("cloudflare:workers"));
    assert!(code.contains("$super0"));
    assert!(code.contains("$super1"));
}

#[test]
fn cloudflare_attributes_reject_missing_implementation_and_wrong_binding_type() {
    let uri = url("project/src/main.ald");
    let source = "#[durable_object(\"Counter\")]\npub type Counter = { count: Number }\n#[binding(\"CACHE\", \"kv\", \"id\")]\npub type Cache = Number";
    let result = build_fixture_sync(
        vec![(uri.clone(), Ok(source.to_owned()))],
        BuildMode::Check,
        BuildDependencies::default(),
    );
    assert!(result.is_success(), "{result:?}");
    let errors =
        crate::cloudflare::extract(uri.as_str(), source, &result.interfaces[0]).unwrap_err();
    assert_eq!(errors.len(), 2);
    assert!(errors[0].message.contains("DurableObject implementation"));
    assert!(errors[1].message.contains("cloudflare.Kv"));
}
