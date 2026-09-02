//! Expressions: the flat binop chain, unary prefixes, the postfix loop and
//! the `primary` dispatch table (docs/parser-internals.md §6.0).
//!
//! Conventions: `expression()`, `unary()` and `postfix()` chomp trailing
//! whitespace and compute their region before the chomp; `primary()` and the
//! individual postfix-op parsers do not chomp. The postfix loop tracks the
//! true end of the node it is extending (`end`) separately from
//! `node.region.end`, because a parenthesized expression is its inner
//! expression (§tuple.rs: `(e)` leaves no node of its own) and so its region
//! stops before the `)`; the newline rules of §2.1 must see the `)`, or
//! `(\n a\n)?` would refuse the `?`. The binop chain uses `last.region.end`,
//! so only `(a\n) -b` / `(a\n) <b` on the `)` line still read the operand
//! as ending on the earlier line (they break the chain; §2.1 rule 2).
//!
//! See docs/parser-internals.md §5.13.
// OWNER: expression/mod.rs (Wave 2)

mod array;
mod if_;
mod lambda;
mod literal;
mod loop_;
mod match_;
mod path;
mod postfix;
mod record;
mod tag;
mod tuple;

use alder_region::{Located, Position, Region};
use alder_source::{BinOp, BinOpOperand, Expr};
use bumpalo::collections::Vec as BumpVec;

use crate::keyword::is_ident_byte;
use crate::{Keyword, Parser, SqlWord, error};

impl<'a> Parser<'a> {
    /// Flat binop chain over `unary`. Chomps trailing whitespace.
    pub fn expression(&mut self) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let mut last = self.unary()?;
        let mut operands = BumpVec::new_in(self.bump);
        loop {
            let saved = self.save_state();
            let Some(op) = self.binop(error::Expr::OperatorReserved)? else {
                break;
            };
            if self.newline_since(last.region.end) && !continues_line(op.value, self.peek()) {
                self.restore_state(saved);
                break;
            }
            self.chomp();
            let operand = match self.unary() {
                Err(error::Expr::Start(row, col)) => {
                    return Err(error::Expr::OperatorRight(op.value, row, col));
                }
                other => other?,
            };
            operands.push(BinOpOperand { expr: last, op });
            last = operand;
        }
        if operands.is_empty() {
            return Ok(last);
        }
        let start = operands[0].expr.region.start;
        Ok(self.expr_at(
            start,
            last.region.end,
            Expr::BinOps {
                operands: operands.into_bump_slice(),
                last,
            },
        ))
    }

    /// `-` / `!` / (query mode) `^` prefix, then `postfix`. A `Start` failure of the
    /// operand becomes `Expr::Unary`; every other operand error propagates (§6.0).
    ///
    /// The operand must follow the prefix immediately (`- x` is `Unary`), the
    /// same adjacency the newline rule relies on to tell `a\n-b` (a new
    /// statement) from `a\n- b` (a continued subtraction).
    pub(crate) fn unary(&mut self) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let start = self.get_position();
        match self.peek() {
            Some(b'-') => {
                self.advance();
                let operand = self.unary_operand()?;
                Ok(self.expr_at(start, operand.region.end, Expr::Negate(operand)))
            }
            Some(b'!') => {
                self.advance();
                let operand = self.unary_operand()?;
                Ok(self.expr_at(start, operand.region.end, Expr::Not(operand)))
            }
            Some(b'^') if self.in_query() => self.pinned_value(),
            _ => self.postfix(),
        }
    }

    /// The operand of `-` / `!`: another unary. Nothing operand-like at all
    /// (`Start`) becomes `Unary` at that position.
    fn unary_operand(&mut self) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        match self.unary() {
            Err(error::Expr::Start(row, col)) => Err(error::Expr::Unary(row, col)),
            other => other,
        }
    }

    /// `primary` then the postfix loop (§6.0). Chomps trailing whitespace.
    pub(crate) fn postfix(&mut self) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let start = self.get_position();
        // A parenthesized expression is its inner expression, whose region
        // stops before the `)`; the loop needs the true end (past the `)`).
        let (mut node, mut end) = if self.peek() == Some(b'(') {
            let node = self.tuple(start)?;
            (node, self.get_position())
        } else {
            let node = self.primary()?;
            (node, node.region.end)
        };
        loop {
            // 1. Tagged template: the backtick must be adjacent (nothing
            //    chomped yet, but a block-ending primary has already chomped
            //    its own trailing whitespace, hence the explicit check).
            if self.peek() == Some(b'`') && self.adjacent_to(end) {
                node = self.tagged_template(node)?;
                end = self.get_position();
                continue;
            }
            // 2. Everything else may follow whitespace; only `.` may follow a
            //    line break (§2.1 rule 1).
            self.chomp();
            let same_line = !self.newline_since(end);
            match self.peek() {
                Some(b'.') if self.peek_at(1) != Some(b'.') => {
                    node = self.dot_suffix(node)?;
                }
                Some(b'(') if same_line => {
                    let arguments = self.specialize(
                        |bump, e, row, col| error::Expr::Call(bump.alloc(e), row, col),
                        |p| p.call_args(),
                    )?;
                    node = self.expr_at(
                        node.region.start,
                        self.get_position(),
                        Expr::Call {
                            function: node,
                            arguments,
                        },
                    );
                }
                Some(b'[') if same_line => {
                    node = self.specialize(
                        |bump, e, row, col| error::Expr::Index(bump.alloc(e), row, col),
                        |p| p.index(node),
                    )?;
                }
                Some(b'?') if same_line && self.peek_at(1) != Some(b'?') => {
                    self.advance();
                    node = self.expr_at(node.region.start, self.get_position(), Expr::Try(node));
                }
                Some(b'{') if same_line && self.record_ctor_allowed() => {
                    let Expr::Path(path) = node.value else {
                        return Ok(node);
                    };
                    node = self.record_ctor(node.region.start, path)?;
                }
                _ => return Ok(node),
            }
            end = self.get_position();
        }
    }

    /// Dispatch table on the first byte/word. Does NOT chomp.
    pub(crate) fn primary(&mut self) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let start = self.get_position();
        let (row, col) = self.position();
        match self.peek() {
            Some(b'0'..=b'9') => self.number(start),
            Some(b'"') => self.string(start),
            Some(b'`') => self.template(start),
            Some(b'(') => self.tuple(start),
            Some(b'[') => self.array(start),
            Some(b'{') => {
                if self.looks_like_record() {
                    self.record(start)
                } else {
                    let block = self.specialize(
                        |bump, e, row, col| error::Expr::Block(bump.alloc(e), row, col),
                        |p| p.block(),
                    )?;
                    Ok(self.expr_at(start, block.region.end, Expr::Block(block)))
                }
            }
            Some(b'<') => match self.peek_at(1) {
                Some(b'/') => Err(error::Expr::UnexpectedClose(row, col)),
                Some(b) if b.is_ascii_alphabetic() || b == b'>' => self.markup(start),
                _ => Err(error::Expr::Start(row, col)),
            },
            Some(b':') => {
                if self.peek_at(1).is_some_and(|b| b.is_ascii_lowercase()) {
                    self.tag(start)
                } else {
                    Err(error::Expr::Tag(
                        self.alloc(error::Tag::Name(row, col)),
                        row,
                        col,
                    ))
                }
            }
            Some(b'^') => Err(error::Expr::PinOutsideQuery(row, col)),
            Some(b'_') if !self.peek_at(1).is_some_and(is_ident_byte) => {
                Err(error::Expr::Placeholder(row, col))
            }
            Some(b) if b.is_ascii_lowercase() => self.lower_primary(start),
            Some(b) if b.is_ascii_uppercase() => self.name_or_path(start),
            _ => Err(error::Expr::Start(row, col)),
        }
    }

    /// At a lowercase letter: literal `true` / `false`, a keyword-led
    /// construct, a misplaced reserved or SQL word, or a name.
    fn lower_primary(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let (row, col) = self.position();
        let word = self.peek_word();
        match word {
            "true" | "false" => {
                self.advance_by(word.len());
                Ok(self.add_end(start, Expr::Bool(word == "true")))
            }
            "fn" => {
                self.advance_by(word.len());
                self.lambda(start)
            }
            "if" => {
                self.advance_by(word.len());
                self.if_(start)
            }
            "match" => {
                self.advance_by(word.len());
                self.match_(start)
            }
            "loop" => {
                self.advance_by(word.len());
                self.loop_(start)
            }
            "state" => {
                self.advance_by(word.len());
                self.state(start)
            }
            "style" => {
                self.advance_by(word.len());
                self.style(start)
            }
            "query" => {
                self.advance_by(word.len());
                self.query(start)
            }
            _ => {
                if let Some(kw) = Keyword::from_word(word) {
                    return Err(error::Expr::Reserved(kw, row, col));
                }
                if self.in_query()
                    && let Some(sql) = SqlWord::from_word(word)
                {
                    return Err(error::Expr::SqlKeyword(sql, row, col));
                }
                self.name_or_path(start)
            }
        }
    }

    /// An expression node whose region ends at `end` rather than at the
    /// cursor (for nodes built after a sub-parser chomped, or whose end is a
    /// child's end).
    pub(super) fn expr_at(
        &self,
        start: Position,
        end: Position,
        value: Expr<'a>,
    ) -> &'a Located<Expr<'a>> {
        self.alloc(Located::at(Region::new(start, end), value))
    }
}

/// May `op` sit at the start of a continuation line? (`-` only if followed by
/// whitespace; `<` only if not followed by a letter or `>`; everything else yes.)
fn continues_line(op: BinOp, next: Option<u8>) -> bool {
    match op {
        BinOp::Sub => next.is_some_and(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n')),
        BinOp::Lt => !next.is_some_and(|b| b.is_ascii_alphabetic() || b == b'>'),
        _ => true,
    }
}

/// Snapshot test macro for successful expression parsing.
#[cfg(test)]
macro_rules! assert_expression_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let result = parser
            .expression()
            .unwrap_or_else(|e| panic!("expected Ok, got Err: {e:#?}\n\nSource:\n{code}"));
        assert!(
            parser.is_eof(),
            "unconsumed input at {:?}\n\nSource:\n{code}",
            parser.position()
        );
        insta::with_settings!({
            description => code,
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }};
}

/// Snapshot test macro for expression parse errors.
#[cfg(test)]
macro_rules! assert_expression_error_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let err = parser
            .expression()
            .err()
            .unwrap_or_else(|| panic!("expected Err, got Ok\n\nSource:\n{code}"));
        insta::with_settings!({
            description => code,
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(err);
        });
    }};
}

#[cfg(test)]
pub(crate) use assert_expression_error_snapshot;
#[cfg(test)]
pub(crate) use assert_expression_snapshot;

#[cfg(test)]
mod tests {

    #[test]
    fn binop_add() {
        assert_expression_snapshot!("a + b");
    }

    #[test]
    fn binop_sub() {
        assert_expression_snapshot!("a - b");
    }

    #[test]
    fn binop_mul() {
        assert_expression_snapshot!("a * b");
    }

    #[test]
    fn binop_div() {
        assert_expression_snapshot!("a / b");
    }

    #[test]
    fn binop_rem() {
        assert_expression_snapshot!("a % b");
    }

    #[test]
    fn binop_eq() {
        assert_expression_snapshot!("a == b");
    }

    #[test]
    fn binop_neq() {
        assert_expression_snapshot!("a != b");
    }

    #[test]
    fn binop_lt() {
        assert_expression_snapshot!("a < b");
    }

    #[test]
    fn binop_lte() {
        assert_expression_snapshot!("a <= b");
    }

    #[test]
    fn binop_gt() {
        assert_expression_snapshot!("a > b");
    }

    #[test]
    fn binop_gte() {
        assert_expression_snapshot!("a >= b");
    }

    #[test]
    fn binop_and() {
        assert_expression_snapshot!("a && b");
    }

    #[test]
    fn binop_or() {
        assert_expression_snapshot!("a || b");
    }

    #[test]
    fn binop_coalesce() {
        assert_expression_snapshot!("a ?? b");
    }

    #[test]
    fn binop_pipe() {
        assert_expression_snapshot!("a |> f");
    }

    #[test]
    fn binop_chained() {
        assert_expression_snapshot!("a + b + c");
    }

    #[test]
    fn binop_mixed_flat() {
        assert_expression_snapshot!("a + b * c == d");
    }

    #[test]
    fn binop_with_parens() {
        assert_expression_snapshot!("(a + b) * c");
    }

    #[test]
    fn binop_eq_negative_no_space() {
        assert_expression_snapshot!("a==-1");
    }

    #[test]
    fn binop_lt_negative_no_space() {
        assert_expression_snapshot!("x<-1");
    }

    #[test]
    fn binop_leading_pipe_newline() {
        assert_expression_snapshot!(
            r#"
            xs
                |> f
                |> g
            "#
        );
    }

    #[test]
    fn binop_leading_plus_newline() {
        assert_expression_snapshot!(
            r#"
            a
                + b
            "#
        );
    }

    #[test]
    fn negate_var() {
        assert_expression_snapshot!("-x");
    }

    #[test]
    fn negate_number() {
        assert_expression_snapshot!("-1");
    }

    #[test]
    fn negate_call() {
        assert_expression_snapshot!("-f(x)");
    }

    #[test]
    fn not_var() {
        assert_expression_snapshot!("!ready");
    }

    #[test]
    fn not_parens() {
        assert_expression_snapshot!("!(a && b)");
    }

    #[test]
    fn double_negate() {
        assert_expression_snapshot!("--x");
    }

    #[test]
    fn error_operator_arrow() {
        assert_expression_error_snapshot!("a -> b");
    }

    #[test]
    fn error_operator_bar() {
        assert_expression_error_snapshot!("a | b");
    }

    #[test]
    fn error_operator_plus_plus() {
        assert_expression_error_snapshot!("a ++ b");
    }

    #[test]
    fn error_operator_double_colon() {
        assert_expression_error_snapshot!("a :: b");
    }

    #[test]
    fn error_operator_caret() {
        assert_expression_error_snapshot!("a ^ b");
    }

    #[test]
    fn error_operator_right_missing() {
        assert_expression_error_snapshot!("a +");
    }

    #[test]
    fn error_operator_right_bad_operand_propagates() {
        assert_expression_error_snapshot!(r#"a + "x"#);
    }

    #[test]
    fn error_unary_missing_operand() {
        assert_expression_error_snapshot!("-");
    }

    #[test]
    fn error_not_missing_operand() {
        assert_expression_error_snapshot!("!)");
    }

    #[test]
    fn error_unary_bad_operand_propagates() {
        assert_expression_error_snapshot!(r#"-"x"#);
    }

    #[test]
    fn error_pin_outside_query() {
        assert_expression_error_snapshot!("^x");
    }

    #[test]
    fn error_start_reserved() {
        assert_expression_error_snapshot!("else");
    }

    #[test]
    fn error_unexpected_close_tag() {
        assert_expression_error_snapshot!("</div>");
    }

    #[test]
    fn error_placeholder_alone() {
        assert_expression_error_snapshot!("_");
    }
}
