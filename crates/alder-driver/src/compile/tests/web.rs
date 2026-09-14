use super::*;

const COUNTER: &str = include_str!("../../../../../tests/e2e/web/src/counter.ald");

#[tokio::test(flavor = "current_thread")]
async fn markup_whitespace_before_and_after_formatting_renders_and_hydrates_identically() {
    let source = indoc::indoc! {r#"
        import html.{Html}
        #[extern("../../../support/web-harness.js", "checkWhitespace")]
        fn checkWhitespace(factory: fn() Html) Task[()]
        pub component View() {
            let name = state("Ada")
            <main>
                <div id="adjacent"><p>A</p><p>B</p></div>
                <p id="prose">
                    Hello

                    world
                </p>
                <p id="inline">Hello <strong>Ada</strong>!</p>
                <p id="joined">Hello
                    <strong>Ada</strong>!</p>
                <p id="explicit">Hello{" "}
                    <strong>Ada</strong>!</p>
                <p id="expression">Hello {name}!</p>
                <p id="punctuation">{name}
                    !</p>
                <p id="spaces">A  B</p>
                <p id="runtime">{"  A\n B  "}</p>
                <pre id="pre">
          A

         B  </pre>
                <textarea id="textarea">
          A
         B  </textarea>
                <button onClick={() -> { name = "Grace" }}>Change</button>
            </main>
        }
        pub async fn main() { checkWhitespace(View).await }
    "#};
    let formatted =
        alder_fmt::format_source(source).expect("markup formats without semantic changes");
    assert_eq!(alder_fmt::format_source(&formatted).unwrap(), formatted);
    run_compiled_web(source.to_owned()).await;
    run_compiled_web(formatted).await;
}

#[tokio::test(flavor = "current_thread")]
async fn named_task_handlers_and_read_closures_capture_live_state_without_running_at_setup() {
    run_compiled_web(
        indoc::indoc! {r#"
        import html.{Html}
        #[extern("../../../support/web-harness.js", "checkNamedHandlers")]
        fn checkNamedHandlers(factory: fn() Html) Task[()]
        pub component View() {
            let count = state(0)
            let clicked = () -> async { count += 1 }
            let read = () -> count
            <button onClick={clicked}>{read()}</button>
        }
        pub async fn main() { checkNamedHandlers(View).await }
    "#}
        .to_owned(),
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn imported_provider_use_reads_hook_context_across_tasks_and_scopes() {
    let session = "pub type Session = { user: String, nickname?: String }";
    let reader = "import ~/session.{Session}\npub async fn read() String { use Session\nassert Session.nickname == None\nSession.user }\npub fn conditional(flag: Bool) String { if flag { use Session\nSession.user } else { \"guest\" } }";
    let source = indoc::indoc! {r#"
        import ~/session.{Session}
        import ~/reader
        pub async fn main() {
            let first = provide Session = { user: "alice" } {
                let outer = reader.read().await
                let inner = provide Session = { user: "bob" } { reader.read().await }
                assert inner == "bob"
                assert reader.conditional(true) == "alice"
                assert reader.read().await == outer
                outer
            }
            assert first == "alice"
            assert reader.conditional(false) == "guest"
        }
    "#};
    run_compiled_web_with_modules(
        source.to_owned(),
        &[("session.ald", session), ("reader.ald", reader)],
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn imported_module_stores_are_request_scoped_and_reactive_after_hydration() {
    let store = "pub let count: Number = state(1)\npub fn increment() { count += 1 }\npub fn read() Number { count }";
    let source = indoc::indoc! {r#"
        import html.{Html}
        import ~/store as store
        #[extern("../../../support/web-harness.js", "checkStores")]
        fn checkStores(factory: fn() Html, read: fn() Number, increment: fn() ()) Task[()]
        pub component View() {
            let doubled = store.count * 2
            <div><button onClick={() -> store.increment()}>{store.count}</button><span>{doubled}</span></div>
        }
        pub async fn main() { checkStores(View, store.read, store.increment).await }
    "#};
    run_compiled_web_with_modules(source.to_owned(), &[("store.ald", store)]).await;
}

#[tokio::test(flavor = "current_thread")]
async fn imported_helpers_track_transitive_private_store_captures() {
    let store = "let count: Number = state(1)\npub fn increment() { count += 1 }\npub fn read() Number { count }";
    let helper = "import ~/store\npub fn read() Number { store.read() }\npub fn increment() { store.increment() }";
    let source = indoc::indoc! {r#"
        import html.{Html}
        import ~/helper
        #[extern("../../../support/web-harness.js", "checkStores")]
        fn checkStores(factory: fn() Html, read: fn() Number, increment: fn() ()) Task[()]
        pub component View() {
            let doubled = helper.read() * 2
            <div><button onClick={() -> helper.increment()}>{helper.read()}</button><span>{doubled}</span></div>
        }
        pub async fn main() { checkStores(View, helper.read, helper.increment).await }
    "#};
    run_compiled_web_with_modules(
        source.to_owned(),
        &[("store.ald", store), ("helper.ald", helper)],
    )
    .await;
}

#[test]
fn module_store_reads_cannot_escape_into_eager_initializers() {
    let source = "let count: Number = state(1)\nfn read() Number { count }\npub let eager = read()";
    assert_rendered_diagnostic_snapshot!(source, web_diagnostic(source, BuildMode::Build));
}

#[test]
fn resource_loader_requires_a_task_returning_result() {
    let source =
        "import html\npub component View() { let data = html.resource(() -> 42)\n<span /> }";
    assert_rendered_diagnostic_snapshot!(source, web_diagnostic(source, BuildMode::Check));
}

#[test]
fn resource_creation_requires_a_component_owner() {
    let source = "import html\nasync fn load() Result[Number, [:missing]] { Ok(1) }\npub fn outside() { html.resource(() -> load()) }";
    assert_rendered_diagnostic_snapshot!(source, web_diagnostic(source, BuildMode::Build));
}

#[test]
fn resource_refresh_requires_the_live_binding() {
    let source = "import html.{Resource}\nimport html\npub component View() { let data: Resource[Number, [:missing]] = Resource::Ready(1)\n<button onClick={() -> html.refresh(data)}>refresh</button> }";
    assert_rendered_diagnostic_snapshot!(source, web_diagnostic(source, BuildMode::Build));
}

#[tokio::test(flavor = "current_thread")]
async fn compiled_resources_suspend_ssr_hydrate_without_fetch_and_refresh() {
    run_compiled_web(
        indoc::indoc! {r#"
        import html.{Html, Resource}
        import html
        #[extern("../../../support/web-harness.js", "resourceLoad")]
        fn load(value: Number) Task[Result[Number, [:missing(String)]]]
        #[extern("../../../support/web-harness.js", "resourceSetup")]
        fn setup() ()
        #[extern("../../../support/web-harness.js", "checkResources")]
        fn checkResources(factory: fn() Html) Task[()]

        pub component View() {
            let _ = setup()
            let count = state(1)
            let data = html.resource(() -> load(count))
            <div>
                <button id="next" onClick={() -> { count += 1 }}>next</button>
                <button id="refresh" onClick={() -> html.refresh(data)}>refresh</button>
                <button id="cancel" onClick={() -> html.cancel(data)}>cancel</button>
                @match data {
                    Resource::Loading => <p id="loading">loading</p>,
                    Resource::Ready(value) => <span id="ready">{value}</span>,
                    Resource::Failed(:missing(reason)) => <strong id="failed">{reason}</strong>,
                }
            </div>
        }
        pub async fn main() { checkResources(View).await }
    "#}
        .to_owned(),
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn compiled_composition_directives_and_keyed_rows_reuse_hydrated_nodes() {
    run_compiled_web(indoc::indoc! {r#"
        import html.{Html}
        #[extern("../../../support/web-harness.js", "checkComposition")]
        fn checkComposition(factory: fn() Html) ()

        component Frame(props: { title: String, children: Html }) {
            <section title={props.title}>{props.children}</section>
        }
        component Count({ value }: { value: Number }) {
            let doubled = value * 2
            <span id="prop">{doubled}</span>
        }
        pub component View() {
            let count = state(0)
            let visible = state(true)
            let items = state([{ id: 1, text: "a" }, { id: 2, text: "b" }])
            <div>
                <button id="increment" onClick={() -> { count += 1 }}>increment</button>
                <button id="toggle" onClick={() -> { visible = !visible }}>toggle</button>
                <button id="reorder" onClick={() -> { items = [{ id: 2, text: "B" }, { id: 1, text: "A" }] }}>reorder</button>
                <Frame title="frame">
                    <Count value={count} />
                    <span id="count">{count}</span>
                    @if visible { <strong id="branch">yes</strong> } @else { <p>no</p> }
                    @match count { 0 => <p id="match">zero</p>, n => { let copy = n * 1
                        <p id="match">{copy}</p> } }
                    @for item in items; key item.id { <button class="row">{item.text}</button> }
                </Frame>
            </div>
        }
        pub fn main() { checkComposition(View) }
    "#}.to_owned()).await;
}

fn web_diagnostic(source: &str, mode: BuildMode) -> Diagnostic {
    let uri = url("project/src/main.ald");
    let result = build_fixture_sync(
        vec![(uri.clone(), Ok(source.to_owned()))],
        mode,
        BuildDependencies::default(),
    );
    assert!(result.artifacts.is_empty());
    let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
        panic!("invalid web source compiled: {result:?}");
    };
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    diagnostics[0].clone()
}

#[test]
fn hot_state_signatures_include_inferred_empty_collection_element_types() {
    let code = |element: &str| {
        let source = format!(
            "fn initial() Array[{element}] {{ [] }}\npub component View() {{ let items = state(initial())\n<span /> }}"
        );
        let uri = url("project/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source))],
            BuildMode::Build,
            BuildDependencies::default(),
        );
        assert!(result.is_success(), "{result:?}");
        result.artifacts[&uri].code().to_owned()
    };
    let numbers = code("Number");
    let strings = code("String");
    let signature = |code: &str| {
        code.lines()
            .find(|line| line.contains("state:items:"))
            .unwrap()
            .to_owned()
    };
    assert_ne!(
        signature(&numbers),
        signature(&strings),
        "HMR type compatibility must not depend only on the empty runtime array shape"
    );
}

#[test]
fn counter_emits_real_signal_and_render_operations() {
    let uri = url("project/src/main.ald");
    let result = build_fixture_sync(
        vec![(uri.clone(), Ok(COUNTER.to_owned()))],
        BuildMode::Build,
        BuildDependencies::default(),
    );
    assert!(result.is_success(), "{result:?}");
    insta::assert_snapshot!(result.artifacts[&uri].code());
}

#[test]
fn unknown_element_has_source_diagnostic() {
    let source = "pub component View() { <nothtml>unknown</nothtml> }";
    assert_rendered_diagnostic_snapshot!(source, web_diagnostic(source, BuildMode::Check));
}

#[test]
fn unknown_attribute_has_source_diagnostic() {
    let source = "pub component View() { <div mystery=\"value\" /> }";
    assert_rendered_diagnostic_snapshot!(source, web_diagnostic(source, BuildMode::Check));
}

#[test]
fn wrong_attribute_type_has_source_diagnostic() {
    let source = "pub component View() { <button disabled={42} /> }";
    assert_rendered_diagnostic_snapshot!(source, web_diagnostic(source, BuildMode::Check));
}

#[test]
fn wrong_event_handler_has_source_diagnostic() {
    let source = "pub component View() { <button onClick={() -> 42} /> }";
    assert_rendered_diagnostic_snapshot!(source, web_diagnostic(source, BuildMode::Check));
}

#[test]
fn wrong_props_have_source_diagnostic() {
    let source = format!(
        "{COUNTER}\npub fn bad() {{ Counter({{ initial: \"wrong\", label: \"counter\" }}) }}"
    );
    assert_rendered_diagnostic_snapshot!(
        source.as_str(),
        web_diagnostic(&source, BuildMode::Check)
    );
}

#[test]
fn invalid_nesting_has_source_diagnostic() {
    let source = "pub component View() { <p><span><div /></span></p> }";
    assert_rendered_diagnostic_snapshot!(source, web_diagnostic(source, BuildMode::Check));
}

#[test]
fn nested_anchor_has_source_diagnostic() {
    let source = "pub component View() { <a><span><a href=\"/\" /></span></a> }";
    assert_rendered_diagnostic_snapshot!(source, web_diagnostic(source, BuildMode::Check));
}

#[test]
fn nested_form_has_source_diagnostic() {
    let source = "pub component View() { <form><div><form /></div></form> }";
    assert_rendered_diagnostic_snapshot!(source, web_diagnostic(source, BuildMode::Check));
}

#[test]
fn interactive_descendant_has_source_diagnostic() {
    let source = "pub component View() { <a href=\"/\">@if true { <input /> }</a> }";
    assert_rendered_diagnostic_snapshot!(source, web_diagnostic(source, BuildMode::Check));
}

#[test]
fn checkbox_input_event_fields_have_source_diagnostic() {
    let source = "pub component View() { <input type=\"checkbox\" onInput={event -> { let kind = event.inputType\n() }} /> }";
    assert_rendered_diagnostic_snapshot!(source, web_diagnostic(source, BuildMode::Check));
}

#[test]
fn invalid_text_hole_has_source_diagnostic() {
    let source = "pub component View() { <span>{[1, 2]}</span> }";
    assert_rendered_diagnostic_snapshot!(source, web_diagnostic(source, BuildMode::Check));
}

#[test]
fn derived_assignment_has_source_diagnostic() {
    let source = "pub component View() { let count = state(0)\nlet doubled = count * 2\n<button onClick={() -> { doubled += 1 }}>{doubled}</button> }";
    assert_rendered_diagnostic_snapshot!(source, web_diagnostic(source, BuildMode::Build));
}

#[tokio::test(flavor = "current_thread")]
async fn compiled_counter_ssr_hydrates_updates_and_disposes() {
    let source = format!(
        "import html.{{Html}}\n{COUNTER}\n#[extern(\"../../../support/web-harness.js\", \"checkCounter\")]\nfn checkCounter(factory: fn({{ initial: Number, label: String }}) Html) ()\npub fn main() {{ checkCounter(Counter) }}\n"
    );
    run_compiled_web(source).await;
}

async fn run_compiled_web(source: String) {
    run_compiled_web_with_modules(source, &[]).await;
}

async fn run_compiled_web_with_modules(source: String, modules: &[(&str, &str)]) {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/e2e/web/src/counter.ald");
    let uri = Url::from_file_path(&fixture).unwrap();
    let mut sources = vec![(uri.clone(), Ok(source))];
    for (name, source) in modules {
        sources.push((
            Url::from_file_path(fixture.with_file_name(name)).unwrap(),
            Ok((*source).to_owned()),
        ));
    }
    let result = build_fixture_sync(sources, BuildMode::Build, BuildDependencies::default());
    assert!(result.is_success(), "{result:?}");
    let entry = result.artifacts[&uri].module_id.clone();
    let code = alder_bundle::bundle(
        result.artifacts.into_values(),
        &entry,
        alder_bundle::EntryKind::Standalone,
    )
    .await
    .expect("counter and JS test bridge bundle");
    let status = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        alder_runtime::execute(code, vec![]),
    )
    .await
    .expect("web regression terminates")
    .expect("compiled counter passes lifecycle assertions");
    assert_eq!(status, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn compiled_setup_runs_once_and_derived_tracks_only_its_dependencies() {
    run_compiled_web(indoc::indoc! {r#"
        import html.{Html}
        #[extern("../../../support/web-harness.js", "recordSetup")]
        fn recordSetup(value: Number) Number
        #[extern("../../../support/web-harness.js", "recordDerived")]
        fn recordDerived(value: Number) Number
        #[extern("../../../support/web-harness.js", "checkReactivity")]
        fn checkReactivity(factory: fn(Number) Html) ()

        pub component Probe(initial: Number) {
            let start = recordSetup(initial)
            let count = state(start)
            let unrelated = state(0)
            let doubled = recordDerived(count)
            <div><button id="count" onClick={() -> { count += 1 }}>{count}</button><button id="other" onClick={() -> { unrelated += 1 }}>{unrelated}</button><button id="same" onClick={() -> { count = count }}>same</button><span>{doubled}</span></div>
        }

        pub fn main() { checkReactivity(Probe) }
    "#}.to_owned()).await;
}

macro_rules! web_error {
    ($name:ident, $mode:expr, $source:expr) => {
        #[test]
        fn $name() {
            let source = indoc::indoc!($source);
            assert_rendered_diagnostic_snapshot!(source, web_diagnostic(source, $mode));
        }
    };
}

#[tokio::test(flavor = "current_thread")]
async fn imported_components_render_through_the_typed_html_module() {
    let main = url("project/src/main.ald");
    let counter = url("project/src/counter.ald");
    let source = indoc::indoc! {r#"
        import (html, ~/counter, ~/counter.{Counter})
        pub fn main() {
            let first = html.renderToString(Counter({ initial: 1, label: "safe" }))
            let second = html.renderToString(counter::Counter({ initial: 1, label: "safe" }))
            assert first == second
            assert first == "<section title=\"safe\"><button><!--alder:text-->safe<!--/alder:text--></button><span title=\"Count 1\"><!--alder:text-->1<!--/alder:text--></span><strong><!--alder:text-->2<!--/alder:text--></strong></section>"
        }
    "#};
    let result = build_fixture_sync(
        vec![
            (main.clone(), Ok(source.to_owned())),
            (counter, Ok(COUNTER.to_owned())),
        ],
        BuildMode::Build,
        BuildDependencies::default(),
    );
    assert!(result.is_success(), "{result:?}");
    let entry = result.artifacts[&main].module_id.clone();
    let code = alder_bundle::bundle(
        result.artifacts.into_values(),
        &entry,
        alder_bundle::EntryKind::Standalone,
    )
    .await
    .unwrap();
    assert_eq!(alder_runtime::execute(code, vec![]).await.unwrap(), 0);
}

web_error!(
    string_attribute_rejects_number,
    BuildMode::Check,
    "pub component View() { <div title={42} /> }"
);
#[test]
fn event_objects_tasks_and_component_tags_compile() {
    for source in [
        "pub component View() { <button onClick={(event) -> ()} /> }",
        "pub component View() { <button onClick={() -> async { () }} /> }",
        "component Child() { <span /> }\npub component View() { <Child /> }",
    ] {
        let uri = url("project/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri, Ok(source.to_owned()))],
            BuildMode::Build,
            BuildDependencies::default(),
        );
        assert!(result.is_success(), "{source}: {result:?}");
    }
}
web_error!(
    duplicate_attribute_has_source_diagnostic,
    BuildMode::Check,
    "pub component View() { <div title=\"a\" title=\"b\" /> }"
);
web_error!(
    nested_button_has_source_diagnostic,
    BuildMode::Check,
    "pub component View() { <button><span><button /></span></button> }"
);
web_error!(
    unannotated_props_are_explicitly_unsupported,
    BuildMode::Build,
    "pub component View(props) { <div /> }"
);
web_error!(
    nested_state_is_explicitly_unsupported,
    BuildMode::Build,
    "pub component View() { <button onClick={() -> { let value = state(0) }} /> }"
);
web_error!(
    nested_state_mutation_is_explicitly_unsupported,
    BuildMode::Build,
    "pub component View() { let value = state({ count: 0 })\n<button onClick={() -> { value.count += 1 }}>{value.count}</button> }"
);
web_error!(
    props_are_read_only,
    BuildMode::Build,
    "pub component View(props: { count: Number }) { <button onClick={() -> { props.count += 1 }}>{props.count}</button> }"
);
web_error!(
    setup_bindings_are_read_only,
    BuildMode::Build,
    "pub component View() { let count = 0\n<button onClick={() -> { count += 1 }}>{count}</button> }"
);
web_error!(
    reactive_computations_cannot_write_state,
    BuildMode::Build,
    "pub component View() { let count = state(0)\nlet doubled = { count += 1\ncount * 2 }\n<span>{doubled}</span> }"
);
web_error!(
    setup_markup_remains_explicitly_unsupported,
    BuildMode::Build,
    "pub component View() { let child = <span />\n<div /> }"
);
