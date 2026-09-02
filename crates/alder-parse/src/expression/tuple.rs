//! Unit, parenthesized and tuple expressions.
//!
//! `()` is `Expr::Unit`, `(e)` is `e` itself (the parentheses leave no node,
//! exactly as Elm's `tupleHelp` returns `firstExpr` and as `pattern/tuple.rs`
//! does), so the node's region excludes the parentheses while the cursor is
//! past `)`; the postfix loop in `expression/mod.rs` tracks that true end
//! itself. `Expr` is not `Copy`, so re-wrapping the inner value with the
//! paren region would need an AST change. `(e, f, …)` is `Expr::Tuple`. A
//! trailing comma is accepted (§10.8), so `(e,)` is also just `e`.
//!
//! Entries are parsed with record constructors re-enabled (§2.3).
//!
//! See docs/parser-internals.md §5.13.
// OWNER: expression/tuple.rs (Wave 2)

use alder_region::{Located, Position};
use alder_source::Expr;
use bumpalo::collections::Vec as BumpVec;

use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// At `(`: unit / parenthesized / tuple.
    pub(crate) fn tuple(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        self.specialize(
            |bump, e, row, col| error::Expr::Tuple(bump.alloc(e), row, col),
            |p| p.with_record_ctor(true, |p| p.tuple_body(start)),
        )
    }

    /// At `(`: through the closing `)`, which is consumed.
    fn tuple_body(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, error::Tuple<'a>> {
        self.advance();
        self.chomp();
        if self.peek() == Some(b')') {
            self.advance();
            return Ok(self.add_end(start, Expr::Unit));
        }
        let first = self.expr_tuple_entry()?;
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
                    rest.push(self.expr_tuple_entry()?);
                }
                Some(b')') => {
                    self.advance();
                    break;
                }
                _ => {
                    let (row, col) = self.position();
                    return Err(error::Tuple::End(row, col));
                }
            }
        }
        if rest.is_empty() {
            return Ok(first);
        }
        let second = rest.remove(0);
        Ok(self.add_end(
            start,
            Expr::Tuple {
                first,
                second,
                rest: rest.into_bump_slice(),
            },
        ))
    }

    fn expr_tuple_entry(&mut self) -> Result<&'a Located<Expr<'a>>, error::Tuple<'a>> {
        self.specialize(
            |bump, e, row, col| error::Tuple::Expr(bump.alloc(e), row, col),
            |p| p.expression(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::{assert_expression_error_snapshot, assert_expression_snapshot};

    #[test]
    fn unit() {
        assert_expression_snapshot!("()");
    }

    #[test]
    fn parenthesized() {
        assert_expression_snapshot!("(x)");
    }

    #[test]
    fn pair() {
        assert_expression_snapshot!("(1, 2)");
    }

    #[test]
    fn triple() {
        assert_expression_snapshot!("(a, b, c)");
    }

    #[test]
    fn nested() {
        assert_expression_snapshot!("((1, 2), 3)");
    }

    #[test]
    fn trailing_comma() {
        assert_expression_snapshot!("(1, 2,)");
    }

    #[test]
    fn multiline() {
        assert_expression_snapshot!(
            r#"
            (
                a,
                b,
            )
            "#
        );
    }

    #[test]
    fn error_unclosed() {
        assert_expression_error_snapshot!("(a, b");
    }

    #[test]
    fn error_empty_comma() {
        assert_expression_error_snapshot!("(,)");
    }
}
