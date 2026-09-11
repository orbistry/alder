//! Exercise checked-in programs through the actual options-aware CLI path.
//! Copy only inputs, so these tests neither reuse nor modify fixture caches.

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct Fixture(PathBuf);

impl Fixture {
    fn copy(name: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "alder-e2e-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let fixture = Self(root);
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/e2e")
            .join(name);
        std::fs::copy(source.join("alder.jsonc"), fixture.0.join("alder.jsonc")).unwrap();
        copy_sources(&source.join("src"), &fixture.0.join("src"));
        fixture
    }

    async fn run(&self, arguments: &[&str]) -> Output {
        tokio::time::timeout(
            Duration::from_secs(30),
            tokio::process::Command::new(env!("CARGO_BIN_EXE_alder"))
                .args(arguments)
                .current_dir(&self.0)
                .env("ALDER_PROXY_VERSION", alder_cli::VERSION)
                .env("NO_COLOR", "1")
                .env("FORCE_HYPERLINK", "0")
                .env("TERM", "dumb")
                .kill_on_drop(true)
                .output(),
        )
        .await
        .expect("fixture command exceeded 30 seconds")
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn copy_sources(source: &Path, destination: &Path) {
    std::fs::create_dir(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_sources(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[tokio::test]
async fn standalone_fixtures_execute_through_the_cli() {
    for name in [
        "hello",
        "pipes",
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
        "explicit_async",
        "record_options",
        "hash_equality",
    ] {
        let fixture = Fixture::copy(name);
        let result = fixture.run(&["--quiet", "run"]).await;
        assert!(
            result.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[tokio::test]
async fn test_fixtures_preserve_success_failure_and_continuation() {
    let successful = Fixture::copy("tests").run(&["test"]).await;
    assert!(
        successful.status.success(),
        "{}",
        String::from_utf8_lossy(&successful.stderr)
    );
    let failed = Fixture::copy("test_failures").run(&["test"]).await;
    assert!(!failed.status.success());
    let report = String::from_utf8(failed.stderr).unwrap();
    for name in [
        "synchronous typed failure",
        "asynchronous typed failure",
        "asynchronous defect",
        "later asynchronous success",
    ] {
        assert!(report.contains(name), "{report}");
    }
}

#[tokio::test]
async fn explicit_pattern_failures_and_cleanup_execute_through_the_cli() {
    let fixture = Fixture::copy("pattern_bindings");
    for mode in [
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
        let result = fixture.run(&["--quiet", "run", "--", mode]).await;
        assert!(!result.status.success(), "{mode}");
        let report = String::from_utf8(result.stderr).unwrap();
        assert!(report.contains("Missing payload"), "{mode}: {report}");
    }
    let result = fixture.run(&["--quiet", "run", "--", "success"]).await;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[tokio::test]
async fn graph_representatives_produce_repeatable_bundles_and_diagnostics() {
    let fixture = Fixture::copy("traits");
    let mut previous = None;
    for _ in 0..3 {
        let result = fixture.run(&["--quiet", "build"]).await;
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let bundle = std::fs::read(fixture.0.join("dist/main.mjs")).unwrap();
        if let Some(previous) = &previous {
            assert_eq!(&bundle, previous);
        }
        previous = Some(bundle);
    }
    let fixture = Fixture::copy("hello");
    std::fs::write(
        fixture.0.join("src/main.ald"),
        "pub fn first(value: a) a { 42 }\npub fn second(value: b) b { true }\n",
    )
    .unwrap();
    let mut previous = None;
    for _ in 0..3 {
        let result = fixture.run(&["--quiet", "check"]).await;
        assert!(!result.status.success());
        let diagnostic = String::from_utf8(result.stderr).unwrap();
        // Elapsed time is intentionally variable, even in quiet failure output.
        // Preserve the complete diagnostics and summary counts in the comparison.
        let (diagnostic, elapsed) = diagnostic.rsplit_once(" · ").unwrap();
        let elapsed = elapsed.strip_suffix("s\n").unwrap().parse::<f64>().unwrap();
        assert!(elapsed.is_finite() && elapsed >= 0.0);
        if let Some(previous) = &previous {
            assert_eq!(diagnostic, previous);
        }
        previous = Some(diagnostic.to_owned());
    }
}
