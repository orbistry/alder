//! Record expression parsing for Alder.
//!
//! Ported from Elm's record parsing in `Parse/Expression.hs`.
//!
//! Handles:
//! - `{}` → empty record
//! - `{ x = 1, y = 2 }` → record literal
//! - `{ model | count = 5 }` → record update

use alder_region::{Located, Position};
use alder_source::{Expr, FieldAssign};
use bumpalo::collections::Vec as BumpVec;

use crate::Parser;
use crate::error::{self, Record};

impl<'a> Parser<'a> {
    /// Parse a record expression.
    ///
    /// Mirrors Elm's `record`:
    /// ```haskell
    /// record start =
    ///   inContext E.Record (word1 0x7B {- { -} E.Start) $
    ///     do  Space.chompAndCheckIndent E.RecordSpace E.RecordIndentOpen
    ///         oneOf E.RecordOpen
    ///           [ do  word1 0x7D {-}-} E.RecordOpen
    ///                 addEnd start (Src.Record [])
    ///           , do  starter <- addLocation (Var.lower E.RecordField)
    ///                 Space.chompAndCheckIndent E.RecordSpace E.RecordIndentEquals
    ///                 oneOf E.RecordEquals
    ///                   [ do  word1 0x7C {-|-} E.RecordEquals
    ///                         ...
    ///                   , do  word1 0x3D {-=-} E.RecordEquals
    ///                         ...
    ///                   ]
    ///           ]
    /// ```
    pub(crate) fn record(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        self.in_context(
            // Wrap Record errors with Expr::Record context
            |bump, record_err, row, col| error::Expr::Record(bump.alloc(record_err), row, col),
            // Start parser: parse '{'
            |p| p.word1(0x7B, error::Expr::Start),
            // Body parser: parse record contents
            |p| p.record_body(start),
        )
    }

    /// Parse the body of a record after the opening '{'.
    fn record_body(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, Record<'a>> {
        // Chomp whitespace and check indent
        self.chomp_and_check_indent(Record::Space, Record::IndentOpen)?;

        // Check what comes next
        self.one_of(
            Record::Open,
            vec![
                // Empty record: just '}'
                Box::new(|p: &mut Parser<'a>| {
                    p.word1(0x7D, Record::Open)?;
                    let empty: &'a [&'a FieldAssign<'a>] = &[];
                    Ok(p.add_end(start, Expr::Record(empty)))
                }),
                // Non-empty: field name first
                Box::new(|p: &mut Parser<'a>| p.record_starter(start)),
            ],
        )
    }

    /// Parse after the first lowercase identifier in a record.
    ///
    /// At this point we have `{ name` and need to determine:
    /// - `{ name | ... }` → record update
    /// - `{ name = ... }` → record literal
    fn record_starter(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, Record<'a>> {
        // Parse the starter field name
        let field_start = self.get_position();
        let starter = self.lower_name(Record::Field)?;
        let starter_located = self.add_end(field_start, starter);

        // Chomp whitespace and check indent
        self.chomp_and_check_indent(Record::Space, Record::IndentEquals)?;

        // Decide: '|' for update, '=' for literal
        self.one_of(
            Record::Equals,
            vec![
                // Record update: '|'
                Box::new(|p: &mut Parser<'a>| {
                    p.word1(0x7C, Record::Equals)?;
                    p.record_update(start, starter_located)
                }),
                // Record literal: '='
                Box::new(|p: &mut Parser<'a>| {
                    p.word1(0x3D, Record::Equals)?;
                    p.record_literal(start, starter_located)
                }),
            ],
        )
    }

    /// Parse a record update: `{ name | field = expr, ... }`.
    fn record_update(
        &mut self,
        start: Position,
        target: &'a Located<&'a str>,
    ) -> Result<&'a Located<Expr<'a>>, Record<'a>> {
        // Chomp whitespace and check indent after '|'
        self.chomp_and_check_indent(Record::Space, Record::IndentField)?;

        // Parse first field
        let first_field = self.chomp_field()?;
        let mut fields: BumpVec<'a, &'a FieldAssign<'a>> = BumpVec::new_in(self.bump);
        fields.push(first_field);

        // Parse remaining fields
        self.chomp_fields(&mut fields)?;

        let slice = fields.into_bump_slice();
        Ok(self.add_end(
            start,
            Expr::Update {
                record: target,
                fields: slice,
            },
        ))
    }

    /// Parse a record literal starting after the first '='.
    fn record_literal(
        &mut self,
        start: Position,
        first_name: &'a Located<&'a str>,
    ) -> Result<&'a Located<Expr<'a>>, Record<'a>> {
        // Chomp whitespace and check indent after '='
        self.chomp_and_check_indent(Record::Space, Record::IndentExpr)?;

        // Parse the first field's value
        let first_value = self.record_expr()?;

        // Check indent after expression
        let (end_row, end_col) = self.position();
        self.check_indent(end_row, end_col, Record::IndentEnd)?;

        // Build first field
        let first_field = self.alloc(FieldAssign {
            field: first_name,
            value: first_value,
        });

        let mut fields: BumpVec<'a, &'a FieldAssign<'a>> = BumpVec::new_in(self.bump);
        fields.push(first_field);

        // Parse remaining fields
        self.chomp_fields(&mut fields)?;

        let slice = fields.into_bump_slice();
        Ok(self.add_end(start, Expr::Record(slice)))
    }

    /// Parse a single field: `name = expr`.
    ///
    /// Mirrors Elm's `chompField`.
    fn chomp_field(&mut self) -> Result<&'a FieldAssign<'a>, Record<'a>> {
        // Parse field name
        let field_start = self.get_position();
        let name = self.lower_name(Record::Field)?;
        let field = self.add_end(field_start, name);

        // Chomp whitespace and check indent
        self.chomp_and_check_indent(Record::Space, Record::IndentEquals)?;

        // Parse '='
        self.word1(0x3D, Record::Equals)?;

        // Chomp whitespace and check indent after '='
        self.chomp_and_check_indent(Record::Space, Record::IndentExpr)?;

        // Parse the expression
        let value = self.record_expr()?;

        // Check indent after expression
        let (end_row, end_col) = self.position();
        self.check_indent(end_row, end_col, Record::IndentEnd)?;

        Ok(self.alloc(FieldAssign { field, value }))
    }

    /// Parse remaining fields after the first.
    ///
    /// Mirrors Elm's `chompFields`.
    fn chomp_fields(
        &mut self,
        fields: &mut BumpVec<'a, &'a FieldAssign<'a>>,
    ) -> Result<(), Record<'a>> {
        loop {
            // Chomp whitespace
            self.chomp(Record::Space)?;

            // Expect comma or closing brace
            let done = self.one_of(
                Record::End,
                vec![
                    // Comma - parse another field
                    Box::new(|p: &mut Parser<'a>| {
                        p.word1(0x2C, Record::End)?;

                        // Chomp whitespace and check indent after comma
                        p.chomp_and_check_indent(Record::Space, Record::IndentField)?;

                        // Parse the field
                        let field = p.chomp_field()?;
                        fields.push(field);

                        Ok(false) // Not done, continue loop
                    }),
                    // Closing brace - done
                    Box::new(|p: &mut Parser<'a>| {
                        p.word1(0x7D, Record::End)?;
                        Ok(true) // Done
                    }),
                ],
            )?;

            if done {
                return Ok(());
            }
        }
    }

    /// Parse a record field expression.
    ///
    /// Mirrors Elm's `specialize E.RecordExpr expression`.
    fn record_expr(&mut self) -> Result<&'a Located<Expr<'a>>, Record<'a>> {
        self.specialize(
            |bump, expr_err, row, col| Record::Expr(bump.alloc(expr_err), row, col),
            |p| p.term(),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::expression::{assert_expr_error_snapshot, assert_expr_snapshot};

    #[test]
    fn empty() {
        assert_expr_snapshot!("{}");
    }

    #[test]
    fn single_field() {
        assert_expr_snapshot!("{ x = 1 }");
    }

    #[test]
    fn two_fields() {
        assert_expr_snapshot!("{ x = 1, y = 2 }");
    }

    #[test]
    fn many_fields() {
        assert_expr_snapshot!("{ a = 1, b = 2, c = 3 }");
    }

    #[test]
    fn nested_record() {
        assert_expr_snapshot!("{ inner = { x = 1 } }");
    }

    #[test]
    fn multiline() {
        assert_expr_snapshot!(
            "{
                x = 1,
                y = 2,
                z = 3
            }"
        );
    }

    #[test]
    fn update_single() {
        assert_expr_snapshot!("{ model | count = 5 }");
    }

    #[test]
    fn update_multiple() {
        assert_expr_snapshot!("{ model | x = 1, y = 2 }");
    }

    #[test]
    fn update_multiline() {
        assert_expr_snapshot!(
            "{
                model
                | x = 1,
                  y = 2
            }"
        );
    }

    #[test]
    fn with_comments() {
        assert_expr_snapshot!(
            "{
                x = 1, -- first field
                y = 2  -- second field
            }"
        );
    }

    #[test]
    fn error_unclosed() {
        assert_expr_error_snapshot!("{ x = 1");
    }

    #[test]
    fn error_trailing_comma() {
        assert_expr_error_snapshot!("{ x = 1, }");
    }

    #[test]
    fn error_missing_equals() {
        assert_expr_error_snapshot!("{ x 1 }");
    }

    #[test]
    fn error_missing_value() {
        assert_expr_error_snapshot!("{ x = }");
    }

    #[test]
    fn error_uppercase_field() {
        assert_expr_error_snapshot!("{ X = 1 }");
    }
}
