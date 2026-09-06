pub mod build;
pub mod check;
pub mod fmt;
pub mod lsp;
pub mod run;
pub mod test;

#[derive(clap::Subcommand)]
pub enum Cmd {
    /// Build a bundled ESM artifact
    Build(build::Args),
    /// Check a Alder project for errors
    #[clap(visible_alias = "c")]
    Check(check::Args),
    /// Format Alder source files
    Fmt(fmt::Args),
    /// Start the Alder language server over stdio
    Lsp(lsp::Args),
    /// Build and execute a standalone application
    Run(run::Args),
    /// Build and run Alder test declarations
    Test(test::Args),
}

impl Cmd {
    pub async fn exec(self) -> miette::Result<()> {
        match self {
            Cmd::Build(args) => args.exec().await,
            Cmd::Check(args) => args.exec().await,
            Cmd::Fmt(args) => args.exec().await,
            Cmd::Lsp(args) => lsp::exec(args).await,
            Cmd::Run(args) => args.exec().await,
            Cmd::Test(args) => args.exec().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use alder_bundle::EntryKind;
    use alder_driver::BuildMode;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/e2e")
            .join(name)
    }

    async fn execute(name: &str, mode: BuildMode, kind: EntryKind) -> i32 {
        let compiled = super::build::compile_ephemeral(&fixture(name), mode)
            .await
            .unwrap();
        let bundle = super::build::bundle(&compiled.result, kind).await.unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            alder_runtime::execute(bundle, Vec::new()),
        )
        .await
        .unwrap_or_else(|_| panic!("{name}: execution did not finish within 30 seconds"))
        .unwrap()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_runner_reports_failure_for_sync_and_async_tests() {
        assert_eq!(
            execute("test_failures", BuildMode::Test, EntryKind::Test).await,
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn hash_superclass_equality_agrees_with_ordinary_equality() {
        assert_eq!(
            execute("hash_equality", BuildMode::Build, EntryKind::Standalone).await,
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn refutable_bindings_fail_before_exposing_invalid_payloads() {
        let compiled =
            super::build::compile_ephemeral(&fixture("pattern_bindings"), BuildMode::Build)
                .await
                .unwrap();
        let bundle = super::build::bundle(&compiled.result, EntryKind::Standalone)
            .await
            .unwrap();
        for argument in [
            "let",
            "parameter",
            "lambda",
            "loop",
            "enum",
            "array",
            "top",
            "nested",
            "async_parameter",
            "async_body",
        ] {
            let error = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(bundle.clone(), vec![argument.to_owned()]),
            )
            .await
            .expect("failed binding execution must finish")
            .expect_err("a failed binding must not execute its continuation");
            let message = error.to_string();
            assert!(
                message.contains("Non-exhaustive match at alder://"),
                "{argument}: {message}"
            );
        }
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(bundle, vec!["success".to_owned()]),
            )
            .await
            .expect("binding success and cleanup checks must finish")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn explicit_async_laziness_capture_and_nested_tasks_execute() {
        assert_eq!(
            execute("explicit_async", BuildMode::Build, EntryKind::Standalone).await,
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn standalone_e2e_projects_execute() {
        for name in [
            "hello",
            "enums",
            "loops",
            "modules",
            "errors",
            "async",
            "externs",
            "control_flow",
            "records",
            "traits",
            "docs_traits",
        ] {
            assert_eq!(
                execute(name, BuildMode::Build, EntryKind::Standalone).await,
                0
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn option_record_defaults_execute() {
        assert_eq!(
            execute("record_options", BuildMode::Build, EntryKind::Standalone).await,
            0,
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn artifact_insertion_order_preserves_bundles_and_initialization() {
        let mut previous = None;
        for iteration in 0..6 {
            let mut compiled =
                super::build::compile_ephemeral(&fixture("traits"), BuildMode::Build)
                    .await
                    .unwrap();
            let mut artifacts = compiled.result.artifacts.drain().collect::<Vec<_>>();
            artifacts.sort_by(|(left, _), (right, _)| left.cmp(right));
            let length = artifacts.len();
            assert!(length > 1, "the fixture must exercise multiple modules");
            artifacts.rotate_left(iteration % length);
            if iteration % 2 == 0 {
                artifacts.reverse();
            }
            compiled.result.artifacts = artifacts.into_iter().collect();
            let bundle = super::build::bundle(&compiled.result, EntryKind::Standalone)
                .await
                .unwrap();
            if let Some(previous) = &previous {
                assert_eq!(&bundle, previous, "bundle changed on iteration {iteration}");
            }
            // The fixture asserts facade initialization occurs exactly once,
            // including unused sibling imports in reverse filename order.
            let exit = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                alder_runtime::execute(bundle.clone(), Vec::new()),
            )
            .await
            .expect("initialization regression exceeded its execution bound")
            .unwrap();
            assert_eq!(exit, 0);
            previous = Some(bundle);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn local_promise_extern_defects_retain_their_context() {
        let compiled = super::build::compile_ephemeral(&fixture("externs"), BuildMode::Build)
            .await
            .unwrap();
        let bundle = super::build::bundle(&compiled.result, EntryKind::Standalone)
            .await
            .unwrap();
        for (argument, symbol) in [("throw", "throws"), ("reject", "rejects")] {
            let error = alder_runtime::execute(bundle.clone(), vec![argument.to_owned()])
                .await
                .unwrap_err();
            let message = error.to_string();
            assert!(message.contains("wrapper failure"), "{message}");
            assert!(
                message.contains(&format!("extern ./client.js:{symbol}")),
                "{message}"
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dependency_extern_uses_its_own_sibling_wrapper() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "alder-extern-dependency-{}-{nonce}",
            std::process::id()
        ));
        let dependency = root.join("package");
        let app = root.join("application");
        for project in [&dependency, &app] {
            std::fs::create_dir_all(project.join("src")).unwrap();
        }
        std::fs::write(
            dependency.join("alder.jsonc"),
            indoc::indoc! {r#"
            { "type": "package", "name": "vendor/wrapper", "version": "0.1.0",
              "summary": "Extern fixture", "license": "MIT", "target": "standalone" }
        "#},
        )
        .unwrap();
        let source = indoc::indoc! {r#"
            #[extern("./client.js", "answer")]
            pub fn answer() Task[Number]
            pub let events: Array[Number] = []
        "#};
        std::fs::write(dependency.join("src/api.ald"), source).unwrap();
        std::fs::write(
            dependency.join("src/facade.ald"),
            indoc::indoc! {r#"
                pub import ~/api.{ answer as read, events as shared }
                let initialized = Array.push(shared, 1)
            "#},
        )
        .unwrap();
        std::fs::write(
            dependency.join("src/mod.ald"),
            indoc::indoc! {r#"
                pub import ~/facade.*
                let initialized = Array.push(shared, 2)
            "#},
        )
        .unwrap();
        std::fs::write(
            dependency.join("src/client.js"),
            "export function answer() { return Promise.resolve(42); }",
        )
        .unwrap();
        super::build::compile(&dependency, BuildMode::Check)
            .await
            .unwrap();
        std::fs::write(
            app.join("alder.jsonc"),
            indoc::indoc! {r#"
            { "type": "application", "target": "standalone",
              "dependencies": { "vendor/wrapper": { "path": "../package" } } }
        "#},
        )
        .unwrap();
        std::fs::write(
            app.join("src/client.js"),
            "export function answer() { return Promise.resolve(99); }",
        )
        .unwrap();
        std::fs::write(
            app.join("src/main.ald"),
            indoc::indoc! {r#"
            import @vendor/wrapper/api
            import @vendor/wrapper.{ read, shared }
            import @vendor/wrapper/facade
            pub async fn main() {
                assert(api.answer().await == 42)
                assert(read().await == 42 && facade.read().await == 42)
                assert(shared == [1, 2])
                Array.push(shared, 3)
                assert(api.events == [1, 2, 3] && facade.shared == [1, 2, 3])
            }
        "#},
        )
        .unwrap();
        let compiled = super::build::compile_ephemeral(&app, BuildMode::Build)
            .await
            .unwrap();
        let bundle = super::build::bundle(&compiled.result, EntryKind::Standalone)
            .await
            .unwrap();
        assert_eq!(alder_runtime::execute(bundle, Vec::new()).await.unwrap(), 0);

        std::fs::remove_file(dependency.join("src/client.js")).unwrap();
        let error = super::build::bundle(&compiled.result, EntryKind::Standalone)
            .await
            .unwrap_err();
        let mut rendered = String::new();
        miette::GraphicalReportHandler::new_themed(miette::GraphicalTheme::unicode_nocolor())
            .render_report(&mut rendered, error.as_ref())
            .unwrap();
        assert!(
            rendered.contains("pub fn answer() Task[Number]"),
            "{rendered}"
        );
        assert!(rendered.contains("./client.js"), "{rendered}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runnable_traits_documentation_matches_its_fixture() {
        let docs = include_str!("../../../../docs/language.md");
        let traits = docs
            .split_once("### Traits\n")
            .expect("language guide has a Traits section")
            .1;
        let example = traits
            .split_once("```alder\n")
            .expect("Traits section has an Alder example")
            .1
            .split_once("\n```")
            .expect("Traits example fence is closed")
            .0;
        let fixture = include_str!("../../../../tests/e2e/docs_traits/src/main.ald");
        assert_eq!(example.trim_end(), fixture.trim_end());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_declarations_execute() {
        assert_eq!(execute("tests", BuildMode::Test, EntryKind::Test).await, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn successful_build_artifacts_are_persisted_outside_the_source_tree() {
        let compiled = super::build::compile_ephemeral(&fixture("traits"), BuildMode::Check)
            .await
            .unwrap();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "alder-cli-interface-test-{}-{nonce}",
            std::process::id()
        ));
        super::build::persist_semantic_artifacts(&root, &compiled.result).unwrap();

        assert!(
            root.join(".alder/interfaces/application/main.aldi")
                .is_file()
        );
        assert!(root.join(".alder/instances/application.aldi").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn source_dependency_builds_without_saved_interfaces() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "alder-cli-cold-dependency-{}-{nonce}",
            std::process::id()
        ));
        for directory in ["src", "widgets/src"] {
            std::fs::create_dir_all(root.join(directory)).unwrap();
        }
        for (path, source) in [
            (
                "alder.jsonc",
                indoc::indoc! {r#"
                    { "type": "application", "target": "standalone",
                      "dependencies": { "vendor/widgets": { "path": "widgets" } } }
                "#},
            ),
            (
                "widgets/alder.jsonc",
                indoc::indoc! {r#"
                    { "type": "package", "name": "vendor/widgets",
                      "version": "0.1.0", "summary": "Cold dependency fixture",
                      "license": "MIT", "target": "standalone" }
                "#},
            ),
            (
                "widgets/src/api.ald",
                indoc::indoc! {r#"
                    pub enum Token { Token }
                    pub trait Display[a] { fn display(value: a) String }
                "#},
            ),
            (
                "widgets/src/instances.ald",
                indoc::indoc! {r#"
                    import ~/api.{ Token, Display }
                    impl Display[Token] {
                        fn display(value: Token) String { "cold token" }
                    }
                "#},
            ),
            (
                "src/main.ald",
                indoc::indoc! {r#"
                    import @vendor/widgets/api.{ Token, display }
                    pub fn main() { assert(display(Token::Token) == "cold token") }
                "#},
            ),
        ] {
            std::fs::write(root.join(path), source).unwrap();
        }
        assert!(!root.join("widgets/.alder").exists());
        let compiled = super::build::compile_ephemeral(&root, BuildMode::Build)
            .await
            .unwrap();
        let bundle = super::build::bundle(&compiled.result, EntryKind::Standalone)
            .await
            .unwrap();
        let exit = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            alder_runtime::execute(bundle, Vec::new()),
        )
        .await
        .expect("cold dependency execution timed out")
        .unwrap();
        assert_eq!(exit, 0);
        assert!(!root.join("widgets/.alder").exists());
        std::fs::create_dir_all(root.join("core/src")).unwrap();
        for (path, source) in [
            (
                "widgets/alder.jsonc",
                indoc::indoc! {r#"
                    { "type": "package", "name": "vendor/widgets",
                      "version": "0.1.0", "summary": "Transitive dependency fixture",
                      "license": "MIT", "target": "standalone",
                      "dependencies": { "vendor/core": { "path": "../core" } } }
                "#},
            ),
            (
                "core/alder.jsonc",
                indoc::indoc! {r#"
                    { "type": "package", "name": "vendor/core",
                      "version": "0.1.0", "summary": "Transitive dependency leaf",
                      "license": "MIT", "target": "standalone" }
                "#},
            ),
            (
                "core/src/api.ald",
                "pub fn label() String { \"cold token\" }",
            ),
            (
                "widgets/src/instances.ald",
                indoc::indoc! {r#"
                    import ~/api.{ Token, Display }
                    import @vendor/core/api.{ label }
                    impl Display[Token] {
                        fn display(value: Token) String { label() }
                    }
                "#},
            ),
        ] {
            std::fs::write(root.join(path), source).unwrap();
        }
        let compiled = super::build::compile_ephemeral(&root, BuildMode::Build)
            .await
            .expect("dependency imports must use the dependency's own configuration");
        let bundle = super::build::bundle(&compiled.result, EntryKind::Standalone)
            .await
            .unwrap();
        let exit = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            alder_runtime::execute(bundle, Vec::new()),
        )
        .await
        .expect("transitive dependency execution timed out")
        .unwrap();
        assert_eq!(exit, 0);
        std::fs::create_dir_all(root.join("other-core/src")).unwrap();
        std::fs::copy(
            root.join("core/alder.jsonc"),
            root.join("other-core/alder.jsonc"),
        )
        .unwrap();
        std::fs::copy(
            root.join("core/src/api.ald"),
            root.join("other-core/src/api.ald"),
        )
        .unwrap();
        std::fs::write(
            root.join("alder.jsonc"),
            indoc::indoc! {r#"
                { "type": "application", "target": "standalone",
                  "dependencies": {
                    "vendor/widgets": { "path": "widgets" },
                    "vendor/core": { "path": "other-core" }
                  } }
            "#},
        )
        .unwrap();
        std::fs::write(
            root.join("src/main.ald"),
            indoc::indoc! {r#"
                import @vendor/widgets/api.{ Token, display }
                import @vendor/core/api.{ label }
                pub fn main() { assert(display(Token::Token) == label()) }
            "#},
        )
        .unwrap();
        let error = super::build::compile_ephemeral(&root, BuildMode::Build)
            .await
            .err()
            .expect("two dependency roots cannot share one package identity");
        let rendered = error.to_string();
        for directory in ["core", "other-core"] {
            let path = root.join(directory).canonicalize().unwrap();
            assert!(
                rendered.contains(path.to_string_lossy().as_ref()),
                "{rendered}"
            );
        }
        std::fs::write(
            root.join("alder.jsonc"),
            indoc::indoc! {r#"
                { "type": "application", "target": "standalone",
                  "dependencies": {
                    "vendor/widgets": { "path": "widgets" },
                    "vendor/core": { "path": "core/../core" }
                  } }
            "#},
        )
        .unwrap();
        let compiled = super::build::compile_ephemeral(&root, BuildMode::Build)
            .await
            .expect("equivalent paths to one dependency root must coalesce");
        assert_eq!(
            compiled
                .result
                .artifacts
                .values()
                .filter(|artifact| artifact.module_id == "alder://pkg/vendor/core/api.mjs")
                .count(),
            1
        );
        std::fs::write(
            root.join("core/alder.jsonc"),
            indoc::indoc! {r#"
                { "type": "package", "name": "vendor/core",
                  "version": "0.1.0", "summary": "Cyclic dependency fixture",
                  "license": "MIT", "target": "standalone",
                  "dependencies": { "vendor/widgets": { "path": "../widgets" } } }
            "#},
        )
        .unwrap();
        std::fs::write(
            root.join("core/src/api.ald"),
            indoc::indoc! {r#"
                import @vendor/widgets/instances
                pub fn label() String { "cold token" }
            "#},
        )
        .unwrap();
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            super::build::compile_ephemeral(&root, BuildMode::Build),
        )
        .await
        .expect("cyclic dependency discovery must terminate")
        .err()
        .expect("a cross-package module import cycle must be rejected");
        assert!(error.to_string().contains("import cycle"), "{error:?}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cli_loads_a_path_dependency_package_instance_index() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "alder-cli-dependency-test-{}-{nonce}",
            std::process::id()
        ));
        let dependency = root.join("widgets");
        let application = root.join("app");
        std::fs::create_dir_all(dependency.join("src")).unwrap();
        std::fs::create_dir_all(application.join("src")).unwrap();
        std::fs::write(
            dependency.join("alder.jsonc"),
            indoc::indoc! {r#"
                {
                  "type": "package",
                  "name": "vendor/widgets",
                  "version": "0.1.0",
                  "summary": "Trait fixtures",
                  "license": "MIT",
                  "target": "standalone"
                }
            "#},
        )
        .unwrap();
        std::fs::write(
            dependency.join("src/api.ald"),
            indoc::indoc! {r#"
                pub enum Token { Token }
                pub enum Badge { Badge }
                pub trait Display[a] {
                    fn display(value: a) String
                }
            "#},
        )
        .unwrap();
        std::fs::write(
            dependency.join("src/instances.ald"),
            indoc::indoc! {r#"
                import ~/api.{ Token, Display }
                impl Display[Array[a]] where a: Display {
                    fn display(value: Array[a]) String { "array" }
                }
                impl Display[Token] {
                    fn display(value: Token) String { "token" }
                }
            "#},
        )
        .unwrap();
        std::fs::write(
            dependency.join("src/alternate.ald"),
            indoc::indoc! {r#"
                import ~/api.{ Badge, Display }
                impl Display[Badge] {
                    fn display(value: Badge) String { "badge" }
                }
            "#},
        )
        .unwrap();
        std::fs::write(
            dependency.join("src/facade.ald"),
            indoc::indoc! {r#"
                pub import ~/api.{ Token as Item, Badge, Display as Render, display as show }
            "#},
        )
        .unwrap();
        std::fs::write(dependency.join("src/mod.ald"), "pub import ~/facade.*").unwrap();
        super::build::compile(&dependency, BuildMode::Check)
            .await
            .unwrap();

        std::fs::write(
            application.join("alder.jsonc"),
            indoc::indoc! {r#"
                {
                  "type": "application",
                  "target": "standalone",
                  "dependencies": {
                    "vendor/widgets": { "path": "../widgets" }
                  }
                }
            "#},
        )
        .unwrap();
        std::fs::write(
            application.join("src/main.ald"),
            indoc::indoc! {r#"
                import @vendor/widgets/api.{ Badge, Token, display }
                import @vendor/widgets.{ Item, Render, show }
                pub fn render(value: Array[Token]) String { display(value) }
                pub fn render_badge(value: Badge) String { display(value) }
                pub fn renamed(value: a) String where a: Render { Render::display(value) }
                pub fn main() {
                    assert(render([Token::Token]) == "array")
                    assert(render_badge(Badge::Badge) == "badge")
                    assert(show(Item::Token) == "token")
                    assert(renamed([Item::Token]) == "array")
                    assert(renamed(Badge::Badge) == "badge")
                }
            "#},
        )
        .unwrap();
        let compiled = super::build::compile_ephemeral(&application, BuildMode::Build)
            .await
            .unwrap();

        assert!(compiled.result.is_success());
        assert!(
            compiled
                .result
                .artifacts
                .values()
                .any(|artifact| artifact.module_id == "alder://pkg/vendor/widgets/instances.mjs")
        );
        let application_module = compiled
            .result
            .artifacts
            .values()
            .find(|artifact| artifact.module_id == "alder://app/main.mjs")
            .unwrap();
        assert!(
            application_module
                .dependencies
                .contains(&"alder://pkg/vendor/widgets/instances.mjs".to_owned())
        );
        assert!(
            application_module
                .dependencies
                .contains(&"alder://pkg/vendor/widgets/alternate.mjs".to_owned())
        );
        let bundle = super::build::bundle(&compiled.result, EntryKind::Standalone)
            .await
            .unwrap();
        assert_eq!(alder_runtime::execute(bundle, Vec::new()).await.unwrap(), 0);
        // Keep the saved package interfaces, but change executable behavior
        // without changing the public contract. A subsequent application build
        // must emit the current dependency source, not reuse an older body.
        std::fs::write(
            dependency.join("src/instances.ald"),
            indoc::indoc! {r#"
                import ~/api.{ Token, Display }
                impl Display[Array[a]] where a: Display {
                    fn display(value: Array[a]) String { "array" }
                }
                impl Display[Token] {
                    fn display(value: Token) String { "updated token" }
                }
            "#},
        )
        .unwrap();
        std::fs::write(
            application.join("src/main.ald"),
            indoc::indoc! {r#"
                import @vendor/widgets/api.{ Token, display }
                pub fn main() {
                    assert(display(Token::Token) == "updated token")
                }
            "#},
        )
        .unwrap();
        let rebuilt = super::build::compile_ephemeral(&application, BuildMode::Build)
            .await
            .unwrap();
        let bundle = super::build::bundle(&rebuilt.result, EntryKind::Standalone)
            .await
            .unwrap();
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(30),
                alder_runtime::execute(bundle, Vec::new()),
            )
            .await
            .expect("rebuilt dependency execution timed out")
            .unwrap(),
            0
        );
        // A removed implementation must not survive through the saved package
        // index and make a consumer pass type checking with nonexistent evidence.
        std::fs::write(
            dependency.join("src/instances.ald"),
            indoc::indoc! {r#"
                import ~/api.{ Display }
                impl Display[Array[a]] where a: Display {
                    fn display(value: Array[a]) String { "array" }
                }
            "#},
        )
        .unwrap();
        let error = super::build::compile_ephemeral(&application, BuildMode::Build)
            .await
            .err()
            .expect("a saved index must not resurrect a removed source implementation");
        assert!(error.to_string().contains("no implementation"), "{error:?}");
        std::fs::remove_dir_all(root).unwrap();
    }
}
