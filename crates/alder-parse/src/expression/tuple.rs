//! Tuple, unit, and parenthesized expression parsing for Alder.
//!
//! Ported from Elm's tuple parsing in `Parse/Expression.hs`.
//!
//! Handles:
//! - `()` → Unit
//! - `(expr)` → parenthesized expression (returned unwrapped)
//! - `(a, b)`, `(a, b, c, ...)` → Tuple

use alder_region::{Located, Position};
use alder_source::Expr;
use bumpalo::collections::Vec as BumpVec;

use crate::Parser;
use crate::error::{self, Tuple};

impl<'a> Parser<'a> {
    /// Parse a tuple, unit, or parenthesized expression.
    ///
    /// Mirrors Elm's `tuple`:
    /// ```haskell
    /// tuple start@(A.Position row col) =
    ///   inContext E.Tuple (word1 0x28 {-(-} E.Start) $
    ///     do  ...
    /// ```
    pub(crate) fn tuple(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        self.in_context(
            // Wrap Tuple errors with Expr::Tuple context
            |bump, tuple_err, row, col| error::Expr::Tuple(bump.alloc(tuple_err), row, col),
            // Start parser: parse '('
            |p| p.word1(0x28, error::Expr::Start),
            // Body parser: parse tuple contents
            |p| p.tuple_body(start),
        )
    }

    /// Parse the body of a tuple after the opening '('.
    ///
    /// Returns `Tuple` errors which get wrapped by `in_context`.
    ///
    /// Handles:
    /// - `()` → Unit
    /// - `(expr)` → parenthesized expression
    /// - `(a, b, ...)` → Tuple
    /// - `(+)` → Operator section (Op)
    /// - `(-expr)` → Parenthesized negation (parsed as expression)
    fn tuple_body(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, Tuple<'a>> {
        let before = self.get_position();
        // Chomp whitespace and check indent
        self.chomp_and_check_indent(Tuple::Space, Tuple::IndentExpr1)?;
        let after = self.get_position();

        // If whitespace was consumed, parse expression normally
        if before != after {
            self.one_of(
                Tuple::IndentExpr1,
                vec![
                    // Expression (might be parenthesized or start of tuple)
                    Box::new(|p: &mut Parser<'a>| {
                        let (first, end) = p.tuple_expr()?;
                        p.check_indent(end.line, end.column, Tuple::IndentEnd)?;
                        p.chomp_tuple_end(start, first)
                    }),
                ],
            )
        } else {
            // No whitespace - check for operator section or unit
            self.one_of(
                Tuple::IndentExpr1,
                vec![
                    // Operator section: `(+)`, `(++)`, etc.
                    // Note: `-` followed by `)` is `(-)`, otherwise it's negation
                    Box::new(|p: &mut Parser<'a>| {
                        let op = p.operator(Tuple::IndentExpr1, Tuple::OperatorReserved)?;

                        if op == "-" {
                            // Special case: `-` could be negation or minus operator
                            p.one_of(
                                Tuple::OperatorClose,
                                vec![
                                    // Just `(-)` - minus operator section
                                    Box::new(|p: &mut Parser<'a>| {
                                        p.word1(0x29, Tuple::OperatorClose)?;
                                        Ok(p.add_end(start, Expr::Op(op)))
                                    }),
                                    // `(-expr)` or `(-expr, ...)` - negation followed by more
                                    Box::new(|p: &mut Parser<'a>| {
                                        // Parse the negation as part of the expression
                                        let neg_start = Position::new(start.line, start.column + 1);
                                        let inner = p.specialize(
                                            |bump, e, row, col| {
                                                Tuple::Expr(bump.alloc(e), row, col)
                                            },
                                            |p| p.term(),
                                        )?;
                                        let neg_end = p.get_position();
                                        let neg_region =
                                            alder_region::Region::new(neg_start, neg_end);
                                        let negated = p.alloc(alder_region::Located::at(
                                            neg_region,
                                            Expr::Negate(inner),
                                        ));

                                        // Now handle rest of expression (function application, operators)
                                        p.chomp(Tuple::Space)?;

                                        let (full_expr, expr_end) = p.specialize(
                                            |bump, e, row, col| {
                                                Tuple::Expr(bump.alloc(e), row, col)
                                            },
                                            |p| p.chomp_expr_end(start, negated, vec![], neg_end),
                                        )?;

                                        p.check_indent(
                                            expr_end.line,
                                            expr_end.column,
                                            Tuple::IndentEnd,
                                        )?;
                                        p.chomp_tuple_end(start, full_expr)
                                    }),
                                ],
                            )
                        } else {
                            // Regular operator section
                            p.word1(0x29, Tuple::OperatorClose)?;
                            Ok(p.add_end(start, Expr::Op(op)))
                        }
                    }),
                    // Unit: just ')'
                    Box::new(|p: &mut Parser<'a>| {
                        p.word1(0x29, Tuple::IndentExpr1)?;
                        Ok(p.add_end(start, Expr::Unit))
                    }),
                    // Expression (might be parenthesized or start of tuple)
                    Box::new(|p: &mut Parser<'a>| {
                        let (first, end) = p.tuple_expr()?;
                        p.check_indent(end.line, end.column, Tuple::IndentEnd)?;
                        p.chomp_tuple_end(start, first)
                    }),
                ],
            )
        }
    }

    /// Parse a tuple entry expression.
    ///
    /// In Elm this uses `specialize E.TupleExpr expression`.
    /// Returns both the expression and its end position for indent checking.
    fn tuple_expr(&mut self) -> Result<(&'a Located<Expr<'a>>, Position), Tuple<'a>> {
        self.specialize(
            |bump, expr_err, row, col| Tuple::Expr(bump.alloc(expr_err), row, col),
            |p| p.expression(),
        )
    }

    /// Parse the rest of a tuple after the first expression.
    ///
    /// Mirrors Elm's `chompTupleEnd`:
    /// ```haskell
    /// chompTupleEnd start firstExpr revExprs =
    ///   oneOf E.TupleEnd
    ///     [ do  word1 0x2C {-,-} E.TupleEnd
    ///           ...
    ///           chompTupleEnd start firstExpr (entry : revExprs)
    ///     , do  word1 0x29 {-)-} E.TupleEnd
    ///           case reverse revExprs of
    ///             [] -> return firstExpr  -- parenthesized
    ///             secondExpr : otherExprs ->
    ///               addEnd start (Src.Tuple firstExpr secondExpr otherExprs)
    ///     ]
    /// ```
    fn chomp_tuple_end(
        &mut self,
        start: Position,
        first: &'a Located<Expr<'a>>,
    ) -> Result<&'a Located<Expr<'a>>, Tuple<'a>> {
        let mut rest: BumpVec<'a, &'a Located<Expr<'a>>> = BumpVec::new_in(self.bump);

        loop {
            // Chomp whitespace
            self.chomp(Tuple::Space)?;

            // Expect comma or closing paren
            let done = self.one_of(
                Tuple::End,
                vec![
                    // Comma - parse another expression
                    Box::new(|p: &mut Parser<'a>| {
                        p.word1(0x2C, Tuple::End)?;

                        // Chomp whitespace and check indent after comma
                        p.chomp_and_check_indent(Tuple::Space, Tuple::IndentExprN)?;

                        // Parse the expression
                        let (elem, end) = p.tuple_expr()?;
                        rest.push(elem);

                        // Check indent using expression's end position
                        p.check_indent(end.line, end.column, Tuple::IndentEnd)?;

                        Ok(false) // Not done, continue loop
                    }),
                    // Closing paren - done
                    Box::new(|p: &mut Parser<'a>| {
                        p.word1(0x29, Tuple::End)?;
                        Ok(true) // Done
                    }),
                ],
            )?;

            if done {
                break;
            }
        }

        // Determine what we parsed
        if rest.is_empty() {
            // Just parenthesized expression - return unwrapped
            Ok(first)
        } else {
            // Tuple: need at least 2 elements
            let second = rest.remove(0);
            let others = rest.into_bump_slice();
            Ok(self.add_end(
                start,
                Expr::Tuple {
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
    use crate::expression::{
        assert_expr_error_snapshot, assert_expr_snapshot, assert_indented_expr_snapshot,
    };

    #[test]
    fn unit() {
        assert_expr_snapshot!("()");
    }

    #[test]
    fn parenthesized() {
        assert_expr_snapshot!("(42)");
    }

    #[test]
    fn parenthesized_var() {
        assert_expr_snapshot!("(foo)");
    }

    #[test]
    fn pair() {
        assert_expr_snapshot!("(1, 2)");
    }

    #[test]
    fn triple() {
        assert_expr_snapshot!("(1, 2, 3)");
    }

    #[test]
    fn with_whitespace() {
        assert_expr_snapshot!("( 1 , 2 , 3 )");
    }

    #[test]
    fn nested() {
        assert_expr_snapshot!("((1, 2), 3)");
    }

    #[test]
    fn nested_list() {
        assert_expr_snapshot!("([1, 2], [3, 4])");
    }

    #[test]
    fn multiline() {
        assert_indented_expr_snapshot!(
            "(
                1,
                2,
                3
            )"
        );
    }

    #[test]
    fn error_unclosed() {
        assert_expr_error_snapshot!("(1, 2");
    }

    #[test]
    fn error_trailing_comma() {
        assert_expr_error_snapshot!("(1, 2,)");
    }

    #[test]
    fn error_empty_comma() {
        assert_expr_error_snapshot!("(,)");
    }
}
