use super::*;
use crate::web_routes::{Route, RouteManifest, Segment, SourceFile};

fn source(uri: &Url) -> SourceFile {
    SourceFile {
        path: "routes/users/[id]/+server.ald".into(),
        uri: uri.to_string(),
    }
}
fn route(uri: &Url) -> Route {
    Route {
        id: "/users/[id]".to_owned(),
        segments: vec![
            Segment::Static("users".to_owned()),
            Segment::Param("id".to_owned()),
        ],
        page: None,
        page_server: None,
        endpoint: Some(source(uri)),
        layouts: vec![],
        errors: vec![],
        option_sources: vec![],
    }
}
fn solved(code: &str) -> (Url, InterfaceFile) {
    let uri = url("project/src/main.ald");
    let result = build_fixture_sync(
        vec![(uri.clone(), Ok(code.to_owned()))],
        BuildMode::Check,
        BuildDependencies::default(),
    );
    assert!(result.is_success(), "{code}\n{result:?}");
    (uri, result.interfaces.into_iter().next().unwrap())
}

#[test]
fn hooks_accept_generic_request_params_and_typed_fetch_continuations() {
    let (uri, interface) = solved(indoc::indoc! {r#"
        import http.{RequestEvent, Request, Response, Fetch, Error}
        pub fn handle(event, resolve) { resolve(event) }
        pub fn handleFetch(event: RequestEvent[p], request: Request, next: Fetch) Task[Result[Response, [:network_error(String)]]] { next(request) }
        pub async fn handleError(err: Error, event: RequestEvent[p]) { () }
    "#});
    let manifest = RouteManifest {
        hooks_server: Some(source(&uri)),
        ..Default::default()
    };
    let errors =
        crate::web_hooks::validate(&manifest, &BTreeMap::from([(uri.to_string(), interface)]));
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn hooks_reject_route_specific_params_and_wrong_effects() {
    for code in [
        "import http\nimport http.{RequestEvent, Response}\npub fn handle(event: RequestEvent[{ id: String }], resolve: fn(RequestEvent[{ id: String }]) Task[Response]) Response { http.text(event.params.id) }",
        "pub fn handle(event, resolve) { 42 }",
        "pub fn handleError(err: { name: Number }, event) { () }",
        "pub fn handleFetch(event, request, next) { 42 }",
    ] {
        let (uri, interface) = solved(code);
        let manifest = RouteManifest {
            hooks_server: Some(source(&uri)),
            ..Default::default()
        };
        assert!(
            !crate::web_hooks::validate(&manifest, &BTreeMap::from([(uri.to_string(), interface)]))
                .is_empty(),
            "{code}"
        );
    }
}

#[test]
fn hooks_client_init_and_reporting_accept_only_unit_effects() {
    let (uri, interface) = solved(
        "import http.{Error}\npub async fn init() { () }\npub fn handleError(err: Error) { () }",
    );
    let manifest = RouteManifest {
        hooks_client: Some(source(&uri)),
        ..Default::default()
    };
    assert!(
        crate::web_hooks::validate(&manifest, &BTreeMap::from([(uri.to_string(), interface)]))
            .is_empty()
    );
    let (uri, interface) = solved("pub fn init() { 42 }");
    assert_eq!(
        crate::web_hooks::validate(&manifest, &BTreeMap::from([(uri.to_string(), interface)]))
            .len(),
        1
    );
}

#[test]
fn hooks_endpoints_validate_concrete_params_and_native_response() {
    let (uri, interface) = solved(
        "import http\nimport http.{RequestEvent, Response}\npub fn get(event: RequestEvent[{ id: String }]) Response { http.text(event.params.id) }",
    );
    let manifest = RouteManifest {
        routes: vec![route(&uri)],
        ..Default::default()
    };
    assert!(
        crate::web_hooks::validate(&manifest, &BTreeMap::from([(uri.to_string(), interface)]))
            .is_empty()
    );
    let (uri, interface) = solved("pub fn get(event) { { body: \"not a response\" } }");
    assert_eq!(
        crate::web_hooks::validate(&manifest, &BTreeMap::from([(uri.to_string(), interface)]))
            .len(),
        1
    );
}

#[test]
fn hooks_error_boundary_preserves_union_and_rejects_partial_data_contract() {
    let (load_uri, load) = solved(
        "pub fn load(event) Result[{ name: String }, [:missing(String) | :denied]] { Err(:missing(\"name\")) }",
    );
    let error_uri = url("project/src/error.ald");
    let (_, boundary) = solved(
        "import http.{PageError}\npub component error(props: { error: PageError[[:missing(String) | :denied]] }) { <p>failed</p> }",
    );
    let mut route = route(&load_uri);
    route.option_sources.push(source(&load_uri));
    route.errors.push(source(&error_uri));
    route.endpoint = None;
    let manifest = RouteManifest {
        routes: vec![route],
        ..Default::default()
    };
    let mut interfaces = BTreeMap::from([
        (load_uri.to_string(), load),
        (error_uri.to_string(), boundary),
    ]);
    assert!(crate::web_hooks::validate(&manifest, &interfaces).is_empty());
    let (_, narrow) = solved(
        "import http.{PageError}\npub component error(props: { error: PageError[[:missing(String)]] }) { <p>failed</p> }",
    );
    interfaces.insert(error_uri.to_string(), narrow);
    assert_eq!(crate::web_hooks::validate(&manifest, &interfaces).len(), 1);
    let (_, partial) = solved(
        "pub component error(props: { data: { name: String } }) { <p>{props.data.name}</p> }",
    );
    interfaces.insert(error_uri.to_string(), partial);
    assert_eq!(crate::web_hooks::validate(&manifest, &interfaces).len(), 1);
}
