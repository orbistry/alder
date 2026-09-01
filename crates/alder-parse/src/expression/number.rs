//! Number expression parsing for Alder.

use alder_region::{Located, Position};
use alder_source::Expr;

use crate::Parser;
use crate::error;

impl<'a> Parser<'a> {
    /// Parse a number expression.
    ///
    /// Mirrors Elm's `number` helper:
    /// ```haskell
    /// number start =
    ///   do  nmbr <- Number.number E.Start E.Number
    ///       addEnd start $
    ///         case nmbr of
    ///           Number.Int int -> Src.Int int
    ///           Number.Float float -> Src.Float float
    /// ```
    pub(crate) fn number(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let n = self.number_literal(error::Expr::Start, error::Expr::Number)?;
        Ok(self.add_end(start, Expr::Int(n)))
    }
}

#[cfg(test)]
mod tests {
    use crate::expression::{assert_expr_error_snapshot, assert_expr_snapshot};

    #[test]
    fn int_simple() {
        assert_expr_snapshot!("42");
    }

    #[test]
    fn int_zero() {
        assert_expr_snapshot!("0");
    }

    #[test]
    fn int_hex() {
        assert_expr_snapshot!("0xFF");
    }

    #[test]
    fn int_large() {
        assert_expr_snapshot!("123456789");
    }

    #[test]
    fn error_leading_zero() {
        assert_expr_error_snapshot!("007");
    }

    #[test]
    fn error_hex_no_digits() {
        assert_expr_error_snapshot!("0x");
    }
}
