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
        let files = alder_files(&self.path)?;
        let mut changed = Vec::new();
        for path in files {
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
            }
            eprintln!("Formatted {} file(s).", changed.len());
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
}
