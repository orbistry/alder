//! List expression parsing for Alder.
//!
//! Ported from Elm's list parsing in `Parse/Expression.hs`.

use alder_region::{Located, Position};
use alder_source::Expr;
use bumpalo::collections::Vec as BumpVec;

use crate::Parser;
use crate::error::{self, List};

impl<'a> Parser<'a> {
    /// Parse a list expression.
    ///
    /// Mirrors Elm's `list`:
    /// ```haskell
    /// list start =
    ///   inContext E.List (word1 0x5B {-[-} E.Start) $
    ///     do  Space.chompAndCheckIndent E.ListSpace E.ListIndentOpen
    ///         oneOf E.ListOpen
    ///           [ do  (entry, end) <- specialize E.ListExpr expression
    ///                 Space.checkIndent end E.ListIndentEnd
    ///                 chompListEnd start [entry]
    ///           , do  word1 0x5D {-]-} E.ListOpen
    ///                 addEnd start (Src.List [])
    ///           ]
    /// ```
    pub(crate) fn list(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        self.in_context(
            // Wrap List errors with Expr::List context
            |bump, list_err, row, col| error::Expr::List(bump.alloc(list_err), row, col),
            // Start parser: parse '['
            |p| p.word1(0x5B, error::Expr::Start),
            // Body parser: parse list contents
            |p| p.list_body(start),
        )
    }

    /// Parse the body of a list after the opening '['.
    ///
    /// Returns `List` errors which get wrapped by `in_context`.
    fn list_body(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, List<'a>> {
        // Chomp whitespace and check indent
        self.chomp_and_check_indent(List::Space, List::IndentOpen)?;

        // Check for empty list or first element
        self.one_of(
            List::Open,
            vec![
                // Try to parse first element
                Box::new(|p: &mut Parser<'a>| {
                    let (first, end) = p.list_expr()?;

                    // Check indent using expression's end position (not current parser position)
                    p.check_indent(end.line, end.column, List::IndentEnd)?;

                    // Parse remaining elements
                    let mut elements = BumpVec::new_in(p.bump);
                    elements.push(first);
                    p.chomp_list_end(&mut elements)?;

                    let slice = elements.into_bump_slice();
                    Ok(p.add_end(start, Expr::List(slice)))
                }),
                // Empty list: just ']'
                Box::new(|p: &mut Parser<'a>| {
                    p.word1(0x5D, List::Open)?;
                    let slice: &'a [&'a Located<Expr<'a>>] = &[];
                    Ok(p.add_end(start, Expr::List(slice)))
                }),
            ],
        )
    }

    /// Parse a list entry expression.
    ///
    /// Mirrors Elm's `specialize E.ListExpr expression`.
    /// Returns both the expression and its end position for indent checking.
    fn list_expr(&mut self) -> Result<(&'a Located<Expr<'a>>, Position), List<'a>> {
        self.specialize(
            |bump, expr_err, row, col| List::Expr(bump.alloc(expr_err), row, col),
            |p| p.expression(),
        )
    }

    /// Parse the rest of a list after the first element.
    ///
    /// Mirrors Elm's `chompListEnd`:
    /// ```haskell
    /// chompListEnd start entries =
    ///   oneOf E.ListEnd
    ///     [ do  word1 0x2C {-,-} E.ListEnd
    ///           Space.chompAndCheckIndent E.ListSpace E.ListIndentExpr
    ///           (entry, end) <- specialize E.ListExpr expression
    ///           Space.checkIndent end E.ListIndentEnd
    ///           chompListEnd start (entry:entries)
    ///     , do  word1 0x5D {-]-} E.ListEnd
    ///           addEnd start (Src.List (reverse entries))
    ///     ]
    /// ```
    fn chomp_list_end(
        &mut self,
        elements: &mut BumpVec<'a, &'a Located<Expr<'a>>>,
    ) -> Result<(), List<'a>> {
        loop {
            // Chomp whitespace between elements
            self.chomp(List::Space)?;

            // Expect comma or closing bracket
            let done = self.one_of(
                List::End,
                vec![
                    // Comma - parse another element
                    Box::new(|p: &mut Parser<'a>| {
                        p.word1(0x2C, List::End)?;

                        // Chomp whitespace and check indent after comma
                        p.chomp_and_check_indent(List::Space, List::IndentExpr)?;

                        // Parse the expression
                        let (elem, end) = p.list_expr()?;
                        elements.push(elem);

                        // Check indent using expression's end position
                        p.check_indent(end.line, end.column, List::IndentEnd)?;

                        Ok(false) // Not done, continue loop
                    }),
                    // Closing bracket - done
                    Box::new(|p: &mut Parser<'a>| {
                        p.word1(0x5D, List::End)?;
                        Ok(true) // Done
                    }),
                ],
            )?;

            if done {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::expression::{
        assert_expr_error_snapshot, assert_expr_snapshot, assert_indented_expr_snapshot,
    };

    #[test]
    fn empty() {
        assert_expr_snapshot!("[]");
    }

    #[test]
    fn single() {
        assert_expr_snapshot!("[1]");
    }

    #[test]
    fn multiple() {
        assert_expr_snapshot!("[1, 2, 3]");
    }

    #[test]
    fn nested() {
        assert_expr_snapshot!("[[1], [2, 3]]");
    }

    #[test]
    fn with_whitespace() {
        assert_expr_snapshot!("[ 1 , 2 , 3 ]");
    }

    #[test]
    fn multiline() {
        assert_indented_expr_snapshot!(
            "[
                1,
                2,
                3
            ]"
        );
    }

    #[test]
    fn with_comments() {
        assert_indented_expr_snapshot!(
            "[
                1, -- first
                2, -- second
                3  -- third
            ]"
        );
    }

    #[test]
    fn mixed_types() {
        assert_expr_snapshot!(r#"[foo, "bar", 42]"#);
    }

    #[test]
    fn error_unclosed() {
        assert_expr_error_snapshot!("[1, 2");
    }

    #[test]
    fn error_trailing_comma() {
        assert_expr_error_snapshot!("[1, 2,]");
    }

    #[test]
    fn error_tab() {
        assert_expr_error_snapshot!("[1,\t2]");
    }
}
