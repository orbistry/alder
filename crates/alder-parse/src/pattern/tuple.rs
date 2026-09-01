//! Tuple pattern parsing for Alder.
//!
//! Ported from Elm's `Pattern.tuple`.
//!
//! Handles: `()`, `(p)`, `(p1, p2)`, `(p1, p2, p3, ...)`

use alder_region::{Located, Position};
use alder_source::Pattern;
use bumpalo::collections::Vec as BumpVec;

use crate::Parser;
use crate::error::{self, PTuple};

impl<'a> Parser<'a> {
    /// Parse a tuple pattern: `()`, `(p)`, `(p1, p2, ...)`
    pub(super) fn pattern_tuple(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        self.in_context(
            |bump, tuple_err, row, col| error::Pattern::Tuple(bump.alloc(tuple_err), row, col),
            |p| p.word1(0x28, error::Pattern::Start),
            |p| p.pattern_tuple_body(start),
        )
    }

    /// Parse tuple pattern body after `(`.
    fn pattern_tuple_body(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, PTuple<'a>> {
        self.chomp_and_check_indent(PTuple::Space, PTuple::IndentExpr1)?;

        self.one_of(
            PTuple::Open,
            vec![
                // Unit: `()`
                Box::new(|p: &mut Parser<'a>| {
                    p.word1(0x29, PTuple::Open)?;
                    Ok(p.add_end(start, Pattern::Unit))
                }),
                // Pattern (might be parenthesized or tuple)
                Box::new(|p: &mut Parser<'a>| {
                    let (first, end) = p.pattern_tuple_entry()?;
                    p.check_indent(end.line, end.column, PTuple::IndentEnd)?;
                    p.pattern_tuple_help(start, first)
                }),
            ],
        )
    }

    /// Parse a pattern inside a tuple.
    /// Returns `(pattern, end)` where end is the position at end of pattern (before any chomp).
    fn pattern_tuple_entry(&mut self) -> Result<(&'a Located<Pattern<'a>>, Position), PTuple<'a>> {
        self.specialize(
            |bump, pat_err, row, col| PTuple::Expr(bump.alloc(pat_err), row, col),
            |p| p.pattern_expr(),
        )
    }

    /// Parse remaining tuple elements.
    fn pattern_tuple_help(
        &mut self,
        start: Position,
        first: &'a Located<Pattern<'a>>,
    ) -> Result<&'a Located<Pattern<'a>>, PTuple<'a>> {
        let mut rest: BumpVec<'a, &'a Located<Pattern<'a>>> = BumpVec::new_in(self.bump);

        loop {
            self.chomp(PTuple::Space)?;

            let done = self.one_of(
                PTuple::End,
                vec![
                    // Comma - another pattern
                    Box::new(|p: &mut Parser<'a>| {
                        p.word1(0x2C, PTuple::End)?;
                        p.chomp_and_check_indent(PTuple::Space, PTuple::IndentExprN)?;

                        let (pat, end) = p.pattern_tuple_entry()?;
                        rest.push(pat);

                        p.check_indent(end.line, end.column, PTuple::IndentEnd)?;
                        Ok(false)
                    }),
                    // Close paren
                    Box::new(|p: &mut Parser<'a>| {
                        p.word1(0x29, PTuple::End)?;
                        Ok(true)
                    }),
                ],
            )?;

            if done {
                break;
            }
        }

        if rest.is_empty() {
            // Just parenthesized pattern
            Ok(first)
        } else {
            // Tuple
            let second = rest.remove(0);
            let others = rest.into_bump_slice();
            Ok(self.add_end(
                start,
                Pattern::Tuple {
                    first,
                    second,
                    rest: others,
                },
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        assert_indented_pattern_snapshot, assert_pattern_error_snapshot, assert_pattern_snapshot,
    };

    #[test]
    fn unit() {
        assert_pattern_snapshot!("()");
    }

    #[test]
    fn parenthesized() {
        assert_pattern_snapshot!("(foo)");
    }

    #[test]
    fn pair() {
        assert_pattern_snapshot!("(a, b)");
    }

    #[test]
    fn triple() {
        assert_pattern_snapshot!("(a, b, c)");
    }

    #[test]
    fn nested() {
        assert_pattern_snapshot!("((a, b), c)");
    }

    #[test]
    fn with_constructors() {
        assert_pattern_snapshot!("(Just x, Nothing)");
    }

    #[test]
    fn multiline() {
        assert_indented_pattern_snapshot!(
            "(
                a,
                b,
                c
            )"
        );
    }

    #[test]
    fn error_unclosed() {
        assert_pattern_error_snapshot!("(a, b");
    }

    #[test]
    fn error_trailing_comma() {
        assert_pattern_error_snapshot!("(a, b,)");
    }
}
