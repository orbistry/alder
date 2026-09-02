//! Array literals: `[ expr { ',' expr } [','] ]`.
//!
//! Elements are full expressions parsed with record constructors re-enabled
//! (brackets reset the `no_record_ctor` restriction, §2.3). A trailing
//! comma is accepted (§10.8); a missing separator is `Array::End` at the
//! offending byte and a missing element (`[1,,2]`) is `Array::Expr(Start)`
//! at the stray comma.
//!
//! See docs/parser-internals.md §5.13.
// OWNER: expression/array.rs (Wave 2)

use alder_region::{Located, Position};
use alder_source::Expr;
use bumpalo::collections::Vec as BumpVec;

use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// At `[`.
    pub(crate) fn array(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let elements = self.specialize(
            |bump, e, row, col| error::Expr::Array(bump.alloc(e), row, col),
            |p| p.with_record_ctor(true, |p| p.array_elements()),
        )?;
        Ok(self.add_end(start, Expr::Array(elements)))
    }

    /// At `[`: elements through the closing `]`, which is consumed.
    fn array_elements(&mut self) -> Result<&'a [&'a Located<Expr<'a>>], error::Array<'a>> {
        self.advance();
        self.chomp();
        let mut elements = BumpVec::new_in(self.bump);
        loop {
            if self.peek() == Some(b']') {
                self.advance();
                break;
            }
            let element = self.specialize(
                |bump, e, row, col| error::Array::Expr(bump.alloc(e), row, col),
                |p| p.expression(),
            )?;
            elements.push(element);
            match self.peek() {
                Some(b',') => {
                    self.advance();
                    self.chomp();
                }
                Some(b']') => {
                    self.advance();
                    break;
                }
                _ => {
                    let (row, col) = self.position();
                    return Err(error::Array::End(row, col));
                }
            }
        }
        Ok(elements.into_bump_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{assert_expression_error_snapshot, assert_expression_snapshot};

    #[test]
    fn empty() {
        assert_expression_snapshot!("[]");
    }

    #[test]
    fn single() {
        assert_expression_snapshot!("[1]");
    }

    #[test]
    fn multiple() {
        assert_expression_snapshot!("[1, 2, 3]");
    }

    #[test]
    fn nested() {
        assert_expression_snapshot!("[[1, 2], [3]]");
    }

    #[test]
    fn trailing_comma() {
        assert_expression_snapshot!("[1, 2,]");
    }

    #[test]
    fn multiline() {
        assert_expression_snapshot!(
            r#"
            [
                1,
                2,
            ]
            "#
        );
    }

    #[test]
    fn with_comments() {
        assert_expression_snapshot!(
            r#"
            [
                // first
                1, // one
                2,
            ]
            "#
        );
    }

    #[test]
    fn error_unclosed() {
        assert_expression_error_snapshot!("[1, 2");
    }

    #[test]
    fn error_double_comma() {
        assert_expression_error_snapshot!("[1,,2]");
    }

    #[test]
    fn error_missing_comma() {
        assert_expression_error_snapshot!("[1 2]");
    }
}
