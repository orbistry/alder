//! Owned, source-aware diagnostics shared by Alder's compiler front ends.

use std::fmt::Display;
use std::sync::Arc;

use alder_region::{Position, Region};
use miette::{Diagnostic as MietteDiagnostic, LabeledSpan, NamedSource, Severity, SourceCode};

/// Source text retained by diagnostics after a module arena is dropped.
#[derive(Clone, Debug)]
pub struct Source(Arc<NamedSource<String>>);

impl Source {
    pub fn new(name: impl AsRef<str>, text: impl Into<String>) -> Self {
        Self(Arc::new(
            NamedSource::new(name, text.into()).with_language("alder"),
        ))
    }

    pub fn name(&self) -> &str {
        self.0.name()
    }

    pub fn text(&self) -> &str {
        self.0.inner()
    }

    pub fn span(&self, region: Region) -> miette::SourceSpan {
        span_for_region(self.text(), region)
    }
}

/// A compiler diagnostic with enough owned data for CLI, LSP, and test
/// renderers to choose their own presentation.
#[derive(Clone, Debug, thiserror::Error)]
#[error("{message}")]
pub struct Diagnostic {
    source_code: Source,
    message: String,
    code: Option<String>,
    severity: Severity,
    labels: Vec<LabeledSpan>,
    help: Option<String>,
    related: Vec<Diagnostic>,
}

impl Diagnostic {
    pub fn error(source: Source, message: impl Into<String>) -> Self {
        Self::new(source, Severity::Error, message)
    }

    pub fn warning(source: Source, message: impl Into<String>) -> Self {
        Self::new(source, Severity::Warning, message)
    }

    pub fn advice(source: Source, message: impl Into<String>) -> Self {
        Self::new(source, Severity::Advice, message)
    }

    pub fn new(source: Source, severity: Severity, message: impl Into<String>) -> Self {
        Self {
            source_code: source,
            message: message.into(),
            code: None,
            severity,
            labels: Vec::new(),
            help: None,
            related: Vec::new(),
        }
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn with_primary_label(mut self, region: Region, label: impl Into<String>) -> Self {
        self.labels.push(LabeledSpan::new_primary_with_span(
            Some(label.into()),
            self.source_code.span(region),
        ));
        self
    }

    pub fn with_secondary_label(mut self, region: Region, label: impl Into<String>) -> Self {
        self.labels.push(LabeledSpan::new_with_span(
            Some(label.into()),
            self.source_code.span(region),
        ));
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn with_related(mut self, related: Diagnostic) -> Self {
        self.related.push(related);
        self
    }

    pub fn source(&self) -> &Source {
        &self.source_code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl MietteDiagnostic for Diagnostic {
    fn code(&self) -> Option<Box<dyn Display + '_>> {
        self.code
            .as_deref()
            .map(|code| Box::new(code) as Box<dyn Display>)
    }

    fn severity(&self) -> Option<Severity> {
        Some(self.severity)
    }

    fn help(&self) -> Option<Box<dyn Display + '_>> {
        self.help
            .as_deref()
            .map(|help| Box::new(help) as Box<dyn Display>)
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        Some(self.source_code.0.as_ref())
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        (!self.labels.is_empty()).then(|| Box::new(self.labels.iter().cloned()) as _)
    }

    fn related(&self) -> Option<Box<dyn Iterator<Item = &dyn MietteDiagnostic> + '_>> {
        (!self.related.is_empty()).then(|| {
            Box::new(
                self.related
                    .iter()
                    .map(|diagnostic| diagnostic as &dyn MietteDiagnostic),
            ) as _
        })
    }
}

/// Translate Alder's one-indexed byte positions into a miette byte span.
pub fn span_for_region(source: &str, region: Region) -> miette::SourceSpan {
    let start = offset_for_position(source, region.start);
    let end = offset_for_position(source, region.end).max(start);
    (start, end - start).into()
}

fn offset_for_position(source: &str, position: Position) -> usize {
    let wanted_line = position.line.max(1) as usize;
    let wanted_column = position.column.max(1) as usize;
    let bytes = source.as_bytes();
    let mut line = 1;
    let mut line_start = 0;

    while line < wanted_line {
        let Some(relative_newline) = bytes[line_start..].iter().position(|byte| *byte == b'\n')
        else {
            return bytes.len();
        };
        line_start += relative_newline + 1;
        line += 1;
    }

    let line_end = bytes[line_start..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(bytes.len(), |relative| line_start + relative);
    (line_start + wanted_column - 1).min(line_end)
}

#[cfg(test)]
mod tests {
    use alder_region::{Position, Region};
    use indoc::indoc;
    use miette::{GraphicalReportHandler, GraphicalTheme};

    use super::*;

    #[test]
    fn maps_multiline_regions_to_byte_spans() {
        let source = indoc! {r#"
            fn greet(name: String) {
                "hello"
            }
        "#};
        let region = Region::new(Position::new(2, 5), Position::new(2, 12));
        let span = span_for_region(source, region);
        assert_eq!(span.offset(), source.find("\"hello\"").unwrap());
        assert_eq!(span.len(), 7);
    }

    #[test]
    fn positions_are_byte_based_for_unicode_source() {
        let source = "let greeting = \"hé\"\nnext";
        let region = Region::new(Position::new(2, 1), Position::new(2, 5));
        let span = span_for_region(source, region);
        assert_eq!(&source[span.offset()..span.offset() + span.len()], "next");
    }

    #[test]
    fn renders_named_source_and_help() {
        let source = Source::new("src/main.ald", "fn main() { missing(1) }\n");
        let diagnostic = Diagnostic::error(source, "missing trait implementation")
            .with_code("alder::trait::missing_instance")
            .with_primary_label(
                Region::new(Position::new(1, 13), Position::new(1, 23)),
                "required here",
            )
            .with_help("add a matching impl");
        let mut rendered = String::new();
        GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor())
            .with_width(80)
            .render_report(&mut rendered, &diagnostic)
            .unwrap();
        insta::assert_snapshot!(rendered);
    }
}
