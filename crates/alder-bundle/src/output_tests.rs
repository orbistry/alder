use super::*;

fn module(id: &str, code: &str) -> EmittedModule {
    EmittedModule {
        module_id: id.into(),
        ast: rolldown_ecmascript::EcmaCompiler::parse(id, code, Default::default()).unwrap(),
        source_path: None,
        source_text: None,
        extern_regions: vec![],
        dependencies: vec![],
        store_keys: vec![],
    }
}

fn graph() -> Vec<EmittedModule> {
    vec![
        module(
            "alder:main",
            "export const first = () => import('alder:first'); export const second = () => import('alder:second');",
        ),
        module(
            "alder:first",
            "import { state } from 'alder:shared'; export const value = () => ++state.count;",
        ),
        module(
            "alder:second",
            "import { state } from 'alder:shared'; export const value = () => state.count;",
        ),
        module("alder:shared", "export const state = { count: 0 };"),
    ]
}

#[tokio::test]
async fn split_output_preserves_graph_and_explicit_entry() {
    let output = bundle_graph(graph(), "alder:main", BundleOptions::production())
        .await
        .unwrap();
    assert!(output.chunks.len() >= 4, "{output:#?}");
    let entry = output.entry("alder:main").unwrap();
    assert!(entry.is_entry);
    assert_eq!(entry.dynamic_imports.len(), 2);
    assert!(entry.filename.starts_with("entry-"));
    for chunk in output.chunks.values() {
        assert!(output.files.contains_key(&chunk.filename));
        for import in chunk.imports.iter().chain(&chunk.dynamic_imports) {
            assert!(output.files.contains_key(import), "missing {import}");
        }
    }
    assert!(output.into_single_code("alder:main").is_err());
}

#[tokio::test]
async fn production_hashes_are_deterministic_and_unrelated_routes_stay_stable() {
    let first = bundle_graph(graph(), "alder:main", BundleOptions::production())
        .await
        .unwrap();
    let mut reversed = graph();
    reversed.reverse();
    let second = bundle_graph(reversed, "alder:main", BundleOptions::production())
        .await
        .unwrap();
    assert_eq!(first, second);
    let mut changed = graph();
    changed[1] = module(
        "alder:first",
        "import { state } from 'alder:shared'; export const value = () => state.count += 10;",
    );
    let third = bundle_graph(changed, "alder:main", BundleOptions::production())
        .await
        .unwrap();
    let route = |output: &BundleOutput, id: &str| {
        output
            .chunks
            .values()
            .find(|chunk| chunk.facade_module_id.as_deref() == Some(id))
            .unwrap()
            .filename
            .clone()
    };
    assert_eq!(route(&first, "alder:second"), route(&third, "alder:second"));
    assert_ne!(route(&first, "alder:first"), route(&third, "alder:first"));
}

#[tokio::test]
async fn source_maps_are_complete_hidden_generated_javascript_maps() {
    let mut options = BundleOptions::production();
    options.sourcemap = true;
    let output = bundle_graph(graph(), "alder:main", options).await.unwrap();
    for chunk in output.chunks.values() {
        let map = chunk.sourcemap_filename.as_ref().unwrap();
        assert!(output.files.contains_key(map));
        assert!(chunk.sourcemap.as_ref().unwrap().contains("sourcesContent"));
        assert!(
            !String::from_utf8_lossy(&output.files[&chunk.filename]).contains("sourceMappingURL")
        );
    }
}

#[tokio::test]
async fn minified_source_map_resolves_expression_to_generated_source_line() {
    let output = bundle_graph(
        vec![module(
            "alder:mapped",
            "export function explode() {\n  const message = 'mapping-marker';\n  throw new Error(message);\n}\n",
        )],
        "alder:mapped",
        BundleOptions {
            sourcemap: true,
            ..BundleOptions::production()
        },
    )
    .await
    .unwrap();
    let entry = output.entry("alder:mapped").unwrap();
    let code = std::str::from_utf8(&output.files[&entry.filename]).unwrap();
    let offset = code
        .find("Error")
        .unwrap_or_else(|| panic!("missing Error expression: {code}"));
    let prefix = &code[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let column = prefix.rsplit('\n').next().unwrap().encode_utf16().count() as u32;
    let map =
        rolldown_sourcemap::SourceMap::from_json_string(entry.sourcemap.as_ref().unwrap()).unwrap();
    let table = map.generate_lookup_table();
    let token = map.lookup_source_view_token(&table, line, column).unwrap();
    assert!(token.get_source().unwrap().contains("alder:mapped"));
    let source = token.get_source_content().unwrap();
    let mapped_line = source.lines().nth(token.get_src_line() as usize).unwrap();
    assert!(mapped_line.contains("throw new Error"), "{mapped_line}");
    assert!(
        token.get_src_line() > 0,
        "must not map every expression to zero"
    );
    assert_eq!(
        output.files[entry.sourcemap_filename.as_ref().unwrap()],
        entry.sourcemap.as_ref().unwrap().as_bytes()
    );
}

#[tokio::test]
async fn single_file_adapter_rejects_inconsistent_public_output_without_panicking() {
    let mut output = bundle_graph(graph(), "alder:main", BundleOptions::default())
        .await
        .unwrap();
    let filename = output.entry("alder:main").unwrap().filename.clone();
    let bytes = output.files.remove(&filename).unwrap();
    output.files.insert("wrong-name.mjs".into(), bytes);
    assert!(matches!(
        output.into_single_code("alder:main"),
        Err(Error::InvalidOutput(_))
    ));
}

#[test]
fn output_rejects_unsafe_and_duplicate_artifact_names() {
    let asset = |name: &str| {
        rolldown_common::Output::Asset(Arc::new(rolldown_common::OutputAsset {
            filename: name.into(),
            names: vec![],
            original_file_names: vec![],
            source: rolldown_common::StrOrBytes::Bytes(vec![]),
        }))
    };
    for name in [
        "",
        "/absolute",
        "../escape",
        "a/../b",
        "a//b",
        "a\\b",
        "a?b",
        "a#b",
        "C:/escape",
        "a\0b",
    ] {
        assert!(
            BundleOutput::from_rolldown(vec![asset(name)]).is_err(),
            "{name:?}"
        );
    }
    assert!(BundleOutput::from_rolldown(vec![asset("same.bin"), asset("same.bin")]).is_err());
}

#[tokio::test]
async fn execution_adapter_explicitly_inlines_dynamic_imports() {
    let output = bundle_graph(graph(), "alder:main", BundleOptions::default())
        .await
        .unwrap();
    assert_eq!(output.chunks.len(), 1);
    assert!(output.into_single_code("alder:main").is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn minified_execution_preserves_exports_properties_and_shared_identity() {
    let mut modules = graph();
    modules[0] = module(
        "alder:main",
        r#"
        const first = await import('alder:first');
        const second = await import('alder:second');
        const value = { publicField: '  spaces\n retained  ', $: 'Tag', _0: 42 };
        if (first.value() !== 1 || second.value() !== 1) throw new Error('shared identity');
        if (JSON.stringify(value) !== '{"publicField":"  spaces\\n retained  ","$":"Tag","_0":42}') throw new Error('public properties');
        export { value as publicValue };
    "#,
    );
    let plain = bundle_graph(modules.clone(), "alder:main", BundleOptions::default())
        .await
        .unwrap()
        .into_single_code("alder:main")
        .unwrap();
    let minified = bundle_graph(
        modules,
        "alder:main",
        BundleOptions {
            minify: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(
        minified
            .entry("alder:main")
            .unwrap()
            .exports
            .contains(&"publicValue".to_owned())
    );
    let minified = minified.into_single_code("alder:main").unwrap();
    assert!(minified.len() < plain.len());
    assert_eq!(alder_runtime::execute(plain, vec![]).await.unwrap(), 0);
    assert_eq!(alder_runtime::execute(minified, vec![]).await.unwrap(), 0);
}

#[test]
fn binary_assets_and_provenance_are_preserved() {
    let output = BundleOutput::from_rolldown(vec![rolldown_common::Output::Asset(Arc::new(
        rolldown_common::OutputAsset {
            filename: "asset.bin".into(),
            names: vec!["binary".into()],
            original_file_names: vec!["original.bin".into()],
            source: rolldown_common::StrOrBytes::Bytes(vec![0, 255, 128]),
        },
    ))])
    .unwrap();
    assert_eq!(output.files["asset.bin"], [0, 255, 128]);
    assert_eq!(
        output.assets["asset.bin"].original_file_names,
        ["original.bin"]
    );
}
