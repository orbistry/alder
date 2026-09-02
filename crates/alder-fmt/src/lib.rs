//! Deterministic, comment-preserving Alder formatting.
//!
//! Formatting is deliberately syntax-safe: both input and output are parsed,
//! and the parser comment side tables must match before output is returned.

mod doc;

use bumpalo::Bump;

pub use doc::Doc;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("source does not parse: {0}")]
    Parse(String),
    #[error("formatter produced invalid source: {0}")]
    InvalidOutput(String),
    #[error("formatter changed a source comment")]
    ChangedComment,
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub indent_width: usize,
    pub max_width: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            indent_width: 4,
            max_width: 100,
        }
    }
}

pub fn format_source(source: &str) -> Result<String, Error> {
    format_with(source, Options::default())
}

pub fn format_with(source: &str, options: Options) -> Result<String, Error> {
    let before = parse_comments(source).map_err(Error::Parse)?;
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let mut state = ScanState::default();
    let mut docs = Vec::new();

    for line in normalized.lines() {
        let raw = line.trim_end_matches([' ', '\t']);
        if raw.trim().is_empty() {
            docs.push(Doc::Nil);
            continue;
        }

        let content = raw.trim_start_matches([' ', '\t']);
        let leading_closers = if state.in_template {
            0
        } else {
            content
                .bytes()
                .take_while(|byte| matches!(byte, b'}' | b')' | b']'))
                .count()
        };
        let line_depth = depth_after_closers(&state.delimiter_groups, leading_closers);
        let formatted = if state.in_template {
            raw.to_owned()
        } else {
            format!("{}{content}", " ".repeat(line_depth * options.indent_width))
        };
        scan_line(content, &mut state);
        docs.push(Doc::text(formatted));
    }

    while docs.last().is_some_and(|doc| matches!(doc, Doc::Nil)) {
        docs.pop();
    }
    let separated = docs
        .into_iter()
        .enumerate()
        .flat_map(|(index, doc)| (index > 0).then_some(Doc::Line).into_iter().chain([doc]));
    let mut output = Doc::concat(separated).render(options.max_width);
    output.push('\n');

    let after = parse_comments(&output).map_err(Error::InvalidOutput)?;
    if before != after {
        return Err(Error::ChangedComment);
    }
    Ok(output)
}

fn parse_comments(source: &str) -> Result<Vec<(String, alder_source_kind::Kind)>, String> {
    let bump = Bump::new();
    let source = bump.alloc_str(source);
    let module = alder_parse::parse_module(&bump, source).map_err(|error| format!("{error:?}"))?;
    Ok(module
        .comments
        .iter()
        .map(|comment| {
            let kind = match comment.kind {
                alder_source::CommentKind::Line => alder_source_kind::Kind::Line,
                alder_source::CommentKind::OuterDoc => alder_source_kind::Kind::OuterDoc,
                alder_source::CommentKind::InnerDoc => alder_source_kind::Kind::InnerDoc,
            };
            (comment.text.to_owned(), kind)
        })
        .collect())
}

mod alder_source_kind {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Kind {
        Line,
        OuterDoc,
        InnerDoc,
    }
}

#[derive(Default)]
struct ScanState {
    in_template: bool,
    escaped: bool,
    // Delimiters opened on one physical line form one visual indentation
    // level. This keeps `call({` from double-indenting while still preserving
    // distinct levels opened on separate lines.
    delimiter_groups: Vec<usize>,
}

fn depth_after_closers(groups: &[usize], mut closers: usize) -> usize {
    let mut depth = groups.len();
    for group in groups.iter().rev() {
        if closers < *group {
            break;
        }
        closers -= group;
        depth -= 1;
    }
    depth
}

fn scan_line(line: &str, state: &mut ScanState) {
    let bytes = line.as_bytes();
    let mut index = 0;
    let mut in_string = false;
    let mut opened_group = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if state.escaped {
            state.escaped = false;
            index += 1;
            continue;
        }
        if byte == b'\\' && (in_string || state.in_template) {
            state.escaped = true;
            index += 1;
            continue;
        }
        if state.in_template {
            if byte == b'`' {
                state.in_template = false;
            }
            index += 1;
            continue;
        }
        if in_string {
            if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            break;
        }
        match byte {
            b'"' => in_string = true,
            b'`' => state.in_template = true,
            b'{' | b'(' | b'[' => {
                if !opened_group {
                    state.delimiter_groups.push(0);
                    opened_group = true;
                }
                *state.delimiter_groups.last_mut().expect("group was pushed") += 1;
            }
            b'}' | b')' | b']' => {
                if let Some(group) = state.delimiter_groups.last_mut() {
                    *group -= 1;
                    if *group == 0 {
                        state.delimiter_groups.pop();
                        opened_group = false;
                    }
                }
            }
            _ => {}
        }
        index += 1;
    }
    state.escaped = false;
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::*;

    #[test]
    fn indents_blocks_and_is_idempotent() {
        let source = "fn main() {\nlet x = 1\nif true {\nx\n}\n}\n";
        let once = format_source(source).unwrap();
        let twice = format_source(&once).unwrap();
        assert_eq!(once, twice);
        assert_eq!(
            once,
            "fn main() {\n    let x = 1\n    if true {\n        x\n    }\n}\n"
        );
    }

    #[test]
    fn preserves_comment_payloads() {
        let source = "//! docs\nfn main() {\n// hello\n1 // tail\n}\n";
        let output = format_source(source).unwrap();
        assert!(output.contains("//! docs"));
        assert!(output.contains("// hello"));
        assert!(output.contains("// tail"));
    }

    #[test]
    fn preserves_multiline_template_text() {
        let source = "let message = `first\n  second\nthird`\n";
        assert_eq!(format_source(source).unwrap(), source);
    }

    #[test]
    fn every_repository_source_is_idempotent() {
        fn visit(path: &Path, sources: &mut Vec<std::path::PathBuf>) {
            for entry in fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    if path.file_name().is_some_and(|name| name == "target") {
                        continue;
                    }
                    visit(&path, sources);
                } else if path.extension().is_some_and(|extension| extension == "ald") {
                    sources.push(path);
                }
            }
        }

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut sources = Vec::new();
        visit(&root, &mut sources);
        sources.sort();
        assert!(!sources.is_empty());
        for path in sources {
            let source = fs::read_to_string(&path).unwrap();
            let once = format_source(&source)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let twice = format_source(&once).unwrap();
            assert_eq!(once, twice, "{}", path.display());
        }
    }
}
