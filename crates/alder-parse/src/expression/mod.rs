//! Expression parsing for Alder.
//!
//! Ported from Elm's `Parse/Expression.hs`.

use alder_region::{Located, Position, Region};
use alder_source::{BinOpOperand, Expr};
use bumpalo::collections::Vec as BumpVec;

use crate::Parser;
use crate::error;

mod accessor;
mod case;
mod if_;
mod lambda;
mod let_;
mod list;
mod number;
mod record;
mod string;
mod tuple;
mod variable;

impl<'a> Parser<'a> {
    /// Parse a full expression, returning the expression and end position.
    ///
    /// Mirrors Elm's `expression`:
    /// ```haskell
    /// expression =
    ///   do  start <- getPosition
    ///       oneOf E.Start
    ///         [ let_ start
    ///         , if_ start
    ///         , case_ start
    ///         , function start
    ///         , do  expr <- possiblyNegativeTerm start
    ///               ...
    ///         ]
    /// ```
    ///
    /// Currently implements: lambda, possiblyNegativeTerm + function application.
    /// TODO: let, if, case, operators
    pub fn expression(&mut self) -> Result<(&'a Located<Expr<'a>>, Position), error::Expr<'a>> {
        let start = self.get_position();

        self.one_of(
            error::Expr::Start,
            vec![
                // Let: let defs in expr
                Box::new(|p: &mut Parser<'a>| p.let_(start)),
                // Case: case expr of pattern -> branch ...
                Box::new(|p: &mut Parser<'a>| p.case_(start)),
                // If: if cond then expr else expr
                Box::new(|p: &mut Parser<'a>| p.if_(start)),
                // Lambda: \args -> body
                Box::new(|p: &mut Parser<'a>| p.lambda(start)),
                // Term (possibly negated) with function application
                Box::new(|p| {
                    let expr = p.possibly_negative_term(start)?;
                    let end = p.get_position();
                    p.chomp(error::Expr::Space)?;
                    p.chomp_expr_end(start, expr, vec![], end)
                }),
            ],
        )
    }

    /// Handle function application and binary operators.
    ///
    /// Mirrors Elm's `chompExprEnd`:
    /// ```haskell
    /// chompExprEnd start (State ops expr args end) =
    ///   oneOfWithFallback
    ///     [ -- argument
    ///       do  Space.checkIndent end E.Start
    ///           arg <- term
    ///           ...
    ///     , -- operator
    ///       do  Space.checkIndent end E.Start
    ///           op <- addLocation (Symbol.operator ...)
    ///           ...
    ///     ]
    ///     -- done: finalize with toCall and possibly wrap in Binops
    /// ```
    pub(crate) fn chomp_expr_end(
        &mut self,
        start: Position,
        expr: &'a Located<Expr<'a>>,
        args: Vec<&'a Located<Expr<'a>>>,
        end: Position,
    ) -> Result<(&'a Located<Expr<'a>>, Position), error::Expr<'a>> {
        // Track ops for binary operator chains
        let mut ops: BumpVec<'a, &'a BinOpOperand<'a>> = BumpVec::new_in(self.bump);
        let mut current_expr = expr;
        let mut current_args = args;
        let mut current_end = end;

        loop {
            let state_for_fallback = (ops.clone(), current_expr, current_args.clone(), current_end);

            let result = self.one_of_with_fallback(
                vec![
                    // argument - function application
                    Box::new(|p: &mut Parser<'a>| {
                        let (row, col) = p.position();
                        p.check_indent(row, col, error::Expr::Start)?;
                        let arg = p.term()?;
                        let new_end = p.get_position();
                        p.chomp(error::Expr::Space)?;

                        let mut new_args = current_args.clone();
                        new_args.push(arg);

                        Ok(ExprEndState::MoreArgs(new_args, new_end))
                    }),
                    // operator
                    Box::new(|p: &mut Parser<'a>| {
                        let (row, col) = p.position();
                        p.check_indent(row, col, error::Expr::Start)?;

                        // Save positions for negative-term detection
                        let op_start = p.get_position();

                        let op = p.add_location_operator(
                            error::Expr::Start,
                            error::Expr::OperatorReserved,
                        )?;
                        let op_name = op.value;
                        let op_end = p.get_position();

                        p.chomp_and_check_indent(error::Expr::Space, |row, col| {
                            error::Expr::IndentOperatorRight(op_name, row, col)
                        })?;

                        let new_start = p.get_position();

                        // Check for negative term: `-` operator where there's no space before
                        // but space after (e.g., `a -b` means apply `a` to `-b`)
                        if op_name == "-"
                            && current_end != op_start // space before operator
                            && op_end == new_start
                        // no space after operator
                        {
                            // This is a negative term being passed as an argument
                            let negated_expr = p.term()?;
                            let neg_end = p.get_position();
                            let neg_region = Region::new(op_start, neg_end);
                            let neg = p.alloc(Located::at(neg_region, Expr::Negate(negated_expr)));
                            p.chomp(error::Expr::Space)?;

                            let mut new_args = current_args.clone();
                            new_args.push(neg);

                            Ok(ExprEndState::MoreArgs(new_args, neg_end))
                        } else {
                            // Regular binary operator
                            p.one_of(
                                |row, col| error::Expr::OperatorRight(op_name, row, col),
                                vec![
                                    // Parse a term (possibly negative)
                                    Box::new(|p: &mut Parser<'a>| {
                                        let new_expr = p.possibly_negative_term(new_start)?;
                                        let new_end = p.get_position();
                                        p.chomp(error::Expr::Space)?;

                                        Ok(ExprEndState::MoreOps(op, new_expr, new_end))
                                    }),
                                    // Parse a "final" expression (let, case, if, lambda)
                                    Box::new(|p: &mut Parser<'a>| {
                                        let (final_expr, final_end) = p.one_of(
                                            |row, col| {
                                                error::Expr::OperatorRight(op_name, row, col)
                                            },
                                            vec![
                                                Box::new(|p: &mut Parser<'a>| p.let_(new_start)),
                                                Box::new(|p| p.case_(new_start)),
                                                Box::new(|p| p.if_(new_start)),
                                                Box::new(|p| p.lambda(new_start)),
                                            ],
                                        )?;

                                        Ok(ExprEndState::Final(op, final_expr, final_end))
                                    }),
                                ],
                            )
                        }
                    }),
                ],
                ExprEndState::Done,
            )?;

            match result {
                ExprEndState::MoreArgs(new_args, new_end) => {
                    current_args = new_args;
                    current_end = new_end;
                }
                ExprEndState::MoreOps(op, new_expr, new_end) => {
                    // Push (toCall current_expr current_args, op) onto ops
                    let call_expr = to_call(self, start, current_expr, current_args.clone());
                    let operand = self.alloc(BinOpOperand {
                        expr: call_expr,
                        op,
                    });
                    ops.push(operand);
                    current_expr = new_expr;
                    current_args = Vec::new();
                    current_end = new_end;
                }
                ExprEndState::Final(op, final_expr, final_end) => {
                    // Push current and build final Binops
                    let call_expr = to_call(self, start, current_expr, current_args);
                    let operand = self.alloc(BinOpOperand {
                        expr: call_expr,
                        op,
                    });
                    ops.push(operand);

                    let ops_slice = ops.into_bump_slice();
                    let binops = Expr::BinOps {
                        operands: ops_slice,
                        last: final_expr,
                    };
                    let result = self.alloc(Located::at(Region::new(start, final_end), binops));
                    return Ok((result, final_end));
                }
                ExprEndState::Done => {
                    // Finalize - use saved state
                    let (saved_ops, saved_expr, saved_args, saved_end) = state_for_fallback;
                    let final_call = to_call(self, start, saved_expr, saved_args);

                    if saved_ops.is_empty() {
                        return Ok((final_call, saved_end));
                    } else {
                        let ops_slice = saved_ops.into_bump_slice();
                        let binops = Expr::BinOps {
                            operands: ops_slice,
                            last: final_call,
                        };
                        let result = self.alloc(Located::at(Region::new(start, saved_end), binops));
                        return Ok((result, saved_end));
                    }
                }
            }
        }
    }

    /// Parse possibly negated term: `-term` or `term`.
    ///
    /// Mirrors Elm's `possiblyNegativeTerm`:
    /// ```haskell
    /// possiblyNegativeTerm start =
    ///   oneOf E.Start
    ///     [ do  word1 0x2D {---} E.Start
    ///           expr <- term
    ///           addEnd start (Src.Negate expr)
    ///     , term
    ///     ]
    /// ```
    fn possibly_negative_term(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        self.one_of(
            error::Expr::Start,
            vec![
                Box::new(|p: &mut Parser<'a>| {
                    p.word1(b'-', error::Expr::Start)?;
                    let expr = p.term()?;
                    Ok(p.add_end(start, Expr::Negate(expr)))
                }),
                Box::new(|p| p.term()),
            ],
        )
    }

    /// Parse a term (atomic expression).
    ///
    /// Mirrors Elm's `term`:
    /// ```haskell
    /// term =
    ///   do  start <- getPosition
    ///       oneOf E.Start
    ///         [ variable start >>= accessible start
    ///         , string start
    ///         , number start
    ///         , ...
    ///         ]
    /// ```
    pub fn term(&mut self) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let start = self.get_position();

        self.one_of(
            error::Expr::Start,
            vec![
                Box::new(|p: &mut Parser<'a>| {
                    let expr = p.variable(start)?;
                    p.accessible(start, expr)
                }),
                Box::new(|p| p.string(start)),
                Box::new(|p| p.number(start)),
                Box::new(|p| p.list(start)),
                Box::new(|p| {
                    let expr = p.record(start)?;
                    p.accessible(start, expr)
                }),
                Box::new(|p| {
                    let expr = p.tuple(start)?;
                    p.accessible(start, expr)
                }),
                Box::new(|p| p.accessor(start)),
            ],
        )
    }
}

/// Convert function + args into a Call expression.
///
/// Mirrors Elm's `toCall`:
/// ```haskell
/// toCall func revArgs =
///   case revArgs of
///     [] -> func
///     lastArg : _ -> A.merge func lastArg (Src.Call func (reverse revArgs))
/// ```
fn to_call<'a>(
    parser: &Parser<'a>,
    _start: Position,
    func: &'a Located<Expr<'a>>,
    args: Vec<&'a Located<Expr<'a>>>,
) -> &'a Located<Expr<'a>> {
    if args.is_empty() {
        func
    } else {
        let last_arg = args.last().unwrap();
        let region = Region::span_across(&func.region, &last_arg.region);
        let args_slice = parser.alloc_slice_copy(&args);
        parser.alloc(Located::at(
            region,
            Expr::Call {
                function: func,
                arguments: args_slice,
            },
        ))
    }
}

/// State for expression end parsing (function application and binary operators).
enum ExprEndState<'a> {
    /// More function arguments accumulated
    MoreArgs(Vec<&'a Located<Expr<'a>>>, Position),
    /// Binary operator found, continue parsing chain
    MoreOps(&'a Located<&'a str>, &'a Located<Expr<'a>>, Position),
    /// Final expression found (let, case, if, lambda) after operator
    Final(&'a Located<&'a str>, &'a Located<Expr<'a>>, Position),
    /// Done parsing, finalize expression
    Done,
}

/// Snapshot test macro for successful expression parsing.
#[cfg(test)]
macro_rules! assert_expr_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let src = bump.alloc_str(indoc::indoc!($code));
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let result = parser.term().expect("expected successful parse");

        insta::with_settings!({
            description => format!("Code:\n\n{}", indoc::indoc!($code)),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }};
}

/// Snapshot test macro for expression parse errors.
#[cfg(test)]
macro_rules! assert_expr_error_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let src = bump.alloc_str(indoc::indoc!($code));
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let result = parser.term().expect_err("expected parse error");

        insta::with_settings!({
            description => format!("Code:\n\n{}", indoc::indoc!($code)),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }};
}

/// Snapshot test macro for full expression parsing.
#[cfg(test)]
macro_rules! assert_expression_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let src = bump.alloc_str(indoc::indoc!($code));
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let (result, _end) = parser.expression().expect("expected successful parse");

        insta::with_settings!({
            description => format!("Code:\n\n{}", indoc::indoc!($code)),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }};
}

/// Snapshot test macro for multiline terms, laid out as they would appear
/// indented inside a definition. Bare fragments cannot use multiline
/// layout: a token at column 1 always starts a new top-level declaration.
#[cfg(test)]
macro_rules! assert_indented_expr_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let fragment = indoc::indoc!($code);
        let indented = $crate::test_support::indent_fragment(fragment);
        let src = bump.alloc_str(&indented);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        parser
            .chomp(|_, _, _| "space error")
            .expect("expected leading indent");
        let result = parser.term().expect("expected successful parse");

        insta::with_settings!({
            description => format!("Code (indented inside a def):\n\n{}", indented),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }};
}

/// Like `assert_indented_expr_snapshot` but for full expressions.
#[cfg(test)]
macro_rules! assert_indented_expression_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let fragment = indoc::indoc!($code);
        let indented = $crate::test_support::indent_fragment(fragment);
        let src = bump.alloc_str(&indented);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        parser
            .chomp(|_, _, _| "space error")
            .expect("expected leading indent");
        let (result, _end) = parser.expression().expect("expected successful parse");

        insta::with_settings!({
            description => format!("Code (indented inside a def):\n\n{}", indented),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }};
}

#[cfg(test)]
pub(crate) use assert_expr_error_snapshot;
#[cfg(test)]
pub(crate) use assert_expr_snapshot;
#[cfg(test)]
pub(crate) use assert_expression_snapshot;
#[cfg(test)]
pub(crate) use assert_indented_expr_snapshot;
#[cfg(test)]
pub(crate) use assert_indented_expression_snapshot;

#[cfg(test)]
mod tests {
    #[test]
    fn negate_var() {
        assert_expression_snapshot!("-x");
    }

    #[test]
    fn negate_number() {
        assert_expression_snapshot!("-42");
    }

    #[test]
    fn negate_parens() {
        assert_expression_snapshot!("-(a)");
    }

    #[test]
    fn expr_simple_var() {
        assert_expression_snapshot!("foo");
    }

    #[test]
    fn expr_simple_number() {
        assert_expression_snapshot!("123");
    }

    #[test]
    fn application_single() {
        assert_expression_snapshot!("f x");
    }

    #[test]
    fn application_multiple() {
        assert_expression_snapshot!("f x y z");
    }

    #[test]
    fn application_nested() {
        assert_expression_snapshot!("f (g x)");
    }

    #[test]
    fn application_with_record() {
        assert_expression_snapshot!("f { x = 1 }");
    }

    // Binary operators
    #[test]
    fn binop_simple_add() {
        assert_expression_snapshot!("a + b");
    }

    #[test]
    fn binop_simple_subtract() {
        assert_expression_snapshot!("a - b");
    }

    #[test]
    fn binop_simple_multiply() {
        assert_expression_snapshot!("a * b");
    }

    #[test]
    fn binop_simple_divide() {
        assert_expression_snapshot!("a / b");
    }

    #[test]
    fn binop_chained() {
        assert_expression_snapshot!("a + b + c");
    }

    #[test]
    fn binop_mixed() {
        assert_expression_snapshot!("a + b * c");
    }

    #[test]
    fn binop_with_parens() {
        assert_expression_snapshot!("(a + b) * c");
    }

    #[test]
    fn binop_pipe_right() {
        assert_expression_snapshot!("a |> b |> c");
    }

    #[test]
    fn binop_pipe_left() {
        assert_expression_snapshot!("c <| b <| a");
    }

    #[test]
    fn binop_comparison() {
        assert_expression_snapshot!("a == b");
    }

    #[test]
    fn binop_less_than() {
        assert_expression_snapshot!("a < b");
    }

    #[test]
    fn binop_greater_than() {
        assert_expression_snapshot!("a > b");
    }

    #[test]
    fn binop_append() {
        assert_expression_snapshot!("a ++ b");
    }

    #[test]
    fn binop_cons() {
        assert_expression_snapshot!("a :: b");
    }

    #[test]
    fn binop_logical_and() {
        assert_expression_snapshot!("a && b");
    }

    #[test]
    fn binop_logical_or() {
        assert_expression_snapshot!("a || b");
    }

    #[test]
    fn binop_less_than_or_equal() {
        assert_expression_snapshot!("a <= b");
    }

    #[test]
    fn binop_greater_than_or_equal() {
        assert_expression_snapshot!("a >= b");
    }

    #[test]
    fn binop_not_equal() {
        assert_expression_snapshot!("a /= b");
    }

    #[test]
    fn binop_compose_right() {
        assert_expression_snapshot!("f >> g");
    }

    #[test]
    fn binop_compose_left() {
        assert_expression_snapshot!("f << g");
    }

    #[test]
    fn binop_power() {
        assert_expression_snapshot!("a ^ b");
    }

    #[test]
    fn binop_with_function_application() {
        assert_expression_snapshot!("f x + g y");
    }

    #[test]
    fn binop_with_negation() {
        assert_expression_snapshot!("a + -b");
    }

    #[test]
    fn binop_negative_first() {
        assert_expression_snapshot!("-a + b");
    }

    #[test]
    fn binop_with_let() {
        assert_expression_snapshot!("a + let x = 1 in x");
    }

    #[test]
    fn binop_with_if() {
        assert_expression_snapshot!("a + if b then c else d");
    }

    #[test]
    fn binop_with_case() {
        assert_expression_snapshot!(
            r#"
            a + case x of
                1 -> y
                _ -> z
            "#
        );
    }

    #[test]
    fn binop_with_lambda() {
        assert_expression_snapshot!("a + \\x -> x");
    }

    // Operator sections
    #[test]
    fn op_section_plus() {
        assert_expression_snapshot!("(+)");
    }

    #[test]
    fn op_section_minus() {
        assert_expression_snapshot!("(-)");
    }

    #[test]
    fn op_section_multiply() {
        assert_expression_snapshot!("(*)");
    }

    #[test]
    fn op_section_append() {
        assert_expression_snapshot!("(++)");
    }

    #[test]
    fn op_section_pipe_right() {
        assert_expression_snapshot!("(|>)");
    }

    #[test]
    fn op_section_cons() {
        assert_expression_snapshot!("(::)");
    }

    // Negation vs subtraction
    #[test]
    fn application_with_negative_arg() {
        // `f -x` means f applied to -x (negative x), not f minus x
        assert_expression_snapshot!("f -x");
    }

    #[test]
    fn negation_in_parens() {
        assert_expression_snapshot!("(-42)");
    }

    #[test]
    fn negation_in_tuple() {
        assert_expression_snapshot!("(-a, b)");
    }
}
