//! Postfix operators: calls, indexing, `.field` / `.0` / `.await`, tagged templates.
//!
//! The loop that applies them (and the `?` / record-constructor cases) lives
//! in `expression/mod.rs::postfix`; this file holds the individual operator
//! parsers. None of them chomps after itself. Each node spans from the
//! target's start to the cursor, so the region of `f(x)[0].y` grows step by
//! step and the innermost node is the leftmost.
//!
//! - `call_args` reads a whole `_` argument as `Expr::Placeholder` (§10.18);
//!   a `_` that is not followed by `,` or `)` goes through `expression` and
//!   fails there as `Expr::Placeholder`, so `f(_ + 1)` names the `_`.
//! - `dot_suffix` requires the member to be adjacent to the `.`; `.await`
//!   is matched as a keyword (`await` is reserved, §10.6), digits are a
//!   bare run via `digits()` (§10.10), anything else is `lower_name`, so a
//!   reserved word is `Expr::Access` after the `.`.
//! - `tagged_template` reuses `template_parts` (§5.7); the caller has
//!   already checked adjacency (§2.1 rule 5).
//!
//! Call, index and tagged-template errors carry the position of their
//! opener (`(`, `[`, the backtick), like Elm's bracket constructs.
//!
//! See docs/parser-internals.md §5.13 and §6.0.
// OWNER: expression/postfix.rs (Wave 2)

use alder_region::Located;
use alder_source::Expr;
use bumpalo::collections::Vec as BumpVec;

use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// At `(`; accepts `_` placeholders as whole arguments. Consumes the `)`.
    pub(crate) fn call_args(&mut self) -> Result<&'a [&'a Located<Expr<'a>>], error::Call<'a>> {
        self.advance();
        self.chomp();
        self.with_record_ctor(true, |p| p.call_args_body())
    }

    fn call_args_body(&mut self) -> Result<&'a [&'a Located<Expr<'a>>], error::Call<'a>> {
        let mut args = BumpVec::new_in(self.bump);
        loop {
            if self.peek() == Some(b')') {
                self.advance();
                break;
            }
            let arg = match self.placeholder_arg() {
                Some(placeholder) => placeholder,
                None => self.specialize(
                    |bump, e, row, col| error::Call::Arg(bump.alloc(e), row, col),
                    |p| p.expression(),
                )?,
            };
            args.push(arg);
            match self.peek() {
                Some(b',') => {
                    self.advance();
                    self.chomp();
                }
                Some(b')') => {
                    self.advance();
                    break;
                }
                _ => {
                    let (row, col) = self.position();
                    return Err(error::Call::End(row, col));
                }
            }
        }
        Ok(args.into_bump_slice())
    }

    /// A `_` that is a whole argument: followed, after whitespace, by `,`
    /// or `)`. Consumes it and the whitespace; otherwise consumes nothing.
    fn placeholder_arg(&mut self) -> Option<&'a Located<Expr<'a>>> {
        if self.peek() != Some(b'_') {
            return None;
        }
        let saved = self.save_state();
        let start = self.get_position();
        self.advance();
        let end = self.get_position();
        self.chomp();
        if matches!(self.peek(), Some(b',' | b')')) {
            Some(self.expr_at(start, end, Expr::Placeholder))
        } else {
            self.restore_state(saved);
            None
        }
    }

    /// At `[`. Consumes the `]`.
    pub(crate) fn index(
        &mut self,
        target: &'a Located<Expr<'a>>,
    ) -> Result<&'a Located<Expr<'a>>, error::Index<'a>> {
        self.advance();
        self.chomp();
        let index = self.specialize(
            |bump, e, row, col| error::Index::Expr(bump.alloc(e), row, col),
            |p| p.with_record_ctor(true, |p| p.expression()),
        )?;
        self.word1(b']', error::Index::End)?;
        Ok(self.expr_at(
            target.region.start,
            self.get_position(),
            Expr::Index { target, index },
        ))
    }

    /// At `.`: field / digits / await.
    pub(crate) fn dot_suffix(
        &mut self,
        target: &'a Located<Expr<'a>>,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let start = target.region.start;
        self.advance();
        if self.peek_keyword(b"await") {
            self.advance_by(5);
            return Ok(self.expr_at(start, self.get_position(), Expr::Await(target)));
        }
        if let Some(index) = self.digits() {
            return Ok(self.expr_at(
                start,
                self.get_position(),
                Expr::TupleAccess {
                    tuple: target,
                    index,
                },
            ));
        }
        let field = self.located_lower(error::Expr::Access)?;
        Ok(self.expr_at(
            start,
            self.get_position(),
            Expr::Access {
                record: target,
                field,
            },
        ))
    }

    /// At an adjacent backtick.
    pub(crate) fn tagged_template(
        &mut self,
        tag: &'a Located<Expr<'a>>,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let parts = self.specialize(
            |bump, e, row, col| error::Expr::TaggedTemplate(bump.alloc(e), row, col),
            |p| p.template_parts(),
        )?;
        Ok(self.expr_at(
            tag.region.start,
            self.get_position(),
            Expr::TaggedTemplate { tag, parts },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{assert_expression_error_snapshot, assert_expression_snapshot};

    #[test]
    fn call_no_args() {
        assert_expression_snapshot!("f()");
    }

    #[test]
    fn call_one_arg() {
        assert_expression_snapshot!("f(x)");
    }

    #[test]
    fn call_many_args() {
        assert_expression_snapshot!("add(1, 2, 3)");
    }

    #[test]
    fn call_trailing_comma() {
        assert_expression_snapshot!("f(a, b,)");
    }

    #[test]
    fn call_nested() {
        assert_expression_snapshot!("f(g(x))");
    }

    #[test]
    fn call_chained() {
        assert_expression_snapshot!("f(1)(2)");
    }

    #[test]
    #[ignore = "waits for expression/lambda.rs"]
    fn call_on_lambda_result() {
        assert_expression_snapshot!("(fn(x) x + 1)(2)");
    }

    #[test]
    fn call_placeholder_first() {
        assert_expression_snapshot!("Array.map(_, double)");
    }

    #[test]
    fn call_placeholder_second() {
        assert_expression_snapshot!("add(1, _)");
    }

    #[test]
    fn call_placeholder_all() {
        assert_expression_snapshot!("f(_, _)");
    }

    #[test]
    fn access_field() {
        assert_expression_snapshot!("user.name");
    }

    #[test]
    fn access_chain() {
        assert_expression_snapshot!("a.b.c");
    }

    #[test]
    fn access_newline_continuation() {
        assert_expression_snapshot!(
            r#"
            builder
                .first()
                .second
            "#
        );
    }

    #[test]
    fn tuple_index() {
        assert_expression_snapshot!("t.0");
    }

    #[test]
    fn tuple_index_chain() {
        assert_expression_snapshot!("t.0.1");
    }

    #[test]
    fn index_simple() {
        assert_expression_snapshot!("xs[0]");
    }

    #[test]
    fn index_nested() {
        assert_expression_snapshot!("grid[i][j]");
    }

    #[test]
    fn index_expr() {
        assert_expression_snapshot!("xs[i + 1]");
    }

    #[test]
    fn await_simple() {
        assert_expression_snapshot!("task.await");
    }

    #[test]
    fn await_chain() {
        assert_expression_snapshot!("fetch(url).await.json().await");
    }

    #[test]
    fn await_then_try() {
        assert_expression_snapshot!("load(id).await?");
    }

    #[test]
    fn try_after_await() {
        assert_expression_snapshot!("find(id)?.await");
    }

    #[test]
    fn try_simple() {
        assert_expression_snapshot!("x?");
    }

    #[test]
    fn try_then_coalesce() {
        assert_expression_snapshot!("x? ?? y");
    }

    #[test]
    fn coalesce_not_try() {
        assert_expression_snapshot!("a ?? b");
    }

    #[test]
    #[ignore = "waits for template.rs"]
    fn tagged_template_adjacent() {
        assert_expression_snapshot!("sql`select ${x}`");
    }

    #[test]
    #[ignore = "waits for template.rs"]
    fn tagged_template_after_access() {
        assert_expression_snapshot!("Db.sql`select 1`");
    }

    #[test]
    fn macro_call_simple() {
        assert_expression_snapshot!("assert_eq!(a, b)");
    }

    #[test]
    fn macro_call_nested_parens() {
        assert_expression_snapshot!("f!((a)(b))");
    }

    #[test]
    fn macro_call_with_string() {
        assert_expression_snapshot!(r#"f!(")")"#);
    }

    /// A backtick after whitespace is not a tagged template: the expression
    /// ends at the name and the backtick is left for the statement layer,
    /// which reports it as a second statement on the same line.
    #[test]
    fn error_tagged_template_with_space() {
        let bump = bumpalo::Bump::new();
        let code = "tag `x`";
        let src = bump.alloc_str(code);
        let mut parser = crate::Parser::new(&bump, src.as_bytes());
        let result = parser
            .expression()
            .unwrap_or_else(|e| panic!("expected Ok, got Err: {e:#?}\n\nSource:\n{code}"));
        assert!(!parser.is_eof(), "the backtick must be left unconsumed");
        assert_eq!(parser.position(), (1, 5));
        insta::with_settings!({
            description => code,
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn error_placeholder_in_binop() {
        assert_expression_error_snapshot!("f(_ + 1)");
    }

    #[test]
    fn error_call_unclosed() {
        assert_expression_error_snapshot!("f(a, b");
    }

    #[test]
    fn error_index_unclosed() {
        assert_expression_error_snapshot!("xs[0");
    }

    #[test]
    fn error_access_no_name() {
        assert_expression_error_snapshot!("x.");
    }

    #[test]
    fn error_macro_unbalanced() {
        assert_expression_error_snapshot!("f!(a]");
    }
}
