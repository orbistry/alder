//! Unit, parenthesized and tuple patterns.
//!
//! `()` is `Pattern::Unit` (§10.17), `(p)` is `p` itself (the parentheses
//! leave no node, exactly as Elm's `tupleHelp` returns `firstPattern`), so
//! the node's region excludes the parentheses while the cursor is past `)`:
//! `(x)` is `Var("x")` at 1:2-1:3 with the cursor at 1:4. `Pattern` is not
//! `Copy`, so re-wrapping the inner value with the paren region would need
//! an AST change. `(p, q, …)` is `Pattern::Tuple`. A trailing comma is
//! accepted (§10.8), so `(p,)` is also just `p`.
//!
//! See docs/parser-internals.md §5.14.
// OWNER: pattern/tuple.rs (Wave 1)

use alder_region::{Located, Position};
use alder_source::Pattern;
use bumpalo::collections::Vec as BumpVec;

use crate::error::PTuple;
use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// At `(`.
    pub(super) fn pattern_tuple(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, error::PTuple<'a>> {
        self.advance();
        self.chomp();
        if self.peek() == Some(b')') {
            self.advance();
            return Ok(self.add_end(start, Pattern::Unit));
        }
        let first = self.pattern_tuple_entry()?;
        let mut rest = BumpVec::new_in(self.bump);
        loop {
            match self.peek() {
                Some(b',') => {
                    self.advance();
                    self.chomp();
                    if self.peek() == Some(b')') {
                        self.advance();
                        break;
                    }
                    rest.push(self.pattern_tuple_entry()?);
                }
                Some(b')') => {
                    self.advance();
                    break;
                }
                _ => {
                    let (row, col) = self.position();
                    return Err(PTuple::End(row, col));
                }
            }
        }
        if rest.is_empty() {
            return Ok(first);
        }
        let second = rest.remove(0);
        Ok(self.add_end(
            start,
            Pattern::Tuple {
                first,
                second,
                rest: rest.into_bump_slice(),
            },
        ))
    }

    fn pattern_tuple_entry(&mut self) -> Result<&'a Located<Pattern<'a>>, PTuple<'a>> {
        self.specialize(
            |bump, e, row, col| PTuple::Pattern(bump.alloc(e), row, col),
            |p| p.pattern(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::{assert_pattern_error_snapshot, assert_pattern_snapshot};

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
    fn parenthesized_single() {
        assert_pattern_snapshot!("(x)");
    }

    #[test]
    fn parenthesized_trailing_comma() {
        assert_pattern_snapshot!("(x,)");
    }

    #[test]
    fn error_unclosed() {
        assert_pattern_error_snapshot!("(a, b");
    }
}
