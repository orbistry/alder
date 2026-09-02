//! Record literals and the record-vs-block lookahead (docs/parser-internals.md §2.2).
//!
//! `{ name: value }`, shorthand `{ name }`, spread `{ ..r, x: 1 }`. Fields
//! are parsed with record constructors re-enabled (§2.3). `record_fields`
//! starts after the `{` and consumes the `}`; it is shared with record
//! constructors (`path.rs`) and the query `set` clause, which is why it
//! neither wraps its errors nor touches the record-constructor flag itself.
//!
//! `{ x = 1 }` is the Elm habit `Record::EqualsNotColon`; a `==` after a
//! shorthand field is left to fail as `Record::End`.
//!
//! See docs/parser-internals.md §5.13.
// OWNER: expression/record.rs (Wave 2)

use alder_region::{Located, Position};
use alder_source::{Expr, RecordField};
use bumpalo::collections::Vec as BumpVec;

use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// At `{` (already known to look like a record).
    pub(crate) fn record(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let fields = self.specialize(
            |bump, e, row, col| error::Expr::Record(bump.alloc(e), row, col),
            |p| {
                p.advance();
                p.with_record_ctor(true, |p| p.record_fields())
            },
        )?;
        Ok(self.add_end(start, Expr::Record(fields)))
    }

    /// After `{`; also RecordCtor and query `set`. Consumes the closing `}`.
    pub(crate) fn record_fields(&mut self) -> Result<&'a [RecordField<'a>], error::Record<'a>> {
        self.chomp();
        let mut fields = BumpVec::new_in(self.bump);
        loop {
            match self.peek() {
                Some(b'}') => {
                    self.advance();
                    break;
                }
                Some(b'.') if self.peek_at(1) == Some(b'.') => {
                    self.advance_by(2);
                    self.chomp();
                    let expr = self.specialize(
                        |bump, e, row, col| error::Record::Spread(bump.alloc(e), row, col),
                        |p| p.expression(),
                    )?;
                    fields.push(RecordField::Spread(expr));
                }
                _ => {
                    let name = self.located_lower(error::Record::Field)?;
                    self.chomp();
                    let value = match self.peek() {
                        Some(b':') => {
                            self.advance();
                            self.chomp();
                            Some(self.specialize(
                                |bump, e, row, col| error::Record::Expr(bump.alloc(e), row, col),
                                |p| p.expression(),
                            )?)
                        }
                        Some(b'=') if !matches!(self.peek_at(1), Some(b'=' | b'>')) => {
                            let (row, col) = self.position();
                            return Err(error::Record::EqualsNotColon(row, col));
                        }
                        _ => None,
                    };
                    fields.push(RecordField::Field { name, value });
                }
            }
            match self.peek() {
                Some(b',') => {
                    self.advance();
                    self.chomp();
                }
                Some(b'}') => {
                    self.advance();
                    break;
                }
                _ => {
                    let (row, col) = self.position();
                    return Err(error::Record::End(row, col));
                }
            }
        }
        Ok(fields.into_bump_slice())
    }

    /// Lookahead at `{` (§2.2): a record iff, after whitespace, the next
    /// token is `}`, `..`, or a `lower_ident` (not reserved, not a SQL word
    /// in query mode) followed after whitespace by `:`, `,` or `}` — or by
    /// a plain `=`, so that the Elm habit `{ x = 1 }` reaches
    /// `Record::EqualsNotColon` instead of parsing as a block containing an
    /// assignment.
    pub(crate) fn looks_like_record(&mut self) -> bool {
        self.lookahead(|p| {
            p.advance();
            p.chomp();
            match p.peek() {
                Some(b'}') => true,
                Some(b'.') => p.peek_at(1) == Some(b'.'),
                Some(b) if b.is_ascii_lowercase() => {
                    if p.lower_name(|_, _| ()).is_err() {
                        return false;
                    }
                    p.chomp();
                    match p.peek() {
                        Some(b',' | b'}') => true,
                        // `x::` never starts a field.
                        Some(b':') => p.peek_at(1) != Some(b':'),
                        // `x = 1`, but not `x == 1` / `x => …`.
                        Some(b'=') => !matches!(p.peek_at(1), Some(b'=' | b'>')),
                        _ => false,
                    }
                }
                _ => false,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{assert_expression_error_snapshot, assert_expression_snapshot};

    #[test]
    fn empty() {
        assert_expression_snapshot!("{}");
    }

    #[test]
    fn single_field() {
        assert_expression_snapshot!("{ x: 1 }");
    }

    #[test]
    fn two_fields() {
        assert_expression_snapshot!("{ x: 1, y: 2 }");
    }

    #[test]
    fn shorthand_single() {
        assert_expression_snapshot!("{ x }");
    }

    #[test]
    fn shorthand_multiple() {
        assert_expression_snapshot!("{ user, prefs }");
    }

    #[test]
    fn mixed_shorthand() {
        assert_expression_snapshot!("{ id, name: \"Ada\" }");
    }

    #[test]
    fn spread_first() {
        assert_expression_snapshot!("{ ..user, name }");
    }

    #[test]
    fn spread_with_fields() {
        assert_expression_snapshot!("{ ..r, x: 1, y: 2 }");
    }

    #[test]
    fn spread_only() {
        assert_expression_snapshot!("{ ..r }");
    }

    #[test]
    fn nested_record() {
        assert_expression_snapshot!("{ pos: { x: 1, y: 2 } }");
    }

    #[test]
    fn trailing_comma() {
        assert_expression_snapshot!("{ x: 1, }");
    }

    #[test]
    fn multiline() {
        assert_expression_snapshot!(
            r#"
            {
                id: 1,
                name: "Ada",
            }
            "#
        );
    }

    #[test]
    #[ignore = "waits for statement.rs"]
    fn block_vs_record_let() {
        assert_expression_snapshot!(
            r#"
            {
                let y = x * 2
                y + 1
            }
            "#
        );
    }

    #[test]
    #[ignore = "waits for statement.rs"]
    fn block_vs_record_call() {
        assert_expression_snapshot!("{ f(x) }");
    }

    #[test]
    fn error_unclosed() {
        assert_expression_error_snapshot!("{ x: 1");
    }

    #[test]
    fn error_missing_value() {
        assert_expression_error_snapshot!("{ x: }");
    }

    #[test]
    fn error_uppercase_field() {
        assert_expression_error_snapshot!("{ x: 1, Foo: 2 }");
    }

    #[test]
    fn error_spread_no_expr() {
        assert_expression_error_snapshot!("{ .. }");
    }

    #[test]
    fn error_equals_not_colon() {
        assert_expression_error_snapshot!("{ x = 1 }");
    }
}
