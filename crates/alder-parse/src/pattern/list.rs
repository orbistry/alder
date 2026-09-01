//! List pattern parsing for Alder.
//!
//! Ported from Elm's `Pattern.list`.
//!
//! Handles: `[]`, `[p1]`, `[p1, p2, ...]`
//!
//! Note: This handles the `[a, b, c]` syntax which produces `Pattern::List`.
//! The cons syntax `head :: tail` is handled in `pattern_expr` and produces `Pattern::Cons`.

use alder_region::{Located, Position};
use alder_source::Pattern;
use bumpalo::collections::Vec as BumpVec;

use crate::Parser;
use crate::error::{self, PList};

impl<'a> Parser<'a> {
    /// Parse a list pattern: `[]`, `[p1, p2, ...]`
    pub(super) fn pattern_list(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        self.in_context(
            |bump, list_err, row, col| error::Pattern::List(bump.alloc(list_err), row, col),
            |p| p.word1(0x5B, error::Pattern::Start),
            |p| p.pattern_list_body(start),
        )
    }

    /// Parse list pattern body after `[`.
    fn pattern_list_body(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, PList<'a>> {
        self.chomp_and_check_indent(PList::Space, PList::IndentOpen)?;

        self.one_of(
            PList::Open,
            vec![
                // Non-empty list
                Box::new(|p: &mut Parser<'a>| {
                    let (first, end) = p.pattern_list_entry()?;
                    p.check_indent(end.line, end.column, PList::IndentEnd)?;

                    let mut patterns: BumpVec<'a, &'a Located<Pattern<'a>>> =
                        BumpVec::new_in(p.bump);
                    patterns.push(first);
                    p.pattern_list_help(&mut patterns)?;

                    let slice = patterns.into_bump_slice();
                    Ok(p.add_end(start, Pattern::List(slice)))
                }),
                // Empty list
                Box::new(|p: &mut Parser<'a>| {
                    p.word1(0x5D, PList::Open)?;
                    let empty: &'a [&'a Located<Pattern<'a>>] = &[];
                    Ok(p.add_end(start, Pattern::List(empty)))
                }),
            ],
        )
    }

    /// Parse a pattern inside a list.
    /// Returns `(pattern, end)` where end is the position at end of pattern (before any chomp).
    fn pattern_list_entry(&mut self) -> Result<(&'a Located<Pattern<'a>>, Position), PList<'a>> {
        self.specialize(
            |bump, pat_err, row, col| PList::Expr(bump.alloc(pat_err), row, col),
            |p| p.pattern_expr(),
        )
    }

    /// Parse remaining list elements.
    fn pattern_list_help(
        &mut self,
        patterns: &mut BumpVec<'a, &'a Located<Pattern<'a>>>,
    ) -> Result<(), PList<'a>> {
        loop {
            self.chomp(PList::Space)?;

            let done = self.one_of(
                PList::End,
                vec![
                    // Comma - another pattern
                    Box::new(|p: &mut Parser<'a>| {
                        p.word1(0x2C, PList::End)?;
                        p.chomp_and_check_indent(PList::Space, PList::IndentExpr)?;

                        let (pat, end) = p.pattern_list_entry()?;
                        patterns.push(pat);

                        p.check_indent(end.line, end.column, PList::IndentEnd)?;
                        Ok(false)
                    }),
                    // Close bracket
                    Box::new(|p: &mut Parser<'a>| {
                        p.word1(0x5D, PList::End)?;
                        Ok(true)
                    }),
                ],
            )?;

            if done {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        assert_indented_pattern_snapshot, assert_pattern_error_snapshot, assert_pattern_snapshot,
    };

    #[test]
    fn empty() {
        assert_pattern_snapshot!("[]");
    }

    #[test]
    fn single() {
        assert_pattern_snapshot!("[a]");
    }

    #[test]
    fn multiple() {
        assert_pattern_snapshot!("[a, b, c]");
    }

    #[test]
    fn nested() {
        assert_pattern_snapshot!("[[a], [b, c]]");
    }

    #[test]
    fn with_constructors() {
        assert_pattern_snapshot!("[Just x, Nothing]");
    }

    #[test]
    fn multiline() {
        assert_indented_pattern_snapshot!(
            "[
                a,
                b,
                c
            ]"
        );
    }

    #[test]
    fn error_unclosed() {
        assert_pattern_error_snapshot!("[a, b");
    }

    #[test]
    fn error_trailing_comma() {
        assert_pattern_error_snapshot!("[a, b,]");
    }
}
