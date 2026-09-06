use std::path::PathBuf;
use std::process::{Command, Output};

struct Project(PathBuf);

impl Project {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "alder-cli-diagnostics-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(
            root.join("alder.jsonc"),
            r#"{"type":"application","target":"standalone"}"#,
        )
        .unwrap();
        Self(root)
    }

    fn source(&self, name: &str, source: &str) {
        std::fs::write(self.0.join("src").join(name), source).unwrap();
    }

    fn run(&self, command: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_alder"))
            .arg(command)
            .arg(&self.0)
            .current_dir(&self.0)
            .env("ALDER_PROXY_VERSION", alder_cli::VERSION)
            .env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .output()
            .unwrap()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        // Only this test's exclusively-created directory is removed.
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn cli_delivers_unused_warnings_without_removing_effects() {
    let project = Project::new();
    project.source(
        "library.ald",
        indoc::indoc! {r#"
            let _ = Io.print("import effect")
            pub let value = 1
        "#},
    );
    project.source(
        "main.ald",
        indoc::indoc! {r#"
            import ~/library.{ value }
            pub fn main() {
                let unused = Io.print("local effect")
                Io.print("main effect")
            }
        "#},
    );
    for command in ["check", "run"] {
        let output = project.run(command);
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(output.status.success(), "{command}: {stderr}");
        assert!(stderr.contains("unused import binding `value`"), "{stderr}");
        assert!(stderr.contains("unused binding `unused`"), "{stderr}");
        assert!(stderr.contains("main.ald"), "{stderr}");
        assert!(!stderr.contains("\u{1b}["), "{stderr}");
        if command == "run" {
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert_eq!(stdout.trim(), "import effect\nlocal effect\nmain effect");
        }
    }
}

#[test]
fn cli_orders_independent_errors_by_source_not_message() {
    let project = Project::new();
    project.source(
        "main.ald",
        indoc::indoc! {r#"
        fn first() Number { "wrong" }
        fn second() Bool { 42 }
        fn third() String { true }
        pub fn main() { 0 }
    "#},
    );
    for command in ["check", "build", "test"] {
        let output = project.run(command);
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(!output.status.success(), "{command}: {stderr}");
        let positions = [
            "type mismatch: expected `Number`, found `String`",
            "type mismatch: expected `Bool`, found `Number`",
            "type mismatch: expected `String`, found `Bool`",
        ]
        .map(|message| {
            stderr
                .find(message)
                .unwrap_or_else(|| panic!("{command}: {stderr}"))
        });
        assert!(
            positions[0] < positions[1] && positions[1] < positions[2],
            "{command}: {stderr}"
        );
        assert!(!project.0.join("dist").exists());
    }
}
