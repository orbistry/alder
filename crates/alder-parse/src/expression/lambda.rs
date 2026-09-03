//! `param -> body` and `(params) [Type] -> body` lambdas.
//!
//! See docs/parser-internals.md §5.13 and §10.13:
//!
//! ```ebnf
//! lambda = lower_ident '->' ( block | assign | expression )
//!        | '(' [ params ] ')' [ type ] '->' ( block | assign | expression ) ;
//! ```
//!
//! A body starting with `{` is always a block (§2.2), so `() -> { x }`
//! returns `x` rather than building a record. Any other body goes through
//! `expr_or_assign` (§5.12): an expression is the body itself; an
//! assignment (`() -> count += 1`) is wrapped as a one-statement block with
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

use alder_region::{Located, Region};
use alder_source::{Block, Expr, Lambda, Param, Pattern, Stmt};

use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// At the start of a possible lambda. Returns `None` without consuming
    /// when the expression is an ordinary name or parenthesized expression.
    pub(crate) fn try_lambda(&mut self) -> Result<Option<&'a Located<Expr<'a>>>, error::Expr<'a>> {
        let saved = self.save_state();
        let start = self.get_position();
        let (row, col) = (start.line, start.column);

        let (params, ret) = if self.peek_lower()
            && !matches!(self.peek_word(), "true" | "false")
            && crate::Keyword::from_word(self.peek_word()).is_none()
        {
            let pattern_start = self.get_position();
            let name = self.peek_word();
            self.advance_by(name.len());
            let pattern = self.alloc(Located::at(
                Region::new(pattern_start, self.get_position()),
                Pattern::Var(name),
            ));
            let head_end = self.get_position();
            self.chomp();
            if self.newline_since(head_end)
                || self.peek() != Some(b'-')
                || self.peek_at(1) != Some(b'>')
            {
                self.restore_state(saved);
                return Ok(None);
            }
            (
                self.alloc_slice_copy(&[Param {
                    mutable: None,
                    pattern,
                    annotation: None,
                }]),
                None,
            )
        } else if self.peek() == Some(b'(') {
            let params = match self.params() {
                Ok(params) => params,
                Err(_) => {
                    self.restore_state(saved);
                    return Ok(None);
                }
            };
            let params_end = self.get_position();
            self.chomp();
            if self.newline_since(params_end) {
                self.restore_state(saved);
                return Ok(None);
            }
            let ret = if self.peek() == Some(b'-') && self.peek_at(1) == Some(b'>') {
                None
            } else if self.starts_type() {
                let before_type = self.save_state();
                match self.type_expr() {
                    Ok(ret)
                        if !self.newline_since(ret.region.end)
                            && self.peek() == Some(b'-')
                            && self.peek_at(1) == Some(b'>') =>
                    {
                        Some(ret)
                    }
                    _ => {
                        self.restore_state(before_type);
                        self.restore_state(saved);
                        return Ok(None);
                    }
                }
            } else {
                self.restore_state(saved);
                return Ok(None);
            };
            (params, ret)
        } else {
            return Ok(None);
        };

        self.advance_by(2);
        self.chomp();
        let lambda = self
            .lambda_body(params, ret)
            .map_err(|e| error::Expr::Lambda(self.alloc(e), row, col))?;
        let end = lambda.body.region.end;
        Ok(Some(self.alloc(Located::at(
            Region::new(start, end),
            Expr::Lambda(self.alloc(lambda)),
        ))))
    }

    fn lambda_body(
        &mut self,
        params: &'a [Param<'a>],
        ret: Option<&'a Located<alder_source::Type<'a>>>,
    ) -> Result<Lambda<'a>, error::Lambda<'a>> {
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
        assert_expression_snapshot!("x -> x + 1");
    }

    #[test]
    fn lambda_is_right_associative() {
        assert_expression_snapshot!("x -> y -> x + y");
    }

    #[test]
    fn lambda_no_params() {
        assert_expression_snapshot!("() -> 1");
    }

    #[test]
    fn lambda_multiple_params() {
        assert_expression_snapshot!("(a, b) -> a + b");
    }

    #[test]
    fn lambda_typed_params() {
        assert_expression_snapshot!("(a: Number, b: String) -> a");
    }

    #[test]
    fn lambda_return_type() {
        assert_expression_snapshot!("(x) Number -> { x * 2 }");
    }

    #[test]
    fn lambda_block_body() {
        assert_expression_snapshot!(
            r#"
            x -> {
                let y = x * 2
                y + 1
            }
        "#
        );
    }

    #[test]
    fn lambda_block_single_name_is_block() {
        assert_expression_snapshot!("() -> { x }");
    }

    #[test]
    fn lambda_assign_body() {
        assert_expression_snapshot!("() -> count += 1");
    }

    #[test]
    fn lambda_pattern_param() {
        assert_expression_snapshot!("((a, b)) -> a + b");
    }

    #[test]
    fn lambda_mut_param() {
        assert_expression_snapshot!("(mut x) -> x");
    }

    #[test]
    fn error_missing_parens() {
        assert_expression_error_snapshot!("fn(x) { x }");
    }

    #[test]
    fn error_missing_body() {
        assert_expression_error_snapshot!("() ->");
    }

    #[test]
    fn error_bad_param() {
        assert_expression_error_snapshot!("(+) 1");
    }

    #[test]
    fn error_assign_no_value() {
        assert_expression_error_snapshot!("() -> x +=");
    }

    #[test]
    fn error_assign_bad_target() {
        assert_expression_error_snapshot!("() -> 1 += 2");
    }

    #[test]
    fn error_assign_bad_target_slash_equals() {
        assert_expression_error_snapshot!("() -> f() /= 2");
    }

    /// The lambda ends where its parenthesized body ends, `)` included (§10.43).
    #[test]
    fn lambda_body_parens() {
        assert_expression_snapshot!("() -> (a)");
    }
}
