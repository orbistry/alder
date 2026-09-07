use std::path::{Path, PathBuf};

use miette::{IntoDiagnostic, Result, miette};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// File or directory to format
    #[arg(default_value = ".")]
    pub path: PathBuf,
    /// Report files that differ without writing them
    #[arg(long)]
    pub check: bool,
}

impl Args {
    pub async fn exec(self) -> Result<()> {
        super::Cmd::Fmt(self).exec().await
    }

    pub(super) async fn exec_with(self, output: &crate::reporting::Output) -> Result<()> {
        let files = alder_files(&self.path)?;
        let total = files.len();
        let mut changed = Vec::new();
        for path in files {
            output.detail("Checking", crate::reporting::display_path(&path));
            let source = tokio::fs::read_to_string(&path).await.into_diagnostic()?;
            let formatted = alder_fmt::format_source(&source)
                .map_err(|error| miette!("{}: {error}", path.display()))?;
            if source != formatted {
                changed.push((path, formatted));
            }
        }

        if self.check && !changed.is_empty() {
            let paths = changed
                .iter()
                .map(|(path, _)| format!("  {}", path.display()))
                .collect::<Vec<_>>()
                .join("\n");
            return Err(miette!(
                "{} file(s) need formatting:\n{paths}",
                changed.len()
            ));
        }
        if !self.check {
            for (path, formatted) in &changed {
                tokio::fs::write(path, formatted).await.into_diagnostic()?;
                output.detail("Formatted", crate::reporting::display_path(path));
            }
            output.status(
                "Formatted",
                format!(
                    "{} changed · {total} checked",
                    crate::reporting::quantity(changed.len(), "file")
                ),
            );
        } else {
            output.status(
                "Finished",
                format!(
                    "format check · {} correct",
                    crate::reporting::quantity(total, "file")
                ),
            );
        }
        Ok(())
    }
}

fn alder_files(path: &Path) -> Result<Vec<PathBuf>> {
    if path.is_file() {
        if path.extension().is_some_and(|extension| extension == "ald") {
            return Ok(vec![path.to_owned()]);
        }
        return Err(miette!("{} is not an .ald file", path.display()));
    }
    if !path.is_dir() {
        return Err(miette!("{} does not exist", path.display()));
    }

    let pattern = path.join("**/*.ald").to_string_lossy().into_owned();
    let mut files = glob::glob(&pattern)
        .into_diagnostic()?
        .collect::<Result<Vec<_>, _>>()
        .into_diagnostic()?;
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_file_is_discovered() {
        assert!(alder_files(Path::new("not-alder.txt")).is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn formatted_templates_keep_their_executed_values() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("alder-fmt-execute-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("alder.jsonc"),
            r#"{"type":"application","target":"standalone"}"#,
        )
        .unwrap();
        let path = root.join("src/main.ald");
        let source = indoc::indoc! {r#"
            pub async fn main() {
            let deferred = async {
            `start
            <spaces>
            end<spaces>`
            }
            let message = deferred.await
            assert(message == "start\n   \nend   ")
            assert(string.length(message) == 16)
            }
        "#}
        .replace("<spaces>", "   ");
        std::fs::write(&path, &source).unwrap();
        for check in [false, true] {
            Args {
                path: path.clone(),
                check,
            }
            .exec()
            .await
            .unwrap();
        }
        let formatted = std::fs::read_to_string(&path).unwrap();
        assert_ne!(formatted, source, "exercise an actual formatting change");
        let compiled =
            super::super::build::compile_ephemeral(&root, alder_driver::BuildMode::Build)
                .await
                .unwrap();
        assert!(compiled.result.is_success());
        let bundle = super::super::build::bundle(&compiled, alder_bundle::EntryKind::Standalone)
            .await
            .unwrap();
        assert_eq!(alder_runtime::execute(bundle, Vec::new()).await.unwrap(), 0);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(root.join("alder.jsonc")).unwrap();
        std::fs::remove_dir(root.join("src")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[tokio::test]
    async fn invalid_file_prevents_all_format_writes() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("alder-fmt-no-write-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let valid = root.join("a.ald");
        let invalid = root.join("b.ald");
        let source = indoc::indoc! {r#"
            fn main() {
            let value = 42
            }
        "#};
        std::fs::write(&valid, source).unwrap();
        std::fs::write(&invalid, "fn broken(").unwrap();
        let result = Args {
            path: root.clone(),
            check: false,
        }
        .exec()
        .await;
        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&valid).unwrap(), source);
        assert_eq!(std::fs::read_to_string(&invalid).unwrap(), "fn broken(");
        std::fs::remove_file(valid).unwrap();
        std::fs::remove_file(invalid).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
