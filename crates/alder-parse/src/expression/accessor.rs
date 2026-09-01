//! Accessor and field access parsing for Alder.
//!
//! Ported from Elm's `Parse/Expression.hs` (accessor, accessible).

use alder_region::{Located, Position, Region};
use alder_source::Expr;

use crate::Parser;
use crate::error;

impl<'a> Parser<'a> {
    /// Parse field accessor `.fieldName`.
    ///
    /// Mirrors Elm's `accessor`:
    /// ```haskell
    /// accessor start =
    ///   do  word1 0x2E {-.-} E.Dot
    ///       field <- Var.lower E.Access
    ///       addEnd start (Src.Accessor field)
    /// ```
    pub(crate) fn accessor(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        self.word1(b'.', error::Expr::Dot)?;
        let field = self.lower_name(error::Expr::Access)?;
        Ok(self.add_end(start, Expr::Accessor(field)))
    }

    /// Handle field access chains like `foo.bar.baz`.
    ///
    /// Mirrors Elm's `accessible`:
    /// ```haskell
    /// accessible start expr =
    ///   oneOfWithFallback
    ///     [ do  word1 0x2E {-.-} E.Dot
    ///           pos <- getPosition
    ///           field <- Var.lower E.Access
    ///           end <- getPosition
    ///           accessible start $
    ///             A.at start end (Src.Access expr (A.at pos end field))
    ///     ]
    ///     expr
    /// ```
    pub(crate) fn accessible(
        &mut self,
        start: Position,
        expr: &'a Located<Expr<'a>>,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        self.one_of_with_fallback(
            vec![Box::new(|p: &mut Parser<'a>| {
                p.word1(b'.', error::Expr::Dot)?;
                let pos = p.get_position();
                let field = p.lower_name(error::Expr::Access)?;
                let end = p.get_position();

                let located_field = p.alloc(Located::at(Region::new(pos, end), field));
                let access_expr = p.alloc(Located::at(
                    Region::new(start, end),
                    Expr::Access {
                        record: expr,
                        field: located_field,
                    },
                ));

                p.accessible(start, access_expr)
            })],
            expr,
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::expression::assert_expr_snapshot;

    #[test]
    fn accessor_simple() {
        assert_expr_snapshot!(".name");
    }

    #[test]
    fn accessor_long() {
        assert_expr_snapshot!(".firstName");
    }

    #[test]
    fn field_access_simple() {
        assert_expr_snapshot!("foo.bar");
    }

    #[test]
    fn field_access_chain() {
        assert_expr_snapshot!("foo.bar.baz");
    }

    #[test]
    fn record_field_access() {
        assert_expr_snapshot!("{ x = 1 }.x");
    }
}
