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
    pub async fn exec(self, output: crate::reporting::Output) -> miette::Result<()> {
        output.begin();

        let operation = match &self {
            Cmd::Build(_) => "build",
            Cmd::Check(_) => "check",
            Cmd::Fmt(_) => "fmt",
            Cmd::Lsp(_) => "lsp",
            Cmd::Run(_) => "run",
            Cmd::Test(_) => "test",
        };

        let started = std::time::Instant::now();

        let result = match self {
            Cmd::Build(args) => args.exec(&output).await,
            Cmd::Check(args) => args.exec(&output).await,
            Cmd::Fmt(args) => args.exec(&output).await,
            Cmd::Lsp(args) => lsp::exec(args).await,
            Cmd::Run(args) => args.exec(&output).await,
            Cmd::Test(args) => args.exec(&output).await,
        };

        match result {
            Ok(()) => {
                if matches!(operation, "build" | "check") {
                    output.finish(operation, started.elapsed());
                }

                Ok(())
            }
            Err(error) => {
                if !output.failure_reported() {
                    output.diagnostic(&error);
                    output.failure(operation, started.elapsed());
                }

                Err(error)
            }
        }
    }
}
