use std::{
    io::{IsTerminal, Write},
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use alder_driver::{
    BuildResult, ModuleResult, Project,
    progress::{Phase, Progress, Reporter},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Color {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, Default, clap::Args)]
pub struct Options {
    /// Show module, phase, file, and compiler-selection details
    #[arg(short, long, global = true)]
    pub verbose: bool,
    /// Suppress routine statuses, even with --verbose; retain diagnostics and failures
    #[arg(short, long, global = true)]
    pub quiet: bool,
    /// Color policy for CLI statuses and diagnostics
    #[arg(long, global = true, value_enum, default_value = "auto")]
    pub color: Color,
}

impl Options {
    /// Scan only reporting flags before proxy selection. Do not consume or
    /// reinterpret program arguments after `--`, or validate another version's CLI.
    pub fn from_startup(args: impl IntoIterator<Item = std::ffi::OsString>) -> Self {
        let mut options = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let Some(arg) = arg.to_str() else { continue };
            match arg {
                "--" => break,
                "--verbose" | "-v" => options.verbose = true,
                "--quiet" | "-q" => options.quiet = true,
                "--color" => {
                    if let Some(value) = args.next() {
                        options.color = parse_color(value.to_str()).unwrap_or_default();
                    }
                }
                _ => {
                    if let Some(value) = arg.strip_prefix("--color=") {
                        options.color = parse_color(Some(value)).unwrap_or_default();
                    } else if let Some(short) = arg
                        .strip_prefix('-')
                        .filter(|short| !short.starts_with('-'))
                    {
                        for flag in short.chars().take_while(|flag| matches!(flag, 'v' | 'q')) {
                            match flag {
                                'v' => options.verbose = true,
                                'q' => options.quiet = true,
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        options
    }

    pub fn use_color(self, terminal: bool, no_color: bool, dumb: bool) -> bool {
        match self.color {
            Color::Always => true,
            Color::Never => false,
            Color::Auto => terminal && !no_color && !dumb,
        }
    }
}

fn parse_color(value: Option<&str>) -> Option<Color> {
    match value? {
        "auto" => Some(Color::Auto),
        "always" => Some(Color::Always),
        "never" => Some(Color::Never),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Counts {
    pub modules: usize,
    pub errors: usize,
    pub warnings: usize,
}

impl Counts {
    pub fn from_build(result: &BuildResult) -> Self {
        Self {
            modules: result.total,
            errors: result.diagnostics.len()
                + result
                    .modules
                    .values()
                    .map(|module| match module {
                        ModuleResult::Failed { diagnostics } => diagnostics.len(),
                        _ => 0,
                    })
                    .sum::<usize>(),
            warnings: result.warnings.len(),
        }
    }
}

/// Cloneable CLI renderer. Writer injection avoids global output redirection.
#[derive(Clone)]
pub struct Output {
    options: Options,
    color: bool,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    counts: Arc<Mutex<Counts>>,
    stage: Arc<Mutex<String>>,
    module_root: Arc<Mutex<std::path::PathBuf>>,
    failure_reported: Arc<std::sync::atomic::AtomicBool>,
}

impl Default for Output {
    fn default() -> Self {
        Self::stderr(Options::default())
    }
}

impl Output {
    pub fn stderr(options: Options) -> Self {
        let color = options.use_color(
            std::io::stderr().is_terminal(),
            std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()),
            std::env::var("TERM").is_ok_and(|value| value == "dumb"),
        );
        // Styling is decided once; pass through OSC-8 source links independently
        // of color. anstream handles platform terminal escape compatibility.
        let writer = anstream::AutoStream::new(std::io::stderr(), anstream::ColorChoice::Always);
        Self::new(writer, options, color)
    }

    pub fn new(writer: impl Write + Send + 'static, options: Options, color: bool) -> Self {
        Self {
            options,
            color,
            writer: Arc::new(Mutex::new(Box::new(writer))),
            counts: Arc::new(Mutex::new(Counts::default())),
            stage: Arc::new(Mutex::new(String::new())),
            module_root: Arc::new(Mutex::new(std::path::PathBuf::new())),
            failure_reported: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub fn silent() -> Self {
        Self::new(std::io::sink(), Options::default(), false)
    }

    pub(crate) fn begin(&self) {
        *self.counts.lock().unwrap() = Counts::default();
        self.stage("");
        self.failure_reported
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn mark_failure_reported(&self) {
        self.failure_reported
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn failure_reported(&self) -> bool {
        self.failure_reported
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Render only runner-owned events, never inspect or rewrite console output.
    /// Returns true when a failed test-result summary has been rendered.
    pub fn test_event(&self, event: alder_runtime::TestEvent, elapsed: Duration) -> bool {
        match event {
            alder_runtime::TestEvent::Passed { module, name } => {
                self.status("Passed", format!("{} — {name}", test_module_name(&module)))
            }
            alder_runtime::TestEvent::Failed {
                module,
                name,
                message,
            } => {
                let message = message
                    .lines()
                    .map(|line| format!("             {line}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                self.line(
                    "Failed",
                    &format!("{} — {name}\n{message}", test_module_name(&module)),
                    true,
                )
            }
            alder_runtime::TestEvent::Finished { passed, failed } => {
                self.line(
                    "Test result",
                    &format!(
                        "{passed} passed; {failed} failed · {:.2}s",
                        elapsed.as_secs_f64()
                    ),
                    failed > 0,
                );
                return failed > 0;
            }
        }
        false
    }

    fn line(&self, action: &str, message: &str, failure: bool) {
        if self.options.quiet && !failure {
            return;
        }
        let style = if self.color {
            anstyle::Style::new().bold().fg_color(Some(if failure {
                anstyle::AnsiColor::Red.into()
            } else if action == "Warning" {
                anstyle::AnsiColor::Yellow.into()
            } else {
                anstyle::AnsiColor::Green.into()
            }))
        } else {
            anstyle::Style::new()
        };
        let _ = writeln!(
            self.writer.lock().unwrap(),
            "{style}{action:>12}{style:#} {message}"
        );
    }

    pub fn status(&self, action: &str, message: impl AsRef<str>) {
        self.line(action, message.as_ref(), false);
    }
    pub fn detail(&self, action: &str, message: impl AsRef<str>) {
        if self.options.verbose {
            self.status(action, message);
        }
    }
    pub fn stage(&self, stage: &str) {
        *self.stage.lock().unwrap() = stage.to_owned();
    }
    pub fn record(&self, result: &BuildResult) {
        *self.counts.lock().unwrap() = Counts::from_build(result);
    }

    pub fn project(&self, action: &str, project: &Project) {
        *self.module_root.lock().unwrap() = project.root.clone();
        for member in &project.members {
            let name = match &member.config {
                alder_config::Config::Package(package) => package.name.to_string(),
                _ => member
                    .root
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            };
            self.status(action, format!("{name} ({})", display_path(&member.root)));
        }
    }

    pub fn diagnostic(&self, report: &miette::Report) {
        let theme = if self.color {
            miette::GraphicalTheme::unicode()
        } else {
            miette::GraphicalTheme::unicode_nocolor()
        };
        let mut rendered = String::new();
        let _ = miette::GraphicalReportHandler::new_themed(theme)
            .render_report(&mut rendered, report.as_ref());
        let _ = writeln!(self.writer.lock().unwrap(), "\n{rendered}");
    }

    pub fn finish(&self, operation: &str, elapsed: Duration) {
        let counts = *self.counts.lock().unwrap();
        let modules = if operation == "check" {
            format!(" · {}", quantity(counts.modules, "module"))
        } else {
            String::new()
        };
        let warnings = if counts.warnings > 0 {
            format!(" · {}", quantity(counts.warnings, "warning"))
        } else {
            String::new()
        };
        self.status(
            "Finished",
            format!(
                "{operation} in {:.2}s{modules}{warnings}",
                elapsed.as_secs_f64()
            ),
        );
    }

    pub fn failure(&self, operation: &str, elapsed: Duration) {
        let counts = *self.counts.lock().unwrap();
        let stage = self.stage.lock().unwrap();
        let operation = if stage.is_empty() { operation } else { &stage };
        self.line(
            "Failed",
            &format!(
                "{operation} · {}, {} · {:.2}s",
                quantity(counts.errors.max(1), "error"),
                quantity(counts.warnings, "warning"),
                elapsed.as_secs_f64()
            ),
            true,
        );
    }
}

impl Reporter for Output {
    fn report(&self, event: Progress) {
        match event {
            Progress::Phase(phase) => self.detail(
                "Processing",
                match phase {
                    Phase::FetchingSources => "source loading",
                    Phase::DiscoveringInterfaces => "interface discovery",
                    Phase::ValidatingPackages => "package validation",
                    Phase::CompilingModules => "module compilation",
                },
            ),
            Progress::ModuleStarted { uri } => self.detail(
                "Compiling",
                uri.to_file_path()
                    .map(|path| {
                        let root = self.module_root.lock().unwrap();
                        display_path(path.strip_prefix(&*root).unwrap_or(&path))
                    })
                    .unwrap_or_else(|_| uri.to_string()),
            ),
        }
    }
}

pub fn display_path(path: &Path) -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    let path = path.strip_prefix(&cwd).unwrap_or(path);
    if path.as_os_str().is_empty() {
        ".".into()
    } else {
        path.display().to_string()
    }
}

pub fn quantity(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

fn test_module_name(module: &str) -> &str {
    module
        .strip_prefix("alder://app/")
        .and_then(|name| name.strip_suffix(".mjs"))
        .unwrap_or(module)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Clone, Default)]
    pub(crate) struct Buffer(Arc<Mutex<Vec<u8>>>);
    impl Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Buffer {
        pub(crate) fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    #[test]
    fn injected_writer_verbosity_counts_timing_and_order() {
        for (verbose, quiet) in [(false, false), (true, false), (false, true)] {
            let buffer = Buffer::default();
            let output = Output::new(
                buffer.clone(),
                Options {
                    verbose,
                    quiet,
                    color: Color::Never,
                },
                false,
            );
            output.status("Checking", "demo (apps/demo)");
            output.report(Progress::ModuleStarted {
                uri: url::Url::parse("file:///src/main.ald").unwrap(),
            });
            *output.counts.lock().unwrap() = Counts {
                modules: 2,
                errors: 1,
                warnings: 2,
            };
            output.diagnostic(&miette::miette!("specific source error"));
            output.failure("check", Duration::from_millis(180));
            let text = buffer.text();
            assert_eq!(text.contains("Checking"), !quiet);
            assert_eq!(text.contains("Compiling"), verbose);
            assert!(text.contains("1 error, 2 warnings · 0.18s"), "{text}");
            assert!(text.find("specific source error").unwrap() < text.find("Failed").unwrap());
            assert!(!text.contains('\x1b'));
        }
    }

    #[test]
    fn success_empty_and_warning_summaries() {
        let buffer = Buffer::default();
        let output = Output::new(buffer.clone(), Options::default(), false);
        output.finish("check", Duration::ZERO);
        *output.counts.lock().unwrap() = Counts {
            modules: 2,
            errors: 0,
            warnings: 1,
        };
        output.finish("check", Duration::from_millis(1234));
        assert_eq!(
            buffer.text(),
            "    Finished check in 0.00s · 0 modules\n    Finished check in 1.23s · 2 modules · 1 warning\n"
        );
    }

    #[test]
    fn color_policy_is_deterministic_and_shared_with_diagnostics() {
        let automatic = Options::default();
        assert!(automatic.use_color(true, false, false));
        assert!(!automatic.use_color(false, false, false));
        assert!(!automatic.use_color(true, true, false));
        assert!(!automatic.use_color(true, false, true));
        assert!(
            Options {
                color: Color::Always,
                ..automatic
            }
            .use_color(false, true, true)
        );
        assert!(
            !Options {
                color: Color::Never,
                ..automatic
            }
            .use_color(true, false, false)
        );
        for color in [false, true] {
            let buffer = Buffer::default();
            let output = Output::new(buffer.clone(), automatic, color);
            output.status("Checking", "demo");
            output.diagnostic(&miette::miette!("bad input"));
            let text = buffer.text();
            assert_eq!(text.lines().next().unwrap().contains('\x1b'), color);
            assert_eq!(text.split_once('\n').unwrap().1.contains('\x1b'), color);
        }
    }

    #[test]
    fn flags_are_global_and_startup_does_not_scan_program_arguments() {
        for args in [
            vec!["alder", "--verbose", "--color=never", "check"],
            vec!["alder", "check", "--verbose", "--color", "never"],
            vec![
                "alder",
                "run",
                "--verbose",
                "--color",
                "never",
                "--",
                "--quiet",
                "--color=always",
            ],
        ] {
            let parsed = crate::Cli::try_parse_from(&args).unwrap();
            let startup = Options::from_startup(args.iter().skip(1).map(std::ffi::OsString::from));
            assert_eq!(parsed.reporting.color, startup.color);
            assert_eq!(parsed.reporting.verbose, startup.verbose);
            assert_eq!(parsed.reporting.quiet, startup.quiet);
        }
        for args in [
            ["alder", "-v", "check", "-q"],
            ["alder", "check", "-q", "-v"],
            ["alder", "-vq", "check", "."],
        ] {
            let parsed = crate::Cli::try_parse_from(args).unwrap();
            let startup = Options::from_startup(args.iter().skip(1).map(std::ffi::OsString::from));
            assert!(parsed.reporting.quiet && startup.quiet);
            assert!(parsed.reporting.verbose && startup.verbose);
        }
    }

    #[test]
    fn blocked_modules_do_not_count_as_diagnostics() {
        let result = BuildResult {
            diagnostics: vec![],
            modules: [(
                url::Url::parse("file:///a.ald").unwrap(),
                ModuleResult::Blocked,
            )]
            .into(),
            total: 1,
            success: 0,
            failed: 1,
            warnings: vec![],
            artifacts: Default::default(),
            interfaces: vec![],
            package_instance_indexes: vec![],
        };
        assert_eq!(Counts::from_build(&result).errors, 0);
    }

    #[test]
    fn test_results_use_runtime_counts_and_quiet_keeps_failures() {
        for quiet in [false, true] {
            let buffer = Buffer::default();
            let output = Output::new(
                buffer.clone(),
                Options {
                    quiet,
                    ..Options::default()
                },
                false,
            );
            output.test_event(
                alder_runtime::TestEvent::Passed {
                    module: "main".into(),
                    name: "works".into(),
                },
                Duration::ZERO,
            );
            assert!(!output.test_event(
                alder_runtime::TestEvent::Finished {
                    passed: 1,
                    failed: 0
                },
                Duration::from_millis(180)
            ));
            assert_eq!(buffer.text().contains("1 passed; 0 failed · 0.18s"), !quiet);
            assert_eq!(buffer.text().contains("Passed"), !quiet);
            assert!(output.test_event(
                alder_runtime::TestEvent::Finished {
                    passed: 1,
                    failed: 2
                },
                Duration::from_millis(250)
            ));
            assert!(
                buffer
                    .text()
                    .ends_with(" Test result 1 passed; 2 failed · 0.25s\n")
            );
        }
    }
}
