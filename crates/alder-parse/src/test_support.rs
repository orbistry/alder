//! Helpers for snapshot tests.

/// Indent every non-empty line of a fragment by four spaces, so multiline
/// layout tests exercise the fragment as it would appear inside a
/// definition. A token at column 1 always starts a new top-level
/// declaration, so bare multiline fragments are not valid input.
pub(crate) fn indent_fragment(fragment: &str) -> String {
    let mut out = String::with_capacity(fragment.len() + 4 * fragment.lines().count());
    for line in fragment.lines() {
        if !line.is_empty() {
            out.push_str("    ");
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}
