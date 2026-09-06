use serde_json::{Value, json};
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Output};

struct Project(PathBuf);

impl Project {
    fn new() -> Self {
        static NEXT_PROJECT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = NEXT_PROJECT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "alder-cli-diagnostics-{}-{nonce}-{sequence} space",
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
            .env("FORCE_HYPERLINK", "0")
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
fn diagnostic_terminal_links_target_real_files_from_another_working_directory() {
    let project = Project::new();
    project.source(
        "main.ald",
        indoc::indoc! {r#"
        pub fn main() {
            let count: Number = "three"
            let enabled: Bool = 42
        }
    "#},
    );
    let expected_uri =
        url::Url::from_file_path(project.0.canonicalize().unwrap().join("src/main.ald")).unwrap();
    for command in ["check", "build", "test"] {
        let output = Command::new(env!("CARGO_BIN_EXE_alder"))
            .arg(command)
            .arg(&project.0)
            .current_dir(project.0.parent().unwrap())
            .env("ALDER_PROXY_VERSION", alder_cli::VERSION)
            .env("NO_COLOR", "1")
            .env("FORCE_HYPERLINK", "1")
            .output()
            .unwrap();
        assert!(!output.status.success());
        let stderr = String::from_utf8(output.stderr).unwrap();
        let link = format!("\x1b]8;;{expected_uri}\x1b\\src/main.ald\x1b]8;;\x1b\\");
        assert_eq!(stderr.matches(&link).count(), 2, "{command}: {stderr:?}");
        assert!(expected_uri.to_file_path().unwrap().is_file());
        assert!(stderr.contains(&format!("{link}:2:25]")), "{stderr:?}");
        assert!(stderr.contains(&format!("{link}:3:25]")), "{stderr:?}");
    }
}

struct Editor {
    child: std::process::Child,
    messages: std::sync::mpsc::Receiver<Value>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl Editor {
    fn new(project: &Project) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_alder"))
            .arg("lsp")
            .current_dir(&project.0)
            .env("ALDER_PROXY_VERSION", alder_cli::VERSION)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .unwrap();
        let mut output = std::io::BufReader::new(child.stdout.take().unwrap());
        let (sender, messages) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            loop {
                let mut length = None;
                loop {
                    let mut line = String::new();
                    if output.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.strip_prefix("Content-Length:") {
                        length = value.trim().parse::<usize>().ok();
                    }
                }
                let Some(length) = length else {
                    return;
                };
                let mut bytes = vec![0; length];
                if output.read_exact(&mut bytes).is_err() {
                    return;
                }
                let Ok(message) = serde_json::from_slice(&bytes) else {
                    return;
                };
                if sender.send(message).is_err() {
                    return;
                }
            }
        });
        Self {
            child,
            messages,
            reader: Some(reader),
        }
    }

    fn send(&mut self, message: Value) {
        let body = serde_json::to_vec(&message).unwrap();
        let input = self.child.stdin.as_mut().unwrap();
        write!(input, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        input.write_all(&body).unwrap();
        input.flush().unwrap();
    }

    fn receive(&self, matches: impl Fn(&Value) -> bool) -> Value {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let message = self
                .messages
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("language server did not deliver the expected response");
            if matches(&message) {
                return message;
            }
        }
    }

    fn diagnostics(&self, version: i32) -> Value {
        self.receive(|message| {
            message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["version"] == version
        })["params"]["diagnostics"]
            .clone()
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = self.reader.take().unwrap().join();
    }
}

#[test]
fn editor_publishes_unsaved_errors_warnings_and_clears_stale_diagnostics() {
    let project = Project::new();
    project.source("main.ald", "pub fn main() { 0 }");
    let uri = url::Url::from_file_path(project.0.canonicalize().unwrap().join("src/main.ald"))
        .unwrap()
        .to_string();
    let mut editor = Editor::new(&project);
    editor.send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}));
    let initialized = editor.receive(|message| message["id"] == 1);
    assert!(initialized["error"].is_null(), "{initialized}");
    editor.send(json!({"jsonrpc":"2.0","method":"initialized","params":{}}));
    editor.send(json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{
        "uri":uri,"languageId":"alder","version":1,"text":"fn first() Number { \"wrong\" }\nfn second() Bool { 42 }"
    }}}));
    let errors = editor.diagnostics(1);
    assert_eq!(errors.as_array().unwrap().len(), 2, "{errors}");
    assert_eq!(errors[0]["severity"], 1);
    assert_eq!(errors[0]["range"]["start"]["line"], 0);
    assert_eq!(errors[1]["range"]["start"]["line"], 1);
    for (version, text, count) in [
        (2, "pub fn main() {\nlet unused = 1\n0\n}", 1),
        (3, "pub fn main() { 0 }", 0),
        (4, "pub fn main( {", 1),
        (5, "pub fn main() { 0 }", 0),
        (6, "let unused = 1\npub fn main() { 0 }", 1),
    ] {
        editor.send(
            json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{
                "textDocument":{"uri":uri,"version":version},"contentChanges":[{"text":text}]
            }}),
        );
        let diagnostics = editor.diagnostics(version);
        assert_eq!(
            diagnostics.as_array().unwrap().len(),
            count,
            "{diagnostics}"
        );
        if count == 1 {
            assert_eq!(diagnostics[0]["severity"], if version == 4 { 1 } else { 2 });
        }
    }
    editor.send(json!({"jsonrpc":"2.0","method":"textDocument/didClose","params":{"textDocument":{"uri":uri}}}));
    let cleared = editor.receive(|message| {
        message["method"] == "textDocument/publishDiagnostics"
            && message["params"]["version"].is_null()
    });
    assert_eq!(cleared["params"]["diagnostics"], json!([]));
}

#[test]
fn editor_rechecks_dependents_and_ignores_stale_versions() {
    let project = Project::new();
    project.source("library.ald", "pub fn value() { 1 }");
    project.source(
        "main.ald",
        "import ~/library\npub fn main() Number { library.value() }",
    );
    let root = project.0.canonicalize().unwrap();
    let library = url::Url::from_file_path(root.join("src/library.ald"))
        .unwrap()
        .to_string();
    let main = url::Url::from_file_path(root.join("src/main.ald"))
        .unwrap()
        .to_string();
    let mut editor = Editor::new(&project);
    editor.send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}));
    editor.receive(|message| message["id"] == 1);
    editor.send(json!({"jsonrpc":"2.0","method":"initialized","params":{}}));
    editor.send(json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{
        "uri":main,"languageId":"alder","version":1,"text":"import ~/library\npub fn main() Number { library.value() }"
    }}}));
    let initial = editor.receive(|message| {
        message["method"] == "textDocument/publishDiagnostics" && message["params"]["uri"] == main
    });
    assert_eq!(initial["params"]["diagnostics"], json!([]));
    editor.send(
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{
            "uri":library,"languageId":"alder","version":1,"text":"pub fn value() { \"changed\" }"
        }}}),
    );
    let errors = editor.receive(|message| {
        message["method"] == "textDocument/publishDiagnostics" && message["params"]["uri"] == main
    });
    assert_eq!(
        errors["params"]["diagnostics"].as_array().unwrap().len(),
        1,
        "{errors}"
    );
    assert!(
        errors["params"]["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("found `String`")
    );
    // Close restores the on-disk interface and rechecks the open dependent.
    editor.send(json!({"jsonrpc":"2.0","method":"textDocument/didClose","params":{"textDocument":{"uri":library}}}));
    let cleared = editor.receive(|message| {
        message["method"] == "textDocument/publishDiagnostics" && message["params"]["uri"] == main
    });
    assert_eq!(cleared["params"]["diagnostics"], json!([]));
    editor.send(
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{
            "uri":library,"languageId":"alder","version":3,"text":"pub fn value() { 1 }"
        }}}),
    );
    assert_eq!(editor.diagnostics(3), json!([]));
    editor.send(json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{
        "textDocument":{"uri":library,"version":2},"contentChanges":[{"text":"pub fn value() { \"stale\" }"}]
    }}));
    // Editing the dependent forces another check of the retained version.
    editor.send(json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{
        "textDocument":{"uri":main,"version":4},"contentChanges":[{"text":"import ~/library\npub fn main() Number { library.value() }"}]
    }}));
    assert_eq!(editor.diagnostics(4), json!([]));
    assert!(!root.join("dist").exists());
    assert!(!root.join(".alder").exists());
}

#[test]
fn editor_preserves_utf16_ranges_and_rechecks_saved_dependencies() {
    let project = Project::new();
    project.source("main.ald", "pub fn main() { 0 }");
    project.source("library.ald", "pub fn value() { 1 }");
    let uri = url::Url::from_file_path(project.0.canonicalize().unwrap().join("src/main.ald"))
        .unwrap()
        .to_string();
    let mut editor = Editor::new(&project);
    editor.send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}));
    editor.receive(|message| message["id"] == 1);
    editor.send(json!({"jsonrpc":"2.0","method":"initialized","params":{}}));
    let source = "pub fn main() Number { \"😀\" }";
    editor.send(
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{
            "uri":uri,"languageId":"alder","version":1,"text":source
        }}}),
    );
    let errors = editor.diagnostics(1);
    assert_eq!(errors.as_array().unwrap().len(), 1, "{errors}");
    let start = source.find('"').unwrap();
    assert_eq!(
        errors[0]["range"]["start"]["character"],
        source[..start].encode_utf16().count()
    );
    assert_eq!(
        errors[0]["range"]["end"]["character"],
        source[..start].encode_utf16().count() + 4
    );
    assert!(
        !errors[0]["relatedInformation"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    editor.send(json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{
        "textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":"import ~/library\npub fn main() Number { library.value() }"}]
    }}));
    assert_eq!(editor.diagnostics(2), json!([]));
    project.source("library.ald", "pub fn value() { \"changed\" }");
    editor.send(json!({"jsonrpc":"2.0","method":"textDocument/didSave","params":{"textDocument":{"uri":uri}}}));
    assert_eq!(editor.diagnostics(2).as_array().unwrap().len(), 1);
    project.source("library.ald", "pub fn value() { 1 }");
    editor.send(json!({"jsonrpc":"2.0","method":"workspace/didChangeWatchedFiles","params":{"changes":[{"uri":uri,"type":2}]}}));
    assert_eq!(editor.diagnostics(2), json!([]));
}

#[test]
fn cli_delivers_unused_warnings_without_removing_effects() {
    let project = Project::new();
    project.source(
        "library.ald",
        indoc::indoc! {r#"
            let initialization = Io.print("import effect")
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
        assert!(
            stderr.contains("unused binding `initialization`"),
            "{stderr}"
        );
        assert!(stderr.contains("[src/main.ald:"), "{stderr}");
        assert!(stderr.contains("[src/library.ald:"), "{stderr}");
        assert!(!stderr.contains("\u{1b}["), "{stderr}");
        if command == "run" {
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert_eq!(stdout.trim(), "import effect\nlocal effect\nmain effect");
        }
    }
}

#[test]
fn cli_reports_failed_dependencies_without_unknown_name_cascades() {
    let project = Project::new();
    project.source(
        "broken.ald",
        indoc::indoc! {r#"
        pub fn value() Number { true }
        fn another() String { 42 }
    "#},
    );
    project.source("facade.ald", "pub import ~/broken.*");
    project.source(
        "main.ald",
        "import ~/facade\npub fn main() { facade.value() }",
    );
    project.source("independent.ald", "pub fn separate() Bool { 42 }");
    for command in ["check", "build", "test"] {
        let output = project.run(command);
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(!output.status.success(), "{command}: {stderr}");
        for message in [
            "expected `Number`, found `Bool`",
            "expected `String`, found `Number`",
            "expected `Bool`, found `Number`",
        ] {
            assert!(stderr.contains(message), "{command}: {stderr}");
        }
        assert!(!stderr.contains("unknown name"), "{command}: {stderr}");
        assert!(!stderr.contains("does not export"), "{command}: {stderr}");
        assert!(!project.0.join("dist").exists());
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

#[test]
fn cli_and_editor_deliver_statement_recovery_and_clear_it() {
    let project = Project::new();
    let source = indoc::indoc! {r#"
        pub fn main() {
            let first: Number = "wrong"
            let dependent = first + true
            let second: Bool = 42
        }
    "#};
    project.source("main.ald", source);
    for command in ["check", "build", "test"] {
        let output = project.run(command);
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(!output.status.success(), "{command}: {stderr}");
        assert_eq!(
            stderr.matches("type mismatch:").count(),
            2,
            "{command}: {stderr}"
        );
        assert!(
            stderr.contains("expected `Number`, found `String`"),
            "{stderr}"
        );
        assert!(
            stderr.contains("expected `Bool`, found `Number`"),
            "{stderr}"
        );
        assert!(!stderr.contains('\u{1b}'), "{stderr}");
        assert!(!project.0.join("dist").exists());
    }
    let uri = url::Url::from_file_path(project.0.canonicalize().unwrap().join("src/main.ald"))
        .unwrap()
        .to_string();
    let mut editor = Editor::new(&project);
    editor.send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}));
    let initialized = editor.receive(|message| message["id"] == 1);
    assert!(initialized["error"].is_null(), "{initialized}");
    editor.send(json!({"jsonrpc":"2.0","method":"initialized","params":{}}));
    editor.send(
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{
            "uri":uri,"languageId":"alder","version":1,"text":source
        }}}),
    );
    let errors = editor.diagnostics(1);
    assert_eq!(errors.as_array().unwrap().len(), 2, "{errors}");
    assert_eq!(errors[0]["range"]["start"]["line"], 1);
    assert_eq!(errors[1]["range"]["start"]["line"], 3);
    editor.send(
        json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{
            "textDocument":{"uri":uri,"version":2},
            "contentChanges":[{"text":"pub fn main() { 0 }"}]
        }}),
    );
    assert_eq!(editor.diagnostics(2), json!([]));
}

#[test]
fn cli_and_editor_deliver_pattern_errors_and_clear_them() {
    let project = Project::new();
    let source = indoc::indoc! {r#"
        pub fn partial(value: Option[Bool]) Number {
            match value { Some(true) => 0, None => 1 }
        }
        pub fn redundant(value: Bool) Number {
            match value { true => 0, false => 1, _ => 2 }
        }
        pub fn main() { 0 }
    "#};
    project.source("main.ald", source);
    for command in ["check", "build", "test"] {
        let output = project.run(command);
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(!output.status.success(), "{command}: {stderr}");
        assert_eq!(stderr.matches("[src/main.ald:").count(), 2, "{stderr}");
        assert!(
            stderr.contains("uncovered patterns include Some(false)"),
            "{stderr}"
        );
        assert!(
            stderr.contains("this pattern is already covered"),
            "{stderr}"
        );
        assert!(!stderr.contains('\u{1b}'), "{stderr}");
        assert!(!project.0.join(".alder").exists());
        assert!(!project.0.join("dist").exists());
    }
    let uri = url::Url::from_file_path(project.0.canonicalize().unwrap().join("src/main.ald"))
        .unwrap()
        .to_string();
    let mut editor = Editor::new(&project);
    editor.send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}));
    let initialized = editor.receive(|message| message["id"] == 1);
    assert!(initialized["error"].is_null(), "{initialized}");
    editor.send(json!({"jsonrpc":"2.0","method":"initialized","params":{}}));
    editor.send(
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{
            "uri":uri,"languageId":"alder","version":1,"text":source
        }}}),
    );
    let errors = editor.diagnostics(1);
    assert_eq!(errors.as_array().unwrap().len(), 2, "{errors}");
    assert_eq!(errors[0]["range"]["start"]["line"], 1);
    assert_eq!(errors[1]["range"]["start"]["line"], 4);
    assert_eq!(errors[0]["code"], "alder::type::non_exhaustive_match");
    assert_eq!(errors[1]["code"], "alder::type::redundant_pattern");
    let valid = source
        .replace("Some(true)", "Some(_)")
        .replace(", _ => 2", "");
    editor.send(
        json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{
            "textDocument":{"uri":uri,"version":2}, "contentChanges":[{"text":valid}]
        }}),
    );
    assert_eq!(editor.diagnostics(2), json!([]));
}
