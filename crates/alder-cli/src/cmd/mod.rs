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
        let compiled = super::build::compile(&fixture(name), mode).await.unwrap();
        let bundle = super::build::bundle(&compiled.result, kind).await.unwrap();
        alder_runtime::execute(bundle, Vec::new()).await.unwrap()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn standalone_e2e_projects_execute() {
        for name in ["hello", "enums", "loops", "modules", "traits"] {
            assert_eq!(
                execute(name, BuildMode::Build, EntryKind::Standalone).await,
                0
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_declarations_execute() {
        assert_eq!(execute("tests", BuildMode::Test, EntryKind::Test).await, 0);
    }
}
