use std::{
    path::PathBuf,
    process::{Command, Output},
};

struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("alder-reporting-{}-{id}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(
            root.join("alder.jsonc"),
            r#"{"type":"application","target":"standalone"}"#,
        )
        .unwrap();
        Self(root)
    }
    fn source(&self, text: &str) {
        std::fs::write(self.0.join("src/main.ald"), text).unwrap();
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_alder"));
        command
            .current_dir(&self.0)
            .env("ALDER_PROXY_VERSION", alder_cli::VERSION)
            .env("NO_COLOR", "1")
            .env("FORCE_HYPERLINK", "0")
            .env("TERM", "dumb");
        command
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

#[test]
fn check_default_verbose_quiet_empty_and_colors() {
    let project = Project::new();
    let empty = project.run(&["check"]);
    assert!(empty.status.success());
    assert!(stderr(&empty).contains("0 modules"));
    project.source("pub fn main() { 42 }");
    for (args, verbose, quiet, color) in [
        (vec!["check"], false, false, false),
        (vec!["--verbose", "check"], true, false, false),
        (vec!["check", "--quiet"], false, true, false),
        (vec!["check", "--color=always"], false, false, true),
        (vec!["--color=never", "check"], false, false, false),
    ] {
        let output = project.run(&args);
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(output.stdout.is_empty());
        let text = stderr(&output);
        assert_eq!(text.contains("Checking"), !quiet, "{text}");
        assert_eq!(text.contains("Finished"), !quiet, "{text}");
        assert_eq!(text.contains("src/main.ald"), verbose, "{text}");
        assert_eq!(text.contains("Resolving"), verbose, "{text}");
        assert_eq!(text.contains('\x1b'), color, "{text}");
        if quiet {
            assert!(text.is_empty());
        }
    }
}

#[test]
fn quiet_keeps_warnings_and_verbose_workspace_identifies_members() {
    let project = Project::new();
    project.source("pub fn main() { let unused = 42\n0 }");
    let warning = project.run(&["check", "--quiet"]);
    assert!(warning.status.success(), "{}", stderr(&warning));
    let text = stderr(&warning);
    assert!(text.contains("unused"), "{text}");
    assert!(!text.contains("Checking"));
    assert!(!text.contains("Finished"));
    let warning = project.run(&["check"]);
    assert!(
        stderr(&warning)
            .lines()
            .last()
            .unwrap()
            .contains("1 warning")
    );

    std::fs::write(
        project.0.join("alder.jsonc"),
        r#"{"type":"workspace","members":["one","two"]}"#,
    )
    .unwrap();
    for name in ["one", "two"] {
        let member = project.0.join(name);
        std::fs::create_dir_all(member.join("src")).unwrap();
        std::fs::write(member.join("alder.jsonc"), format!(r#"{{"type":"package","name":"vendor/{name}","version":"0.1.0","summary":"fixture","license":"MIT"}}"#)).unwrap();
        std::fs::write(member.join("src/main.ald"), "pub fn answer() { 42 }").unwrap();
    }
    let output = project.run(&["check", "--verbose"]);
    let text = stderr(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("Checking vendor/one (one)"), "{text}");
    assert!(text.contains("Checking vendor/two (two)"), "{text}");
    assert!(text.contains("2 modules"), "{text}");
    assert_eq!(text.matches("Finished").count(), 1);
    let build = project.run(&["build"]);
    assert!(!build.status.success());
    assert!(stderr(&build).contains("workspace member"));
}

#[test]
fn lsp_and_plain_help_do_not_leak_human_statuses_or_ansi_to_stdout() {
    let project = Project::new();
    for flags in ["--verbose", "--quiet", "--color=always"] {
        let output = project.run(&["lsp", flags]);
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
    let help = project.run(&["--color=never", "--help"]);
    assert!(help.status.success());
    assert!(!help.stdout.contains(&0x1b));
    assert!(help.stderr.is_empty());
}

#[test]
fn failure_counts_diagnostics_not_blocked_modules_and_summary_is_last() {
    let project = Project::new();
    std::fs::write(
        project.0.join("src/broken.ald"),
        "pub let value: Number = false",
    )
    .unwrap();
    project.source("import ~/broken\npub fn main() { broken.value }");
    for command in ["check", "build", "run", "test"] {
        let output = project.run(&[command, "--quiet"]);
        assert!(!output.status.success());
        let text = stderr(&output);
        assert_eq!(text.matches("Failed").count(), 1, "{text}");
        assert!(
            text.lines().last().unwrap().contains("1 error, 0 warnings"),
            "{text}"
        );
        assert!(text.contains("src/broken.ald"), "{text}");
        assert!(!text.contains("Running"));
        assert!(!text.contains("Testing"));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn run_preserves_program_streams_arguments_and_has_one_build_summary() {
    let project = Project::new();
    project.source("import cli\n#[extern(\"./streams.js\", \"write\")]\nfn write(value: String) ()\npub fn main() { write(cli.args()[0]) }");
    std::fs::write(
        project.0.join("src/streams.js"),
        "export function write(value) { console.log(value); console.error('program stderr'); }",
    )
    .unwrap();
    for flags in [
        vec![],
        vec!["--quiet"],
        vec!["--verbose"],
        vec!["--color=always"],
    ] {
        let mut args = vec!["run"];
        args.extend(flags.clone());
        args.extend(["--", "--quiet"]);
        let output = project.run(&args);
        let text = stderr(&output);
        assert!(output.status.success(), "{text}");
        assert_eq!(output.stdout, b"--quiet\n");
        assert!(text.ends_with("program stderr\n"), "{text}");
        if flags == ["--quiet"] {
            assert_eq!(text, "program stderr\n");
        } else {
            assert_eq!(text.matches("Finished").count(), 1);
            assert!(text.find("Finished").unwrap() < text.find("Running").unwrap());
        }
    }
}

#[test]
fn build_artifact_and_runtime_failure_are_distinct() {
    let project = Project::new();
    project.source("pub fn main() { assert(false) }");
    let build = project.run(&["build"]);
    assert!(build.status.success(), "{}", stderr(&build));
    assert!(project.0.join("dist/main.mjs").is_file());
    let text = stderr(&build);
    assert!(text.find("Bundling").unwrap() < text.find("Built").unwrap());
    assert!(text.find("Built").unwrap() < text.find("Finished").unwrap());
    assert!(text.contains("dist/main.mjs"));
    let run = project.run(&["run"]);
    assert!(!run.status.success());
    assert!(
        stderr(&run)
            .lines()
            .last()
            .unwrap()
            .contains("Failed runtime")
    );
}

#[test]
fn fmt_reports_actual_changes_and_check_never_claims_writes() {
    let project = Project::new();
    project.source("pub fn main(){42}");
    let original = std::fs::read(project.0.join("src/main.ald")).unwrap();
    let check = project.run(&["fmt", "--check", "--quiet"]);
    assert!(!check.status.success());
    assert!(stderr(&check).contains("need formatting"));
    assert!(!stderr(&check).contains("Formatted"));
    assert_eq!(
        std::fs::read(project.0.join("src/main.ald")).unwrap(),
        original
    );
    let formatted = project.run(&["fmt", "--verbose"]);
    assert!(formatted.status.success());
    assert!(stderr(&formatted).contains("1 file changed"));
    assert!(stderr(&formatted).contains("src/main.ald"));
    let unchanged = project.run(&["fmt"]);
    assert!(stderr(&unchanged).contains("0 files changed"));
    let check = project.run(&["fmt", "--check"]);
    assert!(check.status.success());
    assert!(stderr(&check).contains("1 file correct"));
    assert!(
        project
            .run(&["fmt", "--check", "--quiet"])
            .stderr
            .is_empty()
    );
}

#[test]
fn test_runner_owns_actual_counts_once_including_zero_tests() {
    let project = Project::new();
    for (source, summary, success) in [
        ("pub fn main() { 0 }", "0 passed; 0 failed", true),
        ("test \"one\" { assert(true) }", "1 passed; 0 failed", true),
        (
            "test \"one\" { assert(false) }\ntest \"two\" { assert(true) }",
            "1 passed; 1 failed",
            false,
        ),
    ] {
        project.source(source);
        let output = project.run(&["test"]);
        assert_eq!(output.status.success(), success, "{}", stderr(&output));
        assert!(output.stdout.is_empty());
        let text = stderr(&output);
        assert_eq!(text.matches(summary).count(), 1, "{text}");
        assert!(
            !text.contains("Failed test execution"),
            "no duplicate summary: {text}"
        );
        let quiet = project.run(&["test", "--quiet"]);
        assert_eq!(quiet.status.success(), success);
        if success {
            assert!(quiet.stderr.is_empty());
        } else {
            assert!(stderr(&quiet).contains(summary));
            assert!(stderr(&quiet).contains("one"));
        }
    }
    project.source("import io\ntest \"output\" { io.print(\"test stdout\")\nassert(true) }");
    let output = project.run(&["test", "--quiet"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(output.stdout, b"test stdout\n");
    assert!(output.stderr.is_empty());
}

#[cfg(unix)]
#[test]
fn cached_proxy_is_silent_and_forwards_flags_arguments_guard_and_exit() {
    use std::os::unix::fs::PermissionsExt;
    let project = Project::new();
    std::fs::write(
        project.0.join("alder.jsonc"),
        r#"{"type":"application","target":"standalone","compiler":"99.0.0"}"#,
    )
    .unwrap();
    let cache = project.0.join("cache/versions/99.0.0");
    std::fs::create_dir_all(&cache).unwrap();
    let binary = cache.join("alder");
    std::fs::write(&binary, "#!/bin/sh\nprintf 'guard=%s\\n' \"$ALDER_PROXY_VERSION\"\nprintf '<%s>\\n' \"$@\"\nexit 7\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    for verbose in [false, true] {
        let mut command = project.command();
        command
            .env_remove("ALDER_PROXY_VERSION")
            .env("ALDER_HOME", project.0.join("cache"));
        if verbose {
            command.arg("--verbose");
        }
        let output = command
            .args(["run", "--color=never", "--", "--quiet", "a b"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(7));
        let text = stderr(&output);
        assert_eq!(text.contains("Selecting"), verbose, "{text}");
        assert!(!text.contains("Downloading"));
        assert!(!text.contains("Failed"));
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("guard=99.0.0"));
        assert!(stdout.contains("<run>\n<--color=never>\n<-->\n<--quiet>\n<a b>"));
    }
    let output = project
        .command()
        .env("ALDER_PROXY_VERSION", "99.0.0")
        .args(["--quiet", "check"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(stderr(&output).contains("cached binary"));
    assert_eq!(stderr(&output).matches("Failed").count(), 1);
}
