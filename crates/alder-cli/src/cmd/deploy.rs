use std::{path::PathBuf, process::Stdio};

use alder_config::Target;
use alder_driver::BuildMode;
use miette::{IntoDiagnostic, Result, miette};

#[derive(clap::Args, Debug)]
pub struct Args {
    #[arg(default_value = ".")]
    pub path: PathBuf,
    /// Exact Worker name. Required for a real deployment; never inferred.
    #[arg(long, required_unless_present = "dry_run")]
    pub name: Option<String>,
    /// Exact destination account ID. Credentials stay in Wrangler's auth store/environment.
    #[arg(long, required_unless_present = "dry_run")]
    pub account_id: Option<String>,
    #[arg(long, default_value = super::web::COMPATIBILITY_DATE)]
    pub compatibility_date: String,
    /// Validate and package locally without uploading or modifying a Worker.
    #[arg(long)]
    pub dry_run: bool,
    /// Explicit historical Durable Object migration records (a JSON array).
    #[arg(long)]
    pub migrations: Option<PathBuf>,
}

impl Args {
    pub(super) async fn exec(self, output: &crate::reporting::Output) -> Result<()> {
        let compiled = super::build::compile(&self.path, BuildMode::Build, output).await?;
        if compiled.target != Target::Cloudflare || compiled.web.is_none() {
            return Err(miette!(
                "alder deploy requires a Cloudflare application with filesystem routes"
            ));
        }
        let support = super::platform::support_directory()?;
        let mut config = super::web::cloudflare_config(
            &compiled,
            self.name,
            self.account_id,
            &self.compatibility_date,
        )?;
        if let Some(path) = self.migrations {
            let path = if path.is_absolute() {
                path
            } else {
                compiled.root.join(path)
            };
            let records: Vec<serde_json::Value> =
                serde_json::from_slice(&tokio::fs::read(path).await.into_diagnostic()?)
                    .into_diagnostic()?;
            config = alder_driver::cloudflare::wrangler_config(
                &compiled.cloudflare,
                &alder_driver::cloudflare::ConfigOptions {
                    name: config["name"]
                        .as_str()
                        .expect("validated Worker name")
                        .to_owned(),
                    compatibility_date: self.compatibility_date,
                    main: "worker.mjs".to_owned(),
                    assets_directory: "client".to_owned(),
                    account_id: config["account_id"].as_str().map(str::to_owned),
                    legacy_migrations: Some(records),
                },
            )
            .map_err(|errors| miette!("{}", errors.join("\n")))?;
        }
        super::web::write_build(&compiled, output).await?;
        let path = compiled.root.join("dist/wrangler.jsonc");
        tokio::fs::write(
            &path,
            serde_json::to_string_pretty(&config).into_diagnostic()?,
        )
        .await
        .into_diagnostic()?;
        output.stage(if self.dry_run {
            "deployment validation"
        } else {
            "deployment"
        });
        output.status(
            if self.dry_run {
                "Checking deployment"
            } else {
                "Deploying"
            },
            config["name"].as_str().expect("validated Worker name"),
        );
        let mut command = tokio::process::Command::new("node");
        command
            .arg(support.join("node_modules/wrangler/bin/wrangler.js"))
            .args(["deploy", "--config"])
            .arg(&path)
            .args(["--no-bundle", "--keep-vars", "--autoconfig=false"])
            .env("WRANGLER_SEND_METRICS", "false")
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        if self.dry_run {
            command
                .arg("--dry-run")
                .arg("--outdir")
                .arg(compiled.root.join("dist/deploy-check"));
        }
        let status = command.status().await.into_diagnostic()?;
        if !status.success() {
            return Err(miette!(
                "Wrangler exited with {status}; deployment was not confirmed"
            ));
        }
        output.status(
            if self.dry_run {
                "Validated"
            } else {
                "Deployed"
            },
            crate::reporting::display_path(&path),
        );
        Ok(())
    }
}
