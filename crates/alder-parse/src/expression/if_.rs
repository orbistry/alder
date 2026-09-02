//! `if` / `else if` / `else` expressions.
//!
//! See docs/parser-internals.md §5.13:
//!
//! ```ebnf
//! if_expr = 'if' expression block { 'else' 'if' expression block } [ 'else' block ] ;
//! ```
//!
//! Every condition is parsed under `no_record_ctor` (§2.3) so
//! `if s == Shape::Empty { … }` opens the branch rather than a record
//! constructor. The branch blocks run under `with_record_ctor(true, …)`:
//! a `{` that grammatically demands a block (§2.2) is a brace context like
//! the brackets of §2.3, so it resets the flag (Rust clears its
//! struct-literal restriction inside blocks the same way). This is
//! observable only when the whole `if` sits unbracketed inside another
//! head; the plain `block()` of statement.rs does not reset by itself, so
//! the reset is applied here, in lambda `{` bodies and around match arms
//! (recorded for §10 so `@if` / `@match` bodies and item bodies agree).
//! `else` may start on the line after the closing `}`. The expression's region ends at the last branch's `}`;
//! `block()` has already chomped past it.
// OWNER: expression/if_.rs (Wave 2)

use alder_region::{Located, Position, Region};
use alder_source::{Expr, IfBranch};
use bumpalo::collections::Vec as BumpVec;

use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// After `if`.
    pub(crate) fn if_(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let (row, col) = (start.line, start.column);
        self.if_body(start)
            .map_err(|e| error::Expr::If(self.alloc(e), row, col))
    }

    /// The branch list, at the first condition. Builds the node here so the
    /// region can end at the last branch's `}`.
    fn if_body(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, error::If<'a>> {
        let mut branches = BumpVec::new_in(self.bump);
        let mut final_else = None;
        let mut end;
        loop {
            self.chomp();
            let condition = self.specialize(
                |bump, e, row, col| error::If::Condition(bump.alloc(e), row, col),
                |p| p.with_record_ctor(false, |p| p.expression()),
            )?;
            // `expression()` chomped; an Elm-style `then` sits right here.
            if self.peek_keyword(b"then") {
                let (row, col) = self.position();
                return Err(error::If::ThenKeyword(row, col));
            }
            let body = self.specialize(
                |bump, e, row, col| error::If::Then(bump.alloc(e), row, col),
                |p| p.with_record_ctor(true, |p| p.block()),
            )?;
            end = body.region.end;
            branches.push(IfBranch { condition, body });
            // `block()` chomped, so `else` on the next line is visible too.
            if !self.peek_keyword(b"else") {
                break;
            }
            self.advance_by(4);
            self.chomp();
            if self.peek_keyword(b"if") {
                self.advance_by(2);
                continue;
            }
            if self.peek() != Some(b'{') {
                let (row, col) = self.position();
                return Err(error::If::ElseBranchStart(row, col));
            }
            let block = self.specialize(
                |bump, e, row, col| error::If::Else(bump.alloc(e), row, col),
                |p| p.with_record_ctor(true, |p| p.block()),
            )?;
            end = block.region.end;
            final_else = Some(block);
            break;
        }
        Ok(self.alloc(Located::at(
            Region::new(start, end),
            Expr::If {
                branches: branches.into_bump_slice(),
                final_else,
            },
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{assert_expression_error_snapshot, assert_expression_snapshot};

    #[test]
    fn if_no_else() {
        assert_expression_snapshot!("if ready { go() }");
    }

    #[test]
    fn if_else() {
        assert_expression_snapshot!("if a { 1 } else { 2 }");
    }

    #[test]
    fn if_else_if() {
        assert_expression_snapshot!("if a { 1 } else if b { 2 }");
    }

    #[test]
    fn if_else_if_else() {
        assert_expression_snapshot!("if a { 1 } else if b { 2 } else { 3 }");
    }

    #[test]
    fn if_multiline() {
        assert_expression_snapshot!(
            r#"
            if n < 0 {
                "negative"
            } else if n == 0 {
                "zero"
            } else {
                "positive"
            }
        "#
        );
    }

    #[test]
    fn if_else_next_line() {
        assert_expression_snapshot!(
            r#"
            if a {
                1
            }
            else {
                2
            }
        "#
        );
    }

    #[test]
    fn if_condition_call() {
        assert_expression_snapshot!("if isReady(x) { go() }");
    }

    #[test]
    fn if_condition_path_no_record_ctor() {
        assert_expression_snapshot!("if s == Shape::Empty { 1 }");
    }

    #[test]
    fn if_condition_parenthesized_record_ctor() {
        assert_expression_snapshot!("if (s == Shape::Rect { width: 1 }) { 2 }");
    }

    #[test]
    fn if_nested() {
        assert_expression_snapshot!("if a { if b { 1 } else { 2 } } else { 3 }");
    }

    #[test]
    fn if_block_tail_values() {
        assert_expression_snapshot!(
            r#"
            if a {
                let x = 1
                x
            } else {
                let y = 2
                y
            }
        "#
        );
    }

    #[test]
    fn error_missing_block() {
        assert_expression_error_snapshot!("if x");
    }

    #[test]
    fn error_then_keyword() {
        assert_expression_error_snapshot!("if x then y else z");
    }

    #[test]
    fn error_else_dangling() {
        assert_expression_error_snapshot!("if x { 1 } else");
    }

    #[test]
    fn error_condition() {
        assert_expression_error_snapshot!("if else { 1 }");
    }
}
