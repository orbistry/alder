use super::*;

#[tokio::test(flavor = "current_thread")]
async fn forms_compile_native_submit_handler_and_read_urlencoded_fields() {
    let uri = url("project/src/main.ald");
    let source = indoc::indoc! {r#"
        import html
        import http
        import map
        async fn handle(values: Result[Map[String, Array[String]], [:invalid_form(String) | :file_field(String)]]) { () }
        pub component Form() {
            <form onSubmit={html.submit(handle)}><input name="name" /><button type="submit">Save</button></form>
        }
        pub async fn main() {
            let headers = map.new()
            map.set(headers, "content-type", "application/x-www-form-urlencoded")
            let request = http.request("https://example.test", {method: "POST", body: "name=Ada&tag=a&tag=b", headers: headers})
            let parsed = http.readForm(request).await
            let valid = match parsed { Ok(values) => map.get(values, "tag") == Some(["a", "b"]), Err(_) => false }
            assert valid
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

fn route(uri: &Url) -> crate::web_routes::Route {
    crate::web_routes::Route {
        id: "/signup".to_owned(),
        segments: vec![crate::web_routes::Segment::Static("signup".to_owned())],
        page: None,
        page_server: Some(crate::web_routes::SourceFile {
            path: "routes/signup/+page.server.ald".into(),
            uri: uri.to_string(),
        }),
        endpoint: None,
        layouts: vec![],
        errors: vec![],
        option_sources: vec![],
    }
}

#[test]
fn actions_metadata_preserves_solved_input_and_result_records() {
    let uri = url("project/src/main.ald");
    let source = indoc::indoc! {r#"
        async fn save(input: { name: String, tags: Array[String] }) Result[{ id: Number }, [:invalid_name(String)]] {
            if input.name == "" { Err(:invalid_name("required")) } else { Ok({ id: 42 }) }
        }
        pub let actions = { save: save }
    "#};
    let result = build_fixture_sync(
        vec![(uri.clone(), Ok(source.to_owned()))],
        BuildMode::Build,
        BuildDependencies::default(),
    );
    assert!(result.is_success(), "{result:?}");
    let actions =
        crate::web_actions::collect(&route(&uri), &result.interfaces[0], &result.interfaces)
            .unwrap()
            .unwrap();
    assert_eq!(actions.actions.len(), 1);
    assert_eq!(actions.actions[0].name, "save");
    assert_eq!(actions.actions[0].args_validator, "saveArgs");
    assert_eq!(actions.actions[0].result_validator, "saveResult");
    assert_eq!(
        actions.value.scheme,
        result.interfaces[0]
            .values
            .iter()
            .find(|value| value.exported_as == "actions")
            .unwrap()
            .scheme
    );
    let code = actions.client_stub().code();
    assert!(code.contains("$webAction"));
    assert!(code.contains("/signup"));
    assert!(code.contains("save"));
    assert!(actions.validators().code().contains("$webValidate"));
}

#[test]
fn actions_metadata_rejects_untyped_or_non_result_contracts() {
    for source in [
        "pub let actions = 42",
        "pub let actions = { save: 42 }",
        "async fn save(input: String) Result[Number, [:invalid]] { Ok(42) }\npub let actions = { save: save }",
        "fn save(input: { name: String }) Result[Number, [:invalid]] { Ok(42) }\npub let actions = { save: save }",
        "async fn save(input: { name: String }) Number { 42 }\npub let actions = { save: save }",
    ] {
        let uri = url("project/src/main.ald");
        let result = build_fixture_sync(
            vec![(uri.clone(), Ok(source.to_owned()))],
            BuildMode::Check,
            BuildDependencies::default(),
        );
        assert!(result.is_success(), "{source}\n{result:?}");
        assert!(
            crate::web_actions::collect(&route(&uri), &result.interfaces[0], &result.interfaces)
                .is_err(),
            "{source}"
        );
    }
}
