//! `let [mut] pattern [: Type] = expr` — shared by items, statements and child blocks.
//!
//! Grammar (SPEC.md): `let_decl = 'let' [ 'mut' ] pattern [ ':' type ] '=' expression ;`
//! (`let card = style { … }` is the same production with a `style` value).
//!
//! `let_decl` runs after the `let` keyword; the `=` must be a bare `=` (`==`
//! and `=>` are `Let::Equals`). The value is parsed by `expression()`, which
//! chomps trailing whitespace, so the caller finds the cursor past it.
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/let_.rs (Wave 2)

use alder_source::LetDecl;

use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// After `let`.
    pub(crate) fn let_decl(&mut self) -> Result<&'a LetDecl<'a>, error::Let<'a>> {
        self.chomp();
        let mutable = if self.peek_keyword(b"mut") {
            let start = self.get_position();
            self.advance_by(3);
            let region = self.located(start, ()).region;
            self.chomp();
            Some(region)
        } else {
            None
        };
        let pattern = self.specialize(
            |bump, e, row, col| error::Let::Pattern(bump.alloc(e), row, col),
            |p| p.pattern(),
        )?;
        let annotation = if self.peek() == Some(b':') {
            self.advance();
            self.chomp();
            Some(self.specialize(
                |bump, e, row, col| error::Let::Type(bump.alloc(e), row, col),
                |p| p.type_expr(),
            )?)
        } else {
            None
        };
        let (row, col) = self.position();
        if self.peek() != Some(b'=') || matches!(self.peek_at(1), Some(b'=' | b'>')) {
            return Err(error::Let::Equals(row, col));
        }
        self.advance();
        self.chomp();
        let value = self.specialize(
            |bump, e, row, col| error::Let::Body(bump.alloc(e), row, col),
            |p| p.expression(),
        )?;
        Ok(self.alloc(LetDecl {
            mutable,
            pattern,
            annotation,
            value,
        }))
    }
}

#[cfg(test)]
mod tests {
    use crate::item::{assert_item_error_snapshot, assert_item_snapshot};

    #[test]
    fn let_top() {
        assert_item_snapshot!("let x = 1");
    }

    #[test]
    fn let_top_pub() {
        assert_item_snapshot!("pub let x = 1");
    }

    #[test]
    fn let_top_mut_state() {
        assert_item_snapshot!("let mut count = state(0)");
    }

    #[test]
    fn let_top_annotated() {
        assert_item_snapshot!("let x: Number = 1");
    }

    #[test]
    fn let_style() {
        assert_item_snapshot!("let card = style { padding: 16px }");
    }

    /// The item ends at the `)` (§10.43), as the statement form does.
    #[test]
    fn let_top_parens() {
        assert_item_snapshot!("let x = (a)");
    }

    #[test]
    fn error_let_type() {
        assert_item_error_snapshot!("let x: = 1");
    }

    #[test]
    fn error_let_body() {
        assert_item_error_snapshot!(r#"let x = "oops"#);
    }
}
