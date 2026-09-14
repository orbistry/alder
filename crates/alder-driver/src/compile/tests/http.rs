use super::*;

#[tokio::test(flavor = "current_thread")]
async fn compiled_http_facade_preserves_native_objects_options_and_json_evidence() {
    let uri = url("project/src/main.ald");
    let source = indoc::indoc! {r#"
        import http
        import http.{Request, Response, RequestEvent}

        fn endpoint(event: RequestEvent[{ id: String }]) Response {
            http.text(event.params.id, { status: 201 })
        }

        pub async fn main() {
            let response = http.text("hello", { status: 201 })
            assert http.status(response) == 201
            assert http.responseHeader(response, "content-type") == Some("text/plain; charset=utf-8")
            assert http.responseText(response).await == Ok("hello")
            let json = http.json(42)
            assert http.responseText(json).await == Ok("42")
            let request: Request = http.request("https://example.test/", { method: "POST", body: "42" })
            let result: Result[Number, [:body_error(String) | :invalid_json(String)]] = http.readJson(request).await
            assert result == Ok(42)
            assert http.status(http.empty()) == 204
            assert http.responseHeader(http.redirect("/next"), "location") == Some("/next")
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

#[test]
fn http_response_and_request_types_are_not_interchangeable() {
    let uri = url("project/src/main.ald");
    let source = "import http\npub fn bad() { http.readText(http.text(\"hello\")) }";
    let result = build_fixture_sync(
        vec![(uri.clone(), Ok(source.to_owned()))],
        BuildMode::Check,
        BuildDependencies::default(),
    );
    assert!(!result.is_success());
    let ModuleResult::Failed { diagnostics } = &result.modules[&uri] else {
        panic!("{result:?}")
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message().contains("Request")
                && diagnostic.message().contains("Response")),
        "{diagnostics:?}"
    );
}
