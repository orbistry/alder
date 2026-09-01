use std::path::PathBuf;

use miette::{IntoDiagnostic, Result, miette};

/// Set on the child when the proxy execs a cached binary, so the child knows
/// it was already proxied and must not proxy again. Without this, a cached
/// binary whose version doesn't match its folder name would exec itself
/// forever, silently.
const PROXY_ENV_VAR: &str = "ALDER_PROXY_VERSION";

/// Check whether a parent `alder` process already proxied to this one.
///
/// Returns `true` if so, meaning [`maybe_proxy`] must be skipped. Errors if
/// this binary's version doesn't match the version the parent resolved from
/// `alder.jsonc` — that means the cached binary is not what its folder name
/// claims.
///
/// Must be called before the tokio runtime starts: it mutates the process
/// environment, which is only sound while the process is single-threaded.
pub fn proxy_guard() -> Result<bool> {
    let Ok(expected) = std::env::var(PROXY_ENV_VAR) else {
        return Ok(false);
    };

    // SAFETY: called before the tokio runtime exists, so the process is
    // still single-threaded.
    unsafe { std::env::remove_var(PROXY_ENV_VAR) };

    if expected != crate::VERSION {
        return Err(miette!(
            "The cached binary for alder {expected} reports version {}.\n\
             Delete {} and try again.",
            crate::VERSION,
            cached_binary_dir(&expected).display()
        ));
    }

    Ok(true)
}

/// Walk up from cwd looking for `alder.jsonc`. If the project requires a different
/// compiler version, exec the correct binary (downloading it first if needed).
pub async fn maybe_proxy() -> Result<()> {
    let cwd = std::env::current_dir().into_diagnostic()?;

    let config_path = match find_config(&cwd) {
        Some(p) => p,
        None => return Ok(()),
    };

    let config = alder_config::parse_file(&config_path).into_diagnostic()?;

    let required = match config.compiler() {
        Some(v) if v != crate::VERSION => v,
        _ => return Ok(()),
    };

    let cache_dir = cached_binary_dir(required);
    let binary = cached_binary_path(required);

    if binary.exists() {
        exec_cached_binary(&binary, required);
    }

    crate::download::download_version(required, &cache_dir).await?;
    exec_cached_binary(&binary, required);
}

/// Search for `alder.jsonc` by walking up from `start`.
fn find_config(start: &std::path::Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let candidate = dir.join(alder_config::CONFIG_FILE_NAME);
        if candidate.exists() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn cached_binary_dir(version: &str) -> PathBuf {
    let base = std::env::var("ALDER_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("alder")
        });
    base.join("versions").join(version)
}

fn cached_binary_path(version: &str) -> PathBuf {
    let dir = cached_binary_dir(version);
    if cfg!(windows) {
        dir.join("alder.exe")
    } else {
        dir.join("alder")
    }
}

/// Replace this process with the cached binary, forwarding all args.
///
/// `version` is passed via [`PROXY_ENV_VAR`] so the child knows it was
/// proxied and refuses to proxy again.
fn exec_cached_binary(binary: &std::path::Path, version: &str) -> ! {
    let args: Vec<String> = std::env::args().skip(1).collect();

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(binary)
            .args(&args)
            .env(PROXY_ENV_VAR, version)
            .exec();
        eprintln!("Failed to exec cached alder binary: {err}");
        std::process::exit(1);
    }

    #[cfg(not(unix))]
    {
        let status = std::process::Command::new(binary)
            .args(&args)
            .env(PROXY_ENV_VAR, version)
            .status()
            .unwrap_or_else(|e| {
                eprintln!("Failed to run cached alder binary: {e}");
                std::process::exit(1);
            });
        std::process::exit(status.code().unwrap_or(1));
    }
}
