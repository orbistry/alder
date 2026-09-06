use std::collections::BTreeMap;
use std::sync::Arc;

use alder_driver::{
    BuildMode, Database, FileSystemSource, InMemorySource, ModuleResult, OverlaySource, Project,
    build_graph_with_dependencies, build_with_dependencies,
};
use miette::Diagnostic as _;
use tokio::sync::Mutex;
use tower_lsp_server::ls_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, NumberOrString,
    Position, Range, Uri,
};
use url::Url;

pub(crate) struct Document {
    pub uri: Uri,
    pub url: Url,
    pub version: i32,
    pub text: String,
}

pub(crate) fn file_url(uri: &Uri) -> Option<Url> {
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let path = path
        .canonicalize()
        .ok()
        .or_else(|| Some(path.parent()?.canonicalize().ok()?.join(path.file_name()?)))?;
    Url::from_file_path(path).ok()
}

pub(crate) async fn check(
    project: &Project,
    documents: &BTreeMap<String, Document>,
) -> Result<BTreeMap<Url, Vec<Diagnostic>>, alder_driver::DriverError> {
    // A fresh overlay prevents a previous edit's interfaces or sources leaking
    // into the next check. Check mode never emits executable artifacts.
    let overlay = InMemorySource::with_files(
        documents
            .values()
            .map(|doc| (doc.url.clone(), doc.text.clone())),
    );
    let db = Arc::new(Mutex::new(Database::new(OverlaySource::new(
        overlay,
        FileSystemSource::new(),
    ))));
    let mut modules = project.discover_modules(&*db.lock().await).await?;
    let dependencies = project
        .build_dependencies(&mut *db.lock().await, &modules, false)
        .await?;
    modules.extend(dependencies.source_modules.iter().cloned());
    modules.sort();
    modules.dedup();
    let graph = build_graph_with_dependencies(db.clone(), &modules, &dependencies).await?;
    let result = build_with_dependencies(db, &graph, BuildMode::Check, dependencies).await;
    let mut output = BTreeMap::new();
    for (url, module) in result.modules {
        let reports = match module {
            ModuleResult::Failed { diagnostics } => diagnostics,
            _ => vec![],
        };
        output.insert(
            url.clone(),
            reports
                .iter()
                .map(|report| convert(report, &url))
                .collect::<Vec<_>>(),
        );
    }
    for report in result.warnings {
        if let Some(url) = output
            .keys()
            .find(|url| url.path() == report.source().name())
            .cloned()
        {
            output
                .entry(url.clone())
                .or_default()
                .push(convert(&report, &url));
        }
    }
    for report in &result.diagnostics {
        if let Some(url) = output
            .keys()
            .find(|url| url.path() == report.source().name())
            .cloned()
        {
            output
                .entry(url.clone())
                .or_default()
                .push(convert(report, &url));
            continue;
        }
        // Build failures without an owning source remain visible on the open
        // project documents. Do not attribute another source's spans to them.
        for doc in documents.values().filter(|doc| {
            doc.url
                .to_file_path()
                .is_ok_and(|path| path.starts_with(&project.root))
        }) {
            let mut diagnostic = convert(report, &doc.url);
            diagnostic.range = Range::default();
            diagnostic.related_information = None;
            output.entry(doc.url.clone()).or_default().push(diagnostic);
        }
    }
    for diagnostics in output.values_mut() {
        diagnostics.sort_by(|a, b| {
            (a.range.start.line, a.range.start.character, &a.message).cmp(&(
                b.range.start.line,
                b.range.start.character,
                &b.message,
            ))
        });
    }
    Ok(output)
}

fn position(text: &str, offset: usize) -> Position {
    let mut end = offset.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let prefix = &text[..end];
    let line = prefix.bytes().filter(|&byte| byte == b'\n').count() as u32;
    let column = prefix
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .encode_utf16()
        .count() as u32;
    Position::new(line, column)
}

fn convert(report: &alder_report::Diagnostic, url: &Url) -> Diagnostic {
    let labels = report
        .labels()
        .map(Iterator::collect::<Vec<_>>)
        .unwrap_or_default();
    let primary = labels
        .iter()
        .find(|label| label.primary())
        .or_else(|| labels.first());
    let range = |label: &miette::LabeledSpan| {
        Range::new(
            position(report.source().text(), label.offset()),
            position(
                report.source().text(),
                label.offset().saturating_add(label.len()),
            ),
        )
    };
    let mut message = report.message().to_owned();
    if let Some(label) = primary.and_then(|label| label.label()) {
        message.push_str(&format!("\n{label}"));
    }
    if let Some(help) = report.help() {
        message.push_str(&format!("\n{help}"));
    }
    let related_information = url.as_str().parse::<Uri>().ok().map(|uri| {
        labels
            .iter()
            .filter(|label| !label.primary())
            .map(|label| DiagnosticRelatedInformation {
                location: Location::new(uri.clone(), range(label)),
                message: label.label().unwrap_or("Related requirement").to_owned(),
            })
            .collect()
    });
    Diagnostic {
        range: primary.map(range).unwrap_or_default(),
        severity: Some(match report.severity() {
            Some(miette::Severity::Warning) => DiagnosticSeverity::WARNING,
            Some(miette::Severity::Advice) => DiagnosticSeverity::HINT,
            _ => DiagnosticSeverity::ERROR,
        }),
        code: report
            .code()
            .map(|code| NumberOrString::String(code.to_string())),
        source: Some("alder".to_owned()),
        message,
        related_information,
        ..Diagnostic::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_count_utf16_units_and_crlf_lines() {
        let text = "a😀é\r\nnext";
        assert_eq!(position(text, "a😀".len()), Position::new(0, 3));
        assert_eq!(position(text, "a😀é\r\n".len()), Position::new(1, 0));
        assert_eq!(position(text, usize::MAX), Position::new(1, 4));
        assert_eq!(position(text, 2), Position::new(0, 1));
    }
}
