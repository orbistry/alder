//! `fn(params) [-> Type] body` lambdas.
//!
//! See docs/parser-internals.md §5.13 and §10.13:
//!
//! ```ebnf
//! lambda = 'fn' '(' [ params ] ')' [ '->' type ] ( block | assign | expression ) ;
//! ```
//!
//! A body starting with `{` is always a block (§2.2), so `fn() { x }`
//! returns `x` rather than building a record. Any other body goes through
//! `expr_or_assign` (§5.12): an expression is the body itself; an
//! assignment (`fn() count += 1`) is wrapped as a one-statement block with
//! no tail. The lambda's region ends where its body ends; `primary` does
//! not chomp, but a body parsed by `block()` / `expr_or_assign()` has
//! already chomped its trailing whitespace.
//!
//! A `{` body runs under `with_record_ctor(true, …)`: it is a brace context
//! that grammatically demands a block (§2.2), so it resets `no_record_ctor`
//! like the brackets of §2.3 (Rust clears its struct-literal restriction
//! inside blocks the same way). This only matters when the lambda itself
//! sits unbracketed in an `if` / `while` / `for` / `match` head; the same
//! choice is made for `if` branches and `match` arms (see those modules).
// OWNER: expression/lambda.rs (Wave 2)

use alder_region::{Located, Position, Region};
use alder_source::{Block, Expr, Lambda, Stmt};

use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// After `fn`.
    pub(crate) fn lambda(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let (row, col) = (start.line, start.column);
        let lambda = self
            .lambda_body()
            .map_err(|e| error::Expr::Lambda(self.alloc(e), row, col))?;
        let end = lambda.body.region.end;
        Ok(self.alloc(Located::at(
            Region::new(start, end),
            Expr::Lambda(self.alloc(lambda)),
        )))
    }

    fn lambda_body(&mut self) -> Result<Lambda<'a>, error::Lambda<'a>> {
        self.chomp();
        let params = self.specialize(
            |bump, e, row, col| error::Lambda::Params(bump.alloc(e), row, col),
            |p| p.params(),
        )?;
        self.chomp();
        let ret = if self.peek() == Some(b'-') && self.peek_at(1) == Some(b'>') {
            self.advance_by(2);
            self.chomp();
            Some(self.specialize(
                |bump, e, row, col| error::Lambda::Ret(bump.alloc(e), row, col),
                |p| p.type_expr(),
            )?)
        } else {
            None
        };
        let body = if self.peek() == Some(b'{') {
            let block = self.specialize(
                |bump, e, row, col| error::Lambda::Block(bump.alloc(e), row, col),
                |p| p.with_record_ctor(true, |p| p.block()),
            )?;
            self.alloc(Located::at(block.region, Expr::Block(block)))
        } else {
            let body_start = self.position();
            let stmt = self
                .expr_or_assign()
                .map_err(|e| lambda_body_error(self, e, body_start))?;
            match stmt.value {
                Stmt::Expr(expr) => expr,
                _ => {
                    // An assignment body is a synthetic one-statement block
                    // (§10.13) sharing the statement's region.
                    let block = self.alloc(Located::at(
                        stmt.region,
                        Block {
                            stmts: self.alloc_slice_copy(&[stmt]),
                            tail: None,
                        },
                    ));
                    self.alloc(Located::at(stmt.region, Expr::Block(block)))
                }
            }
        };
        Ok(Lambda { params, ret, body })
    }
}

/// Map an `expr_or_assign` failure onto the lambda error vocabulary.
fn lambda_body_error<'a>(
    parser: &Parser<'a>,
    err: error::Stmt<'a>,
    body_start: (crate::Row, crate::Col),
) -> error::Lambda<'a> {
    match err {
        error::Stmt::Expr(e, row, col) => error::Lambda::Body(e, row, col),
        error::Stmt::AssignValue(e, row, col) => error::Lambda::AssignValue(e, row, col),
        // `Stmt::AssignTarget` carries the target's start, i.e. the body start.
        error::Stmt::AssignTarget(op, row, col) => error::Lambda::AssignTarget(op, row, col),
        // `expr_or_assign` produces no other variant.
        _ => {
            let (row, col) = body_start;
            error::Lambda::Body(parser.alloc(error::Expr::Start(row, col)), row, col)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{assert_expression_error_snapshot, assert_expression_snapshot};

    #[test]
    fn lambda_expr_body() {
        assert_expression_snapshot!("fn(x) x + 1");
    }

    #[test]
    fn lambda_no_params() {
        assert_expression_snapshot!("fn() 1");
    }

    #[test]
    fn lambda_multiple_params() {
        assert_expression_snapshot!("fn(a, b) a + b");
    }

    #[test]
    fn lambda_typed_params() {
        assert_expression_snapshot!("fn(a: Number, b: String) a");
    }

    #[test]
    fn lambda_return_type() {
        assert_expression_snapshot!("fn(x) -> Number { x * 2 }");
    }

    #[test]
    fn lambda_block_body() {
        assert_expression_snapshot!(
            r#"
            fn(x) {
                let y = x * 2
                y + 1
            }
        "#
        );
    }

    #[test]
    fn lambda_block_single_name_is_block() {
        assert_expression_snapshot!("fn() { x }");
    }

    #[test]
    fn lambda_assign_body() {
        assert_expression_snapshot!("fn() count += 1");
    }

    #[test]
    fn lambda_pattern_param() {
        assert_expression_snapshot!("fn((a, b)) a + b");
    }

    #[test]
    fn lambda_mut_param() {
        assert_expression_snapshot!("fn(mut x) x");
    }

    #[test]
    fn error_missing_parens() {
        assert_expression_error_snapshot!("fn x");
    }

    #[test]
    fn error_missing_body() {
        assert_expression_error_snapshot!("fn()");
    }

    #[test]
    fn error_bad_param() {
        assert_expression_error_snapshot!("fn(+) 1");
    }

    #[test]
    fn error_assign_no_value() {
        assert_expression_error_snapshot!("fn() x +=");
    }

    #[test]
    fn error_assign_bad_target() {
        assert_expression_error_snapshot!("fn() 1 += 2");
    }

    #[test]
    fn error_assign_bad_target_slash_equals() {
        assert_expression_error_snapshot!("fn() f() /= 2");
    }
}
