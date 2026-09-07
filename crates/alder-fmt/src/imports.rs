//! Import runs are sorted without crossing declarations or visibility boundaries.

use alder_source::{Import, ImportTail, ItemKind, ModuleRoot, Visibility};

struct Entry {
    section: u8,
    path: String,
    text: String,
    start_line: u32,
    end_line: u32,
    before: Vec<String>,
    after: Vec<String>,
}

fn spelling(import: &Import<'_>) -> (u8, String, String) {
    let (section, mut path) = match import.path.value.root {
        ModuleRoot::StandardLibrary => (0, String::new()),
        ModuleRoot::Local(_) => (1, "~/".to_owned()),
        ModuleRoot::Package { author, package } => {
            (2, format!("@{}/{}", author.value, package.value))
        }
    };
    for (index, segment) in import.path.value.segments.iter().enumerate() {
        if index > 0 || section == 2 {
            path.push('/');
        }
        path.push_str(segment.value);
    }
    let tail = match import.tail {
        ImportTail::Module => String::new(),
        ImportTail::Alias(alias) => format!(" as {}", alias.value),
        ImportTail::All(_) => ".*".to_owned(),
        ImportTail::Names(names) => format!(
            ".{{{}}}",
            names
                .iter()
                .map(|name| match name.alias {
                    Some(alias) => format!("{} as {}", name.name.value, alias.value),
                    None => name.name.value.to_owned(),
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    let text = format!("{path}{tail}");
    (section, path, text)
}

pub(super) fn format(source: &str, indent: usize) -> Result<String, String> {
    let bump = bumpalo::Bump::new();
    let module = alder_parse::parse_module(&bump, source).map_err(|err| format!("{err:?}"))?;
    let mut lines = vec![0];
    lines.extend(source.match_indices('\n').map(|(offset, _)| offset + 1));
    let offset = |line: u32, column: u32| lines[line as usize - 1] + column as usize - 1;
    let is_import =
        |kind: &ItemKind<'_>| matches!(kind, ItemKind::Import(_) | ItemKind::ImportGroup(_));
    let mut output = String::new();
    let mut cursor = 0;
    let mut index = 0;
    while index < module.items.len() {
        let first = module.items[index];
        if !is_import(&first.value.kind) || !first.value.attributes.is_empty() {
            index += 1;
            continue;
        }
        let public = matches!(first.value.visibility, Visibility::Pub(_));
        let mut stop = index + 1;
        while stop < module.items.len() {
            let item = module.items[stop];
            if !is_import(&item.value.kind)
                || !item.value.attributes.is_empty()
                || matches!(item.value.visibility, Visibility::Pub(_)) != public
            {
                break;
            }
            stop += 1;
        }
        let last = module.items[stop - 1];
        let next_start = module.items.get(stop).map_or(source.len(), |item| {
            offset(item.region.start.line, item.region.start.column)
        });
        let mut start = offset(first.region.start.line, first.region.start.column);
        let mut end = offset(last.region.end.line, last.region.end.column);
        // A contiguous leading comment block travels with the first import.
        // Blank-separated comments outside a run stay exactly where they were.
        let mut first_line = first.region.start.line;
        for comment in module.comments.iter().rev() {
            if comment.region.end.line + 1 == first_line
                && comment.kind != alder_source::CommentKind::InnerDoc
            {
                let comment_start = offset(comment.region.start.line, comment.region.start.column);
                let line_start = lines[comment.region.start.line as usize - 1];
                if comment_start < cursor || !source[line_start..comment_start].trim().is_empty() {
                    break;
                }
                start = line_start;
                first_line = comment.region.start.line;
            }
        }
        let mut entries = Vec::new();
        for item in &module.items[index..stop] {
            let mut add = |import: &Import<'_>, start_line, end_line| {
                let (section, path, text) = spelling(import);
                entries.push(Entry {
                    section,
                    path,
                    text,
                    start_line,
                    end_line,
                    before: vec![],
                    after: vec![],
                });
            };
            match item.value.kind {
                ItemKind::Import(import) => {
                    add(import, item.region.start.line, item.region.end.line)
                }
                ItemKind::ImportGroup(group) => {
                    for entry in group {
                        add(entry.value, entry.region.start.line, entry.region.end.line);
                    }
                }
                _ => unreachable!(),
            }
        }
        // Comments not adjacent to an entry are a run preamble. This keeps
        // standalone notes in lexical order while allowing complete path sorting.
        let mut preamble = Vec::new();
        for comment in module.comments.iter().rev() {
            let comment_start = offset(comment.region.start.line, comment.region.start.column);
            if comment_start < start
                || comment_start >= next_start
                || comment.region.start.line > last.region.end.line
            {
                continue;
            }
            let comment_end = offset(comment.region.end.line, comment.region.end.column);
            end = end.max(comment_end);
            if let Some(entry) = entries
                .iter_mut()
                .rev()
                .find(|entry| entry.end_line == comment.region.start.line)
            {
                entry.after.insert(0, comment.text.to_owned());
            } else if let Some(entry) = entries.iter_mut().find(|entry| {
                comment.region.end.line + 1 == entry.start_line
                    || (comment.region.start.line >= entry.start_line
                        && comment.region.end.line <= entry.end_line)
            }) {
                entry.start_line = entry.start_line.min(comment.region.start.line);
                entry.before.insert(0, comment.text.to_owned());
            } else {
                preamble.insert(0, comment.text.to_owned());
            }
        }
        output.push_str(&source[cursor..start]);
        let newline = if source.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let has_preamble = !preamble.is_empty();
        for comment in preamble {
            output.push_str(&comment);
            output.push_str(newline);
        }
        if has_preamble {
            output.push_str(newline);
        }
        entries.sort_by(|a, b| (a.section, &a.path).cmp(&(b.section, &b.path)));
        let keyword = if public { "pub import" } else { "import" };
        let grouped = entries.len() > 1;
        if grouped {
            output.push_str(keyword);
            output.push_str(" (");
            output.push_str(newline);
        }
        let padding = if grouped {
            " ".repeat(indent)
        } else {
            String::new()
        };
        let mut section = entries[0].section;
        for entry in entries {
            if section != entry.section {
                output.push_str(newline);
                section = entry.section;
            }
            for comment in entry.before {
                output.push_str(&padding);
                output.push_str(&comment);
                output.push_str(newline);
            }
            output.push_str(&padding);
            if !grouped {
                output.push_str(keyword);
                output.push(' ');
            }
            output.push_str(&entry.text);
            if grouped {
                output.push(',');
            }
            for comment in entry.after {
                output.push(' ');
                output.push_str(&comment);
            }
            if grouped {
                output.push_str(newline);
            }
        }
        if grouped {
            output.push(')');
        }
        cursor = end;
        index = stop;
    }
    output.push_str(&source[cursor..]);
    // Sorting and grouping may change comment order and AST item boundaries,
    // but never the import multiset, visibility, or exact comment spellings.
    let formatted = alder_parse::parse_module(&bump, &output)
        .map_err(|error| format!("formatted imports do not parse: {error:?}"))?;
    let imports = |module: &alder_source::Module<'_>| {
        let mut entries = module
            .import_entries()
            .map(|(visibility, _, import)| {
                (matches!(visibility, Visibility::Pub(_)), spelling(import).2)
            })
            .collect::<Vec<_>>();
        entries.sort();
        entries
    };
    let comments = |module: &alder_source::Module<'_>| {
        let mut comments = module
            .comments
            .iter()
            .map(|comment| (format!("{:?}", comment.kind), comment.text.to_owned()))
            .collect::<Vec<_>>();
        comments.sort();
        comments
    };
    if imports(&module) != imports(&formatted) || comments(&module) != comments(&formatted) {
        return Err("import formatting changed imports or comments".to_owned());
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use crate::format_source;
    use indoc::indoc;

    fn check(input: &str, expected: &str) {
        let actual = format_source(input).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(format_source(&actual).unwrap(), actual);
    }

    #[test]
    fn roots_and_paths_sort_independently_of_aliases() {
        check(
            "import @vendor/cache\nimport ~/z as a\nimport json\nimport array.{map}\nimport ~/a as z\nimport @acme/http\n",
            indoc! {"\
                import (
                    array.{map},
                    json,

                    ~/a as z,
                    ~/z as a,

                    @acme/http,
                    @vendor/cache,
                )
            "},
        );
    }

    #[test]
    fn single_groups_collapse() {
        check(
            "pub import (~/user.{User as Person},)\n",
            "pub import ~/user.{User as Person}\n",
        );
    }

    #[test]
    fn declarations_and_visibility_separate_runs() {
        check(
            "import json\npub import fiber\nlet value = 1\nimport io\n",
            "import json\npub import fiber\nlet value = 1\nimport io\n",
        );
    }

    #[test]
    fn attached_comments_follow_entries() {
        check(
            "// JSON tools\nimport json // encoder\n// Array tools\nimport array.{map}\n",
            indoc! {"\
                import (
                    // Array tools
                    array.{map},
                    // JSON tools
                    json, // encoder
                )
            "},
        );
    }

    #[test]
    fn standalone_comments_remain_run_preamble() {
        check(
            "// Utilities\n\nimport json\nimport array\n",
            "// Utilities\n\nimport (\n    array,\n    json,\n)\n",
        );
    }

    #[test]
    fn standalone_notes_inside_runs_have_a_stable_preamble() {
        check(
            "import json\n\n// Utilities\n\nimport array\n",
            "// Utilities\n\nimport (\n    array,\n    json,\n)\n",
        );
    }

    #[test]
    fn comments_inside_selections_are_preserved() {
        check(
            "import (json.{\n// Encoder\nencode,\n// Decoder\ndecode\n}, array,)\n",
            "import (\n    array,\n    // Encoder\n    // Decoder\n    json.{encode, decode},\n)\n",
        );
    }

    #[test]
    fn public_and_private_groups_stay_separate() {
        check(
            "import json\nimport array\npub import ~/z.*\npub import ~/a as client\n",
            "import (\n    array,\n    json,\n)\npub import (\n    ~/a as client,\n    ~/z.*,\n)\n",
        );
    }

    #[test]
    fn retains_crlf_and_nested_paths() {
        check(
            "import http/router\r\nimport http/client as z\r\n",
            "import (\r\n    http/client as z,\r\n    http/router,\r\n)\r\n",
        );
    }
}
