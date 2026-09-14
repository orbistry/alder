use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::Duration,
};

use alder_config::Target;
use alder_driver::{BuildMode, Project};
use miette::{IntoDiagnostic, Result, miette};
use tokio::sync::{mpsc, watch};

#[derive(serde::Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(super) enum Event {
    Build {
        server: String,
        config: serde_json::Value,
    },
    Error {
        message: String,
    },
    Shutdown,
}

#[derive(clap::Args, Debug)]
pub struct Args {
    #[arg(default_value = ".")]
    pub path: PathBuf,
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
    #[arg(long, default_value_t = 3000)]
    pub port: u16,
}

impl Args {
    pub(super) async fn exec(self, output: &crate::reporting::Output) -> Result<()> {
        let project = Project::load(&self.path).await.into_diagnostic()?;
        let cloudflare = matches!(&project.config, alder_config::Config::Application(app) if app.target == Target::Cloudflare);
        let (sender, receiver) = mpsc::channel(8);
        let (stop, stopped) = watch::channel(false);
        let work = watch_project(
            project.root.clone(),
            if cloudflare {
                Target::Cloudflare
            } else {
                Target::Standalone
            },
            sender.clone(),
            stopped,
            output.clone(),
        );
        let server = async {
            if cloudflare {
                super::platform::serve_dev(receiver, &project.root, &self.host, self.port, output)
                    .await
            } else {
                serve_native(receiver, self.host, self.port).await
            }
        };
        tokio::pin!(work, server);
        tokio::select! {
            result = &mut server => { let _ = stop.send(true); work.await?; result?; }
            result = &mut work => { let _ = sender.send(Event::Shutdown).await; server.await?; result?; }
            signal = tokio::signal::ctrl_c() => {
                signal.into_diagnostic()?;
                let _ = stop.send(true);
                let _ = sender.send(Event::Shutdown).await;
                server.await?;
                work.await?;
            }
        }
        output.status("Stopped", "development server");
        Ok(())
    }
}

async fn serve_native(mut receiver: mpsc::Receiver<Event>, host: String, port: u16) -> Result<()> {
    let (sender, native) = mpsc::channel(8);
    let forward = async {
        while let Some(event) = receiver.recv().await {
            let shutdown = matches!(event, Event::Shutdown);
            let event = match event {
                Event::Build { server, .. } => alder_runtime::DevEvent::Build { server },
                Event::Error { message } => alder_runtime::DevEvent::Error { message },
                Event::Shutdown => alder_runtime::DevEvent::Shutdown,
            };
            if sender.send(event).await.is_err() || shutdown {
                break;
            }
        }
    };
    let runtime = alder_runtime::execute_dev(native, host, port);
    tokio::pin!(runtime, forward);
    let result = tokio::select! {
        result = &mut runtime => result,
        () = &mut forward => runtime.await,
    };
    result.map_err(|error| miette!(error.to_string()))?;
    Ok(())
}

async fn watch_project(
    root: PathBuf,
    target: Target,
    sender: mpsc::Sender<Event>,
    mut stopped: watch::Receiver<bool>,
    output: crate::reporting::Output,
) -> Result<()> {
    let mut previous = None;
    loop {
        if *stopped.borrow() {
            return Ok(());
        }
        let path = root.clone();
        let snapshot = tokio::task::spawn_blocking(move || source_snapshot(&path))
            .await
            .into_diagnostic()?
            .into_diagnostic()?;
        if previous.as_ref() != Some(&snapshot) {
            previous = Some(snapshot);
            let compiled = super::build::compile(&root, BuildMode::Build, &output)
                .await
                .and_then(|compiled| {
                    check_target(target, compiled.target)?;
                    Ok(compiled)
                });
            let event = match compiled {
                Ok(compiled) => match super::web::bundle_mode(&compiled, true).await {
                    Ok(artifacts) => build_event(
                        artifacts.server,
                        super::web::cloudflare_config(
                            &compiled,
                            None,
                            None,
                            super::web::COMPATIBILITY_DATE,
                        ),
                        &output,
                    ),
                    Err(error) => {
                        output.diagnostic(&error);
                        Event::Error {
                            message: format!("{error:?}"),
                        }
                    }
                },
                Err(error) => {
                    output.diagnostic(&error);
                    Event::Error {
                        message: format!("{error:?}"),
                    }
                }
            };
            if *stopped.borrow() {
                return Ok(());
            }
            if sender.send(event).await.is_err() {
                return Ok(());
            }
        }
        tokio::select! {
            _ = stopped.changed() => return Ok(()),
            _ = tokio::time::sleep(Duration::from_millis(300)) => {},
        }
    }
}

fn check_target(expected: Target, actual: Target) -> Result<()> {
    if expected != actual {
        return Err(miette!(
            "The application runtime target changed. Stop and restart alder dev to switch runtimes, or restore the previous target to resume this session."
        ));
    }
    Ok(())
}

fn build_event(
    server: String,
    config: Result<serde_json::Value>,
    output: &crate::reporting::Output,
) -> Event {
    match config {
        Ok(config) => {
            output.status("Ready", "web application updated");
            Event::Build { server, config }
        }
        Err(error) => {
            output.diagnostic(&error);
            Event::Error {
                message: format!("{error:?}"),
            }
        }
    }
}

/// Content fingerprints catch same-timestamp rewrites and route additions or
/// removals. Generated output and dependency caches never trigger rebuild loops.
fn source_snapshot(root: &Path) -> std::io::Result<BTreeMap<PathBuf, u64>> {
    let mut pending = vec![root.to_owned()];
    let mut result = BTreeMap::new();
    while let Some(directory) = pending.pop() {
        let entries = match std::fs::read_dir(&directory) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && directory != root => {
                continue;
            }
            result => result?,
        };
        for entry in entries {
            let entry = entry?;
            let kind = match entry.file_type() {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                result => result?,
            };
            let path = entry.path();
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if !matches!(
                    entry.file_name().to_str(),
                    Some(".git" | ".alder" | "dist" | "target" | "node_modules" | "elm")
                ) {
                    pending.push(path);
                }
            } else if kind.is_file() {
                let bytes = match std::fs::read(&path) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    result => result?,
                };
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                bytes.hash(&mut hasher);
                result.insert(path, hasher.finish());
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_changes_require_restart_but_restoring_target_is_valid() {
        for target in [Target::Standalone, Target::Cloudflare] {
            assert!(check_target(target, target).is_ok());
        }
        let error = check_target(Target::Standalone, Target::Cloudflare).unwrap_err();
        assert!(error.to_string().contains("restart alder dev"));
        assert!(check_target(Target::Standalone, Target::Standalone).is_ok());
    }

    #[test]
    fn configuration_failure_is_an_error_event_and_next_build_can_recover() {
        let output = crate::reporting::Output::silent();
        let failed = build_event(
            "broken".into(),
            Err(miette!("invalid platform configuration")),
            &output,
        );
        assert!(
            matches!(failed, Event::Error {message} if message.contains("invalid platform configuration"))
        );
        let recovered = build_event(
            "fixed".into(),
            Ok(serde_json::json!({"name":"valid"})),
            &output,
        );
        assert!(
            matches!(recovered, Event::Build {server, config} if server == "fixed" && config["name"] == "valid")
        );
    }

    #[test]
    fn public_file_edits_and_removals_change_the_watch_snapshot() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("public")).unwrap();
        let empty = source_snapshot(root.path()).unwrap();
        std::fs::write(root.path().join("public/style.css"), "old").unwrap();
        let first = source_snapshot(root.path()).unwrap();
        std::fs::write(root.path().join("public/style.css"), "new").unwrap();
        let second = source_snapshot(root.path()).unwrap();
        assert_ne!(first, second);
        assert_ne!(empty, second);
        std::fs::remove_file(root.path().join("public/style.css")).unwrap();
        assert_eq!(empty, source_snapshot(root.path()).unwrap());
    }
}
