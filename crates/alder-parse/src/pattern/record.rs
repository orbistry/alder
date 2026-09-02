//! Record patterns `{ a, b: p, .. }` — shared with `CtorRecord`.
//!
//! `{}`, `{ x }`, `{ x, y: p }`, `{ x, .. }`. Each field is a shorthand
//! binding or `name: pattern`; `..` must be last (a trailing comma after
//! it is fine). Trailing commas are accepted (§10.8).
//!
//! See docs/parser-internals.md §5.14.
// OWNER: pattern/record.rs (Wave 1)

use alder_region::Region;
use alder_source::FieldPattern;
use bumpalo::collections::Vec as BumpVec;

use crate::error::PRecord;
use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// After `{`. Consumes the closing `}` and nothing after it.
    pub(super) fn pattern_record_fields(
        &mut self,
    ) -> Result<(&'a [FieldPattern<'a>], Option<Region>), error::PRecord<'a>> {
        self.chomp();
        let mut fields = BumpVec::new_in(self.bump);
        loop {
            match self.peek() {
                Some(b'}') => {
                    self.advance();
                    return Ok((fields.into_bump_slice(), None));
                }
                Some(b'.') if self.peek_at(1) == Some(b'.') => {
                    let rest_start = self.get_position();
                    self.advance_by(2);
                    let rest = Region::new(rest_start, self.get_position());
                    self.chomp();
                    if self.peek() == Some(b',') {
                        self.advance();
                        self.chomp();
                    }
                    if self.peek() == Some(b'}') {
                        self.advance();
                        return Ok((fields.into_bump_slice(), Some(rest)));
                    }
                    let (row, col) = self.position();
                    return Err(PRecord::RestNotLast(row, col));
                }
                _ => {
                    let name = self.located_lower(PRecord::Field)?;
                    self.chomp();
                    let pattern = if self.peek() == Some(b':') {
                        self.advance();
                        self.chomp();
                        Some(self.specialize(
                            |bump, e, row, col| PRecord::Pattern(bump.alloc(e), row, col),
                            |p| p.pattern(),
                        )?)
                    } else {
                        None
                    };
                    fields.push(FieldPattern { name, pattern });
                    match self.peek() {
                        Some(b',') => {
                            self.advance();
                            self.chomp();
                        }
                        Some(b'}') => {
                            self.advance();
                            return Ok((fields.into_bump_slice(), None));
                        }
                        _ => {
                            let (row, col) = self.position();
                            return Err(PRecord::End(row, col));
                        }
                    }
                }
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
    fn single_shorthand() {
        assert_pattern_snapshot!("{ x }");
    }

    #[test]
    fn multiple_shorthand() {
        assert_pattern_snapshot!("{ x, y }");
    }

    #[test]
    fn renamed_field() {
        assert_pattern_snapshot!("{ x: a }");
    }

    #[test]
    fn nested_pattern() {
        assert_pattern_snapshot!("{ point: (x, y) }");
    }

    #[test]
    fn rest() {
        assert_pattern_snapshot!("{ x, .. }");
    }

    #[test]
    fn error_rest_not_last() {
        assert_pattern_error_snapshot!("{ .., x }");
    }

    #[test]
    fn error_unclosed() {
        assert_pattern_error_snapshot!("{ x, y");
    }
}
