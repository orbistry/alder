//! String literal scanning for Alder: single-line `"…"` with escapes
//! `\n \r \t \0 \" \' \\ \u{…}`. Templates reuse the escape scanners.
//!
//! See docs/parser-internals.md §2 and §5.6.
//!
//! Hand-off: the pre-rewrite escape scanners are largely reusable — recover
//! them with `git show 95c298e:crates/alder-parse/src/string.rs` (the old
//! name helpers live at `…:crates/alder-parse/src/expression/variable.rs`).
//! `EscapeResult` is not `Copy`: `error::Escape` (§4, verbatim) is not `Clone`.
// OWNER: string.rs (Wave 1)

use crate::error::{Escape, StringError};
use crate::{Col, Parser, Row};

/// Result of scanning an escape sequence after the backslash.
#[allow(unused)]
#[derive(Debug)]
pub(crate) enum EscapeResult {
    /// Normal escape like `\n`; width in bytes.
    Normal(usize),
    /// Unicode escape `\u{…}`; total bytes consumed.
    Unicode(usize),
    /// End of file during escape.
    EndOfFile,
    /// Invalid escape.
    Problem(Escape),
}

#[allow(unused)]
impl<'a> Parser<'a> {
    /// `"…"` single-line.
    pub(crate) fn string_literal<E>(
        &mut self,
        to_expectation: impl FnOnce(Row, Col) -> E,
        to_error: impl FnOnce(StringError, Row, Col) -> E,
    ) -> Result<&'a str, E> {
        todo!()
    }

    /// Scan the escape after a backslash (`template` adds `` \` `` and `\$`).
    pub(crate) fn eat_escape(&self, template: bool) -> EscapeResult {
        todo!()
    }

    /// Scan `\u{…}` at `u`.
    pub(crate) fn eat_unicode(&self) -> EscapeResult {
        todo!()
    }

    /// Cook the escapes in `src[start..end]` into an arena string.
    pub(crate) fn build_escaped_string(&self, start: usize, end: usize, template: bool) -> &'a str {
        todo!()
    }
}

/// Byte width of the UTF-8 character starting with `b`.
#[allow(unused)]
pub(crate) fn utf8_char_width(b: u8) -> usize {
    todo!()
}

#[cfg(test)]
mod tests {}
