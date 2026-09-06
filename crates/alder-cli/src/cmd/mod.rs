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
        "#};
        std::fs::write(dependency.join("src/api.ald"), source).unwrap();
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
            pub async fn main() { assert(api.answer().await == 42) }
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

        assert!(root.join(".alder/interfaces/main.aldi").is_file());
        assert!(root.join(".alder/instances/application.aldi").is_file());
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
                pub fn render(value: Array[Token]) String { display(value) }
                pub fn render_badge(value: Badge) String { display(value) }
                pub fn main() {
                    assert(render([Token::Token]) == "array")
                    assert(render_badge(Badge::Badge) == "badge")
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
        std::fs::remove_dir_all(root).unwrap();
    }
}
