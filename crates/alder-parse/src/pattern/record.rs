//! Record pattern parsing for Alder.
//!
//! Ported from Elm's `Pattern.record`.
//!
//! Handles: `{}`, `{ x }`, `{ x, y, z }`

use alder_region::{Located, Position};
use alder_source::Pattern;
use bumpalo::collections::Vec as BumpVec;

use crate::Parser;
use crate::error::{self, PRecord};

impl<'a> Parser<'a> {
    /// Parse a record pattern: `{ x, y, z }`
    pub(super) fn pattern_record(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        self.in_context(
            |bump, record_err, row, col| error::Pattern::Record(bump.alloc(record_err), row, col),
            |p| p.word1(0x7B, error::Pattern::Start),
            |p| p.pattern_record_body(start),
        )
    }

    /// Parse record pattern body after `{`.
    fn pattern_record_body(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, PRecord> {
        self.chomp_and_check_indent(PRecord::Space, PRecord::IndentOpen)?;

        self.one_of(
            PRecord::Open,
            vec![
                // Non-empty: first field
                Box::new(|p: &mut Parser<'a>| {
                    let field_start = p.get_position();
                    let name = p.lower_name(PRecord::Field)?;
                    let field = p.add_end(field_start, name);

                    // Check indent after field name (don't chomp - that happens in the helper)
                    let (end_row, end_col) = p.position();
                    p.check_indent(end_row, end_col, PRecord::IndentEnd)?;

                    let mut fields: BumpVec<'a, &'a Located<&'a str>> = BumpVec::new_in(p.bump);
                    fields.push(field);
                    p.pattern_record_help(&mut fields)?;

                    let slice = fields.into_bump_slice();
                    Ok(p.add_end(start, Pattern::Record(slice)))
                }),
                // Empty record: `{}`
                Box::new(|p: &mut Parser<'a>| {
                    p.word1(0x7D, PRecord::Open)?;
                    let empty: &'a [&'a Located<&'a str>] = &[];
                    Ok(p.add_end(start, Pattern::Record(empty)))
                }),
            ],
        )
    }

    /// Parse remaining record pattern fields.
    fn pattern_record_help(
        &mut self,
        fields: &mut BumpVec<'a, &'a Located<&'a str>>,
    ) -> Result<(), PRecord> {
        loop {
            // Chomp whitespace before checking for comma or brace
            self.chomp(PRecord::Space)?;

            let done = self.one_of(
                PRecord::End,
                vec![
                    // Comma - another field
                    Box::new(|p: &mut Parser<'a>| {
                        p.word1(0x2C, PRecord::End)?;
                        p.chomp_and_check_indent(PRecord::Space, PRecord::IndentField)?;

                        let field_start = p.get_position();
                        let name = p.lower_name(PRecord::Field)?;
                        let field = p.add_end(field_start, name);
                        fields.push(field);

                        // Check indent after field (don't chomp - that happens next iteration)
                        let (end_row, end_col) = p.position();
                        p.check_indent(end_row, end_col, PRecord::IndentEnd)?;
                        Ok(false)
                    }),
                    // Close brace
                    Box::new(|p: &mut Parser<'a>| {
                        p.word1(0x7D, PRecord::End)?;
                        Ok(true)
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
    use super::super::{assert_pattern_error_snapshot, assert_pattern_snapshot};

    #[test]
    fn empty() {
        assert_pattern_snapshot!("{}");
    }

    #[test]
    fn single() {
        assert_pattern_snapshot!("{ x }");
    }

    #[test]
    fn multiple() {
        assert_pattern_snapshot!("{ x, y, z }");
    }

    #[test]
    fn multiline() {
        assert_pattern_snapshot!(
            "{
                x,
                y,
                z
            }"
        );
    }

    #[test]
    fn error_unclosed() {
        assert_pattern_error_snapshot!("{ x, y");
    }

    #[test]
    fn error_trailing_comma() {
        assert_pattern_error_snapshot!("{ x, y, }");
    }
}
