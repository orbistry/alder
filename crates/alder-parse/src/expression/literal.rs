//! Number and string literal primaries.
//!
//! Both are thin wrappers over the crate-root scanners (`number.rs`,
//! `string.rs`): a `Start` expectation never fires from `primary`, which
//! dispatches on the first byte, so the interesting errors are the scanners'
//! committed ones (`Number::End`, `StringError::Endless`, …) wrapped as
//! `Expr::Number` / `Expr::String` at the position the scanner reports.
//!
//! See docs/parser-internals.md §5.13.
// OWNER: expression/literal.rs (Wave 2)

use alder_region::{Located, Position};
use alder_source::Expr;

use crate::number::NumberLiteral;
use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// At a digit: `Expr::Number` or `Expr::BigInt`.
    pub(crate) fn number(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let literal = self.number_literal(error::Expr::Start, error::Expr::Number)?;
        let value = match literal {
            NumberLiteral::Number(lit) => Expr::Number(lit),
            NumberLiteral::BigInt(digits) => Expr::BigInt(digits),
        };
        Ok(self.add_end(start, value))
    }

    /// At `"`.
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
    use super::super::{assert_expression_error_snapshot, assert_expression_snapshot};

    #[test]
    fn number_int() {
        assert_expression_snapshot!("42");
    }

    #[test]
    fn number_float() {
        assert_expression_snapshot!("3.25");
    }

    #[test]
    fn number_hex() {
        assert_expression_snapshot!("0x1f");
    }

    #[test]
    fn number_exponent() {
        assert_expression_snapshot!("1e3");
    }

    #[test]
    fn number_keeps_text() {
        assert_expression_snapshot!("0xFF");
    }

    #[test]
    fn bigint() {
        assert_expression_snapshot!("123n");
    }

    #[test]
    fn bigint_hex() {
        assert_expression_snapshot!("0xFFn");
    }

    #[test]
    fn string_simple() {
        assert_expression_snapshot!(r#""hello""#);
    }

    #[test]
    fn string_empty() {
        assert_expression_snapshot!(r#""""#);
    }

    #[test]
    fn string_escapes() {
        assert_expression_snapshot!(r#""a\nb\t\"q\"""#);
    }

    #[test]
    fn string_unicode() {
        assert_expression_snapshot!(r#""\u{1F600}""#);
    }

    #[test]
    fn bool_true() {
        assert_expression_snapshot!("true");
    }

    #[test]
    fn bool_false() {
        assert_expression_snapshot!("false");
    }

    #[test]
    fn error_number_dirty_end() {
        assert_expression_error_snapshot!("123abc");
    }

    #[test]
    fn error_number_leading_zero() {
        assert_expression_error_snapshot!("007");
    }

    #[test]
    fn error_string_endless() {
        assert_expression_error_snapshot!(r#""abc"#);
    }

    #[test]
    fn error_string_newline() {
        assert_expression_error_snapshot!(
            r#"
            "abc
            def"
            "#
        );
    }

    #[test]
    fn error_string_bad_escape() {
        assert_expression_error_snapshot!(r#""\q""#);
    }
}
