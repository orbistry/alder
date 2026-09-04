//! Deterministic, comment-preserving Alder formatting.
//!
//! Formatting preserves parser-designated verbatim spans and physical token
//! lines. Both input and output are parsed before output is returned.

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
    #[error("formatter changed verbatim source text or non-layout tokens")]
    ChangedSource,
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
    let (before, ranges) = parse_info(source).map_err(Error::Parse)?;
    let mut masked = source.as_bytes().to_vec();
    for &(start, end) in &ranges {
        masked[start..end].fill(b' ');
    }
    let mut state = ScanState::default();
    let mut docs = Vec::new();
    let mut offset = 0;
    let mut range_index = 0;
    for line in source.split_inclusive('\n') {
        let end = offset + line.len();
        while ranges
            .get(range_index)
            .is_some_and(|&(_, stop)| stop <= offset)
        {
            range_index += 1;
        }
        let protected = ranges
            .get(range_index)
            .is_some_and(|&(start, _)| start < end);
        let (body, newline) = if let Some(body) = line.strip_suffix("\r\n") {
            (body, "\r\n")
        } else if let Some(body) = line.strip_suffix('\n') {
            (body, "\n")
        } else {
            (line, "")
        };
        let raw = body.trim_end_matches([' ', '\t']);
        let content = raw.trim_start_matches([' ', '\t']);
        let scanned = std::str::from_utf8(&masked[offset..end])
            .expect("parser ranges end on UTF-8 boundaries");
        let leading_closers = scanned
            .trim_start_matches([' ', '\t'])
            .bytes()
            .take_while(|byte| matches!(byte, b'}' | b')' | b']'))
            .count();
        let line_depth = depth_after_closers(&state.delimiter_groups, leading_closers);
        let formatted = if protected {
            line.to_owned()
        } else if content.is_empty() {
            newline.to_owned()
        } else {
            format!(
                "{}{content}{newline}",
                " ".repeat(line_depth * options.indent_width)
            )
        };
        scan_line(scanned, &mut state);
        docs.push(Doc::text(formatted));
        offset = end;
    }
    let mut output = Doc::concat(docs).render(options.max_width);
    if !output.ends_with('\n') {
        output.push('\n');
    }
    let (after, after_ranges) = parse_info(&output).map_err(Error::InvalidOutput)?;
    if before != after {
        return Err(Error::ChangedComment);
    }
    let payloads = |text: &str, ranges: &[(usize, usize)]| {
        ranges
            .iter()
            .map(|&(start, end)| text[start..end].to_owned())
            .collect::<Vec<_>>()
    };
    // This formatter only changes horizontal whitespace at physical line
    // boundaries. Preserve all interior tokens and every physical line, in
    // addition to the parser-selected verbatim payloads.
    let layout = |text: &str| {
        let mut lines = text
            .split_terminator('\n')
            .map(|line| line.trim_matches([' ', '\t', '\r']).to_owned())
            .collect::<Vec<_>>();
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        lines
    };
    if payloads(source, &ranges) != payloads(&output, &after_ranges)
        || layout(source) != layout(&output)
    {
        return Err(Error::ChangedSource);
    }
    Ok(output)
}

type ParseInfo = (Vec<(String, alder_source_kind::Kind)>, Vec<(usize, usize)>);

fn parse_info(source: &str) -> Result<ParseInfo, String> {
    let bump = Bump::new();
    let source = bump.alloc_str(source);
    let (module, mut ranges) = alder_parse::parse_module_with_verbatim(&bump, source)
        .map_err(|error| format!("{error:?}"))?;
    ranges.sort_unstable();
    ranges.dedup();
    Ok((
        module
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
            .collect(),
        ranges,
    ))
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
        if byte == b'\\' && in_string {
            state.escaped = true;
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
    use indoc::indoc;

    fn template_payload(source: &str) -> String {
        let bump = Bump::new();
        let module = alder_parse::parse_module(&bump, source).unwrap();
        let alder_source::ItemKind::Let(binding) = &module.items[0].value.kind else {
            panic!("expected let")
        };
        let alder_source::Expr::Template(parts) = binding.value.value else {
            panic!("expected template")
        };
        parts
            .iter()
            .map(|part| match part {
                alder_source::TemplatePart::Text(text) => *text,
                _ => panic!("expected literal text"),
            })
            .collect()
    }

    #[test]
    fn preserves_template_whitespace_payload_not_just_parseability() {
        let source = indoc! {r#"
            let message = `start
            <spaces>
            end<spaces>`
        "#}
        .replace("<spaces>", "   ");
        let formatted = format_source(&source).unwrap();
        assert_eq!(template_payload(&formatted), "start\n   \nend   ");
        assert_eq!(template_payload(&formatted), template_payload(&source));
        assert_eq!(format_source(&formatted).unwrap(), formatted);
    }

    #[test]
    fn preserves_nested_template_interpolation_and_escapes() {
        let source = indoc! {r#"
            let message = `outer ${`inner\` ${"x"}
            <spaces>
            end`}
            <spaces>
            final`
        "#}
        .replace("<spaces>", " \t ");
        assert_eq!(format_source(&source).unwrap(), source);
    }

    #[test]
    fn preserves_template_line_endings() {
        let source = indoc! {r#"
            let message = `start
            end`
        "#}
        .replace('\n', "\r\n");
        let formatted = format_source(&source).unwrap();
        assert_eq!(formatted, source);
        assert_eq!(template_payload(&formatted), template_payload(&source));
    }

    #[test]
    fn preserves_a_literal_carriage_return() {
        let source = "let message = `start<cr>end`\n".replace("<cr>", "\r");
        let formatted = format_source(&source).unwrap();
        assert_eq!(template_payload(&formatted), "start\rend");
        assert_eq!(formatted, source);
    }

    #[test]
    fn preserves_trailing_comment_spaces() {
        let source = "// comment   \nlet value = 42\n";
        assert_eq!(format_source(source).unwrap(), source);
    }

    #[test]
    fn preserves_raw_macro_bodies_and_calls() {
        let source = indoc! {r#"
            macro example() {
              quote { a }
            <spaces>
            }
            let value = example!(
              a
            <spaces>
            )
        "#}
        .replace("<spaces>", "   ");
        assert_eq!(format_source(&source).unwrap(), source);
    }

    #[test]
    fn preserves_markup_text_whitespace() {
        let source = indoc! {r#"
            let view = <div>first
            <spaces>
            last</div>
        "#}
        .replace("<spaces>", "   ");
        assert_eq!(format_source(&source).unwrap(), source);
    }

    #[test]
    fn empty_source_formats_idempotently() {
        let formatted = format_source("").unwrap();
        assert_eq!(format_source(&formatted).unwrap(), formatted);
    }

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
