//! String expression parsing for Alder.

use alder_region::{Located, Position};
use alder_source::Expr;

use crate::Parser;
use crate::error;

impl<'a> Parser<'a> {
    /// Parse a string expression.
    ///
    /// Mirrors Elm's `string` helper:
    /// ```haskell
    /// string start =
    ///   do  str <- String.string E.Start E.String
    ///       addEnd start (Src.Str str)
    /// ```
    pub(crate) fn string(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let s = self.string_literal(error::Expr::Start, error::Expr::String)?;
        Ok(self.add_end(start, Expr::Str(s)))
    }
}

#[cfg(test)]
mod tests {
    use crate::expression::{assert_expr_error_snapshot, assert_expr_snapshot};

    #[test]
    fn simple() {
        assert_expr_snapshot!(r#""hello""#);
    }

    #[test]
    fn with_escape() {
        assert_expr_snapshot!(r#""hello\nworld""#);
    }

    #[test]
    fn multi_line() {
        assert_expr_snapshot!(r#""""multi-line""""#);
    }

    #[test]
    fn empty() {
        assert_expr_snapshot!(r#""""#);
    }

    #[test]
    fn unicode() {
        assert_expr_snapshot!(r#""\u{1F600}""#);
    }

    #[test]
    fn error_endless() {
        assert_expr_error_snapshot!(r#""hello"#);
    }
}
