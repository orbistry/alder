//! `loop { }`, `state(expr)` and `name!( … )` macro calls.
//!
//! Keyword-led constructs report their errors at the keyword (Elm's
//! `in_context` convention): `Expr::Loop` and `Expr::State` carry the
//! position of `loop` / `state`. `Expr::MacroCall` carries the position
//! `raw_balanced` reports for each problem (§5.9): the `(` for `Endless`,
//! the offending byte for `Unbalanced`, the string's own position for
//! `String`.
//!
//! See docs/parser-internals.md §5.13 and §6.5.
// OWNER: expression/loop_.rs (Wave 2)

use alder_region::{Located, Position};
use alder_source::{Expr, Name};

use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// After `loop`.
    pub(crate) fn loop_(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        self.chomp();
        let body = self.specialize(
            |bump, e, _, _| error::Expr::Loop(bump.alloc(e), start.line, start.column),
            |p| p.block(),
        )?;
        // `block()` chomps trailing whitespace; the loop ends where its body does.
        Ok(self.expr_at(start, body.region.end, Expr::Loop(body)))
    }

    /// After `state`.
    pub(crate) fn state(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let initial = self.specialize(
            |bump, e, _, _| error::Expr::State(bump.alloc(e), start.line, start.column),
            |p| p.state_body(),
        )?;
        Ok(self.add_end(start, Expr::State(initial)))
    }

    /// `( expression )`, whitespace allowed before the `(`. Consumes the `)`.
    fn state_body(&mut self) -> Result<&'a Located<Expr<'a>>, error::State<'a>> {
        self.chomp();
        self.word1(b'(', error::State::Open)?;
        self.chomp();
        let initial = self.specialize(
            |bump, e, row, col| error::State::Expr(bump.alloc(e), row, col),
            |p| p.with_record_ctor(true, |p| p.expression()),
        )?;
        self.word1(b')', error::State::End)?;
        Ok(initial)
    }

    /// At `!(` immediately after a lowercase name.
    pub(crate) fn macro_call(
        &mut self,
        start: Position,
        name: Name<'a>,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        self.advance();
        let tokens = self.raw_balanced(b'(', b')', error::Expr::MacroCall)?;
        Ok(self.add_end(start, Expr::MacroCall { name, tokens }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{assert_expression_error_snapshot, assert_expression_snapshot};

    #[test]
    #[ignore = "waits for statement.rs"]
    fn loop_simple() {
        assert_expression_snapshot!(
            r#"
            loop {
                step()
            }
            "#
        );
    }

    #[test]
    #[ignore = "waits for statement.rs"]
    fn loop_break_value() {
        assert_expression_snapshot!(
            r#"
            loop {
                let next = iter.next()
                if matches(next) { break next }
            }
            "#
        );
    }

    #[test]
    #[ignore = "waits for statement.rs"]
    fn loop_nested() {
        assert_expression_snapshot!(
            r#"
            loop {
                loop {
                    break
                }
            }
            "#
        );
    }

    #[test]
    fn state_simple() {
        assert_expression_snapshot!("state(0)");
    }

    #[test]
    fn state_expr() {
        assert_expression_snapshot!("state(props.start ?? 0)");
    }

    #[test]
    #[ignore = "waits for statement.rs"]
    fn error_loop_missing_block() {
        assert_expression_error_snapshot!("loop x");
    }

    #[test]
    fn error_state_no_parens() {
        assert_expression_error_snapshot!("state 0");
    }
}
