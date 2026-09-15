//! Explicit, shared Cloudflare tool installation. No platform command installs
//! dependencies implicitly, and no npm tree is shipped with the compiler.
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    ops::Deref,
    path::{Path, PathBuf},
    process::Stdio,
};

use miette::{IntoDiagnostic, Result, miette};
use sha2::{Digest, Sha256};
use tokio::process::Command;

const PACKAGE: &str = include_str!("../../support/package.json");
const LOCK: &str = include_str!("../../support/package-lock.json");
const BRIDGE: &str = include_str!("../../support/cloudflare-dev.mjs");
const RECORD: &str = "alder-install.json";
const SETUP: &str = "Run `alder cloudflare setup` to install or repair the tooling.";

#[derive(clap::Args, Debug)]
pub struct Args {
    #[command(subcommand)]
    pub command: Subcommand,
}

#[derive(clap::Subcommand, Debug)]
pub enum Subcommand {
    /// Install pinned Cloudflare tooling in the shared user cache (requires Node and npm)
    Setup {
        /// Validate the selected installation without downloading anything
        #[arg(long, conflicts_with = "remove")]
        check: bool,
        /// Remove only this tooling version; preserve credentials and other versions
        #[arg(long)]
        remove: bool,
    },
    /// Log in through the installed Wrangler, using its normal credential storage
    Login,
}

impl Args {
    pub(super) async fn exec(self, output: &crate::reporting::Output) -> Result<()> {
        let cache = Cache::current()?;
        match self.command {
            Subcommand::Setup { remove: true, .. } => {
                let _lock = cache.lock(false)?;
                cache.recover()?;
                cache.remove()?;
                output.status(
                    "Removed",
                    "selected Cloudflare tooling (credentials preserved)",
                );
            }
            Subcommand::Setup { check: true, .. } => {
                let tools = resolve().await?;
                output.status("Validated", tools.directory.display().to_string());
            }
            Subcommand::Setup { .. } => {
                let node = node().await?;
                let _lock = cache.lock(false)?;
                cache.recover()?;
                if cache.validate().is_ok() {
                    match probe(&node, &cache.directory, true).await {
                        Ok(()) => {
                            output.status("Ready", cache.directory.display().to_string());
                            return Ok(());
                        }
                        Err(error)
                            if error
                                .downcast_ref::<super::cloudflare_process::Cancelled>()
                                .is_some() =>
                        {
                            return Err(error);
                        }
                        Err(_) => {}
                    }
                }
                let npm =
                    executable(if cfg!(windows) { "npm.cmd" } else { "npm" }).ok_or_else(|| {
                        miette!(
                            "npm is required for setup. Install Node.js >=22 with npm, then retry."
                        )
                    })?;
                output.status(
                    "Installing",
                    "pinned Cloudflare tooling in the shared cache",
                );
                let stage = tempfile::Builder::new()
                    .prefix(".install-")
                    .tempdir_in(&cache.root)
                    .into_diagnostic()?;
                for (name, bytes) in [
                    ("package.json", PACKAGE),
                    ("package-lock.json", LOCK),
                    ("cloudflare-dev.mjs", BRIDGE),
                ] {
                    fs::write(stage.path().join(name), bytes).into_diagnostic()?;
                }
                install(&mut npm_command(&npm, stage.path())?).await?;
                probe(&node, stage.path(), true).await?;
                cache.publish(stage)?;
                output.status("Installed", cache.directory.display().to_string());
            }
            Subcommand::Login => {
                let tools = resolve().await?;
                login(&tools).await?;
            }
        }
        Ok(())
    }
}

async fn login(tools: &Toolchain) -> Result<()> {
    let status = super::cloudflare_process::run(tools.wrangler().arg("login"), false).await?;
    if !status.success() {
        return Err(miette::Report::new(ToolExit(status)));
    }
    Ok(())
}

async fn install(command: &mut Command) -> Result<()> {
    // Spool to an anonymous temporary file rather than pipes: large npm output
    // cannot deadlock the supervised child or grow an in-memory buffer.
    let mut log = tempfile::tempfile().into_diagnostic()?;
    command
        .stdout(log.try_clone().into_diagnostic()?)
        .stderr(log.try_clone().into_diagnostic()?);
    let status = super::cloudflare_process::run(command, true).await?;
    if status.success() {
        return Ok(());
    }
    let length = log.metadata().into_diagnostic()?.len();
    let tail = length.min(16 * 1024);
    log.seek(SeekFrom::End(-(tail as i64))).into_diagnostic()?;
    let mut bytes = Vec::new();
    log.take(tail).read_to_end(&mut bytes).into_diagnostic()?;
    let details = String::from_utf8_lossy(&bytes);
    Err(miette!(
        "npm setup failed ({status}); the previous installation was not replaced.\n{}",
        details.trim()
    ))
}

fn npm_command(npm: &Path, stage: &Path) -> Result<Command> {
    // npm treats a symlinked prefix (including macOS /var -> /private/var) as
    // a linked root, which can make `ci` reject an otherwise matching lockfile.
    #[cfg(unix)]
    let stage = stage.canonicalize().into_diagnostic()?;
    // Keep ordinary Windows paths for npm.cmd rather than introducing the
    // verbatim \\?\ prefix returned by canonicalize (which cmd.exe may reject).
    #[cfg(windows)]
    let stage = std::path::absolute(stage).into_diagnostic()?;
    let mut command = Command::new(npm);
    command
        .args([
            "ci",
            "--global=false",
            "--workspaces=false",
            "--install-strategy=hoisted",
            "--ignore-scripts",
            "--omit=dev",
            "--include=prod",
            "--include=optional",
            "--package-lock=true",
            "--bin-links=false",
            "--no-audit",
            "--no-fund",
            "--prefix",
        ])
        .arg(&stage)
        .arg(match std::env::consts::ARCH {
            "aarch64" => "--cpu=arm64",
            _ => "--cpu=x64",
        })
        .arg(match std::env::consts::OS {
            "macos" => "--os=darwin",
            "windows" => "--os=win32",
            _ => "--os=linux",
        })
        .current_dir(&stage)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    if cfg!(target_os = "linux") {
        command.arg("--libc=glibc");
    }
    Ok(command)
}

/// Preserve the delegated command's exit code without capturing its output or
/// credentials. The executable entry point translates this diagnostic.
#[derive(Debug)]
pub struct ToolExit(pub std::process::ExitStatus);
impl std::fmt::Display for ToolExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Wrangler exited with {}", self.0)
    }
}
impl std::error::Error for ToolExit {}
impl miette::Diagnostic for ToolExit {}

pub(super) struct Toolchain {
    pub node: PathBuf,
    directory: PathBuf,
    _lease: Lease,
}

struct Lease(File);

impl Drop for Lease {
    fn drop(&mut self) {
        // A concurrent fork may briefly inherit this open file description
        // before exec closes it. Explicit unlock ends the lease immediately;
        // relying only on close could retain it in that unrelated child.
        let _ = self.0.unlock();
    }
}

impl Deref for Toolchain {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.directory
    }
}

impl Toolchain {
    pub fn wrangler(&self) -> Command {
        let mut command = Command::new(&self.node);
        command
            .arg(self.join("node_modules/wrangler/bin/wrangler.js"))
            .env("WRANGLER_SEND_METRICS", "false")
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        command
    }
}

pub(super) async fn resolve() -> Result<Toolchain> {
    let node = node().await?;
    let cache = Cache::current()?;
    let lease = cache.lock(true)?;
    cache
        .validate()
        .map_err(|_| miette!("Cloudflare tooling is missing or damaged. {SETUP}"))?;
    probe(&node, &cache.directory, false).await?;
    Ok(Toolchain {
        node,
        directory: cache.directory,
        _lease: lease,
    })
}

fn executable(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|directory| {
        let file = directory.join(name);
        // Preserve the executable's basename: version-manager shims dispatch
        // node/npm based on argv[0], even when both symlink to the same binary.
        file.is_file()
            .then(|| std::path::absolute(file).ok())
            .flatten()
    })
}

async fn node() -> Result<PathBuf> {
    let node = executable(if cfg!(windows) { "node.exe" } else { "node" }).ok_or_else(|| {
        miette!("Cloudflare tooling requires Node.js >=22. Install Node.js and put it on PATH.")
    })?;
    let result = Command::new(&node)
        .args([
            "-p",
            "JSON.stringify({version:process.version,platform:process.platform,arch:process.arch})",
        ])
        .kill_on_drop(true)
        .output()
        .await
        .into_diagnostic()?;
    let info = serde_json::from_slice::<NodeInfo>(&result.stdout).ok();
    if !result.status.success()
        || !info
            .as_ref()
            .is_some_and(|info| supported_node(&info.version))
    {
        return Err(miette!(
            "Cloudflare tooling requires Node.js >=22. Update Node.js before continuing."
        ));
    }
    let info = info.expect("validated Node metadata");
    let target = crate::download::platform_target()?;
    if info.target() != Some(target) {
        return Err(miette!(
            "Node.js uses {}/{}, but this Alder requires {target}. Install a matching Node.js architecture and put it on PATH.",
            info.platform,
            info.arch
        ));
    }
    Ok(node)
}

#[derive(serde::Deserialize)]
struct NodeInfo {
    version: String,
    platform: String,
    arch: String,
}

impl NodeInfo {
    fn target(&self) -> Option<&'static str> {
        match (self.platform.as_str(), self.arch.as_str()) {
            ("darwin", "arm64") => Some("aarch64-apple-darwin"),
            ("darwin", "x64") => Some("x86_64-apple-darwin"),
            ("linux", "arm64") => Some("aarch64-unknown-linux-gnu"),
            ("linux", "x64") => Some("x86_64-unknown-linux-gnu"),
            ("win32", "x64") => Some("x86_64-pc-windows-msvc"),
            _ => None,
        }
    }
}

fn supported_node(version: &str) -> bool {
    version
        .trim()
        .strip_prefix('v')
        .and_then(|v| v.split('.').next())
        .and_then(|v| v.parse::<u32>().ok())
        .is_some_and(|major| major >= 22)
}

async fn probe(node: &Path, directory: &Path, setup: bool) -> Result<()> {
    // Load the host and run the installed native binary locally. No Worker,
    // authentication, telemetry, or network request is started by this check.
    let mut command = Command::new(node);
    command.arg("-e").arg(
        "const fs=require('node:fs');const p=require('./package.json');for(const [n,v] of Object.entries(p.dependencies)){if(JSON.parse(fs.readFileSync('node_modules/'+n+'/package.json')).version!==v)process.exit(1)}require('miniflare');require('node:child_process').execFileSync(require('workerd').default,['--version'],{stdio:'ignore'});"
    ).current_dir(directory).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .kill_on_drop(true);
    let status = if setup {
        super::cloudflare_process::run(&mut command, true).await?
    } else {
        // Resolution must not install process-wide signal handlers: Tokio
        // retains those handlers after its receiver drops, which would change
        // the signal behavior of the build/dev/deploy command that follows.
        command.status().await.into_diagnostic()?
    };
    if !status.success() {
        return Err(miette!(
            "Cloudflare tooling could not load with this Node/platform. {SETUP}"
        ));
    }
    Ok(())
}

struct Cache {
    root: PathBuf,
    directory: PathBuf,
    key: String,
}

impl Cache {
    fn current() -> Result<Self> {
        let root = dirs::cache_dir()
            .ok_or_else(|| miette!("Cannot locate the user cache directory"))?
            .join("alder/cloudflare");
        Ok(Self::new(root, crate::download::platform_target()?))
    }

    fn new(root: PathBuf, target: &str) -> Self {
        let key = cache_key(target, PACKAGE, LOCK, BRIDGE);
        Self {
            directory: root.join(&key),
            root,
            key,
        }
    }

    fn lock(&self, shared: bool) -> Result<Lease> {
        reject_link(&self.root)?;
        fs::create_dir_all(&self.root).into_diagnostic()?;
        let path = self.root.join(format!("{}.lock", self.key));
        reject_link(&path)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .into_diagnostic()?;
        let result = if shared {
            file.try_lock_shared()
        } else {
            file.try_lock()
        };
        result.map_err(|_| miette!("This Cloudflare tooling is in use or setup is running. Stop its commands and retry."))?;
        Ok(Lease(file))
    }

    fn validate(&self) -> Result<()> {
        reject_link(&self.directory)?;
        reject_link(&self.directory.join(RECORD))?;
        let record: BTreeMap<String, String> =
            serde_json::from_slice(&fs::read(self.directory.join(RECORD)).into_diagnostic()?)
                .into_diagnostic()?;
        if record != inventory(&self.directory)? {
            return Err(miette!("Cloudflare installation integrity mismatch"));
        }
        for (name, expected) in [
            ("package.json", PACKAGE),
            ("package-lock.json", LOCK),
            ("cloudflare-dev.mjs", BRIDGE),
        ] {
            if fs::read(self.directory.join(name)).into_diagnostic()? != expected.as_bytes() {
                return Err(miette!("Cloudflare support contract mismatch"));
            }
        }
        Ok(())
    }

    fn publish(&self, stage: tempfile::TempDir) -> Result<()> {
        let record = inventory(stage.path())?;
        fs::write(
            stage.path().join(RECORD),
            serde_json::to_vec(&record).into_diagnostic()?,
        )
        .into_diagnostic()?;
        self.replace(stage.path(), |from, to| fs::rename(from, to))?;
        Ok(())
    }

    fn previous(&self) -> PathBuf {
        self.root.join(format!("{}.previous", self.key))
    }

    // Called only under the exclusive installation lease. A process may have
    // stopped between the two renames, or after publication but before cleanup.
    fn recover(&self) -> Result<()> {
        let previous = self.previous();
        reject_link(&previous)?;
        reject_link(&self.directory)?;
        if previous.exists() {
            if self.validate().is_ok() {
                remove_managed(&previous)?;
            } else {
                self.remove()?;
                fs::rename(previous, &self.directory).into_diagnostic()?;
            }
        }
        Ok(())
    }

    fn replace(
        &self,
        stage: &Path,
        publish: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
    ) -> Result<()> {
        self.recover()?;
        let previous = self.previous();
        let replacing = self.directory.exists();
        if replacing {
            fs::rename(&self.directory, &previous).into_diagnostic()?;
        }
        if let Err(error) = publish(stage, &self.directory) {
            if replacing {
                fs::rename(&previous, &self.directory).map_err(|rollback| {
                    miette!(
                        "Could not publish tooling ({error}) or restore it ({rollback}). The previous installation is preserved at {}. Retry setup to recover it.",
                        previous.display()
                    )
                })?;
            }
            return Err(error).into_diagnostic();
        }
        if replacing {
            remove_managed(&previous)?;
        }
        Ok(())
    }

    fn remove(&self) -> Result<()> {
        remove_managed(&self.directory)
    }
}

fn remove_managed(path: &Path) -> Result<()> {
    reject_link(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path).into_diagnostic(),
        Ok(metadata) if metadata.is_file() => fs::remove_file(path).into_diagnostic(),
        Ok(_) => Err(miette!(
            "Refusing non-regular managed tooling entry: {}",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).into_diagnostic(),
    }
}

fn cache_key(target: &str, package: &str, lock: &str, bridge: &str) -> String {
    let mut hash = Sha256::new();
    for part in ["alder-cloudflare-cache-v1", target, package, lock, bridge] {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    format!("{:x}", hash.finalize())
}

fn reject_link(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(miette!(
            "Refusing symlink in managed tooling: {}",
            path.display()
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).into_diagnostic(),
    }
}

fn inventory(root: &Path) -> Result<BTreeMap<String, String>> {
    fn visit(root: &Path, dir: &Path, files: &mut BTreeMap<String, String>) -> Result<()> {
        for entry in fs::read_dir(dir).into_diagnostic()? {
            let entry = entry.into_diagnostic()?;
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .expect("descendant")
                .to_string_lossy()
                .replace('\\', "/");
            if relative == RECORD {
                continue;
            }
            let kind = entry.file_type().into_diagnostic()?;
            if kind.is_dir() {
                visit(root, &path, files)?;
            } else if kind.is_file() {
                files.insert(
                    relative,
                    format!("{:x}", Sha256::digest(fs::read(path).into_diagnostic()?)),
                );
            } else {
                return Err(miette!("Cloudflare cache contains a non-regular entry"));
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files)?;
    Ok(files)
}
