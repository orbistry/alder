//! Array patterns `[a, b, ..rest]`.
//!
//! `[]`, `[a]`, `[a, b]`, `[a, ..]`, `[a, ..rest]`, `[..]`. The rest must be
//! the last element (a trailing comma after it is fine); a rest name must
//! follow `..` directly. Trailing commas are accepted (§10.8).
//!
//! See docs/parser-internals.md §5.14.
// OWNER: pattern/array.rs (Wave 1)

use alder_region::{Located, Position, Region};
use alder_source::{ArrayRest, Pattern};
use bumpalo::collections::Vec as BumpVec;

use crate::error::PArray;
use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// At `[`.
    pub(super) fn pattern_array(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, error::PArray<'a>> {
        self.advance();
        self.chomp();
        let mut elements = BumpVec::new_in(self.bump);
        let mut rest = None;
        loop {
            match self.peek() {
                Some(b']') => {
                    self.advance();
                    break;
                }
                Some(b'.') if self.peek_at(1) == Some(b'.') => {
                    let rest_start = self.get_position();
                    self.advance_by(2);
                    let name = if self.peek_lower() {
                        Some(self.located_lower(PArray::End)?)
                    } else {
                        None
                    };
                    rest = Some(ArrayRest {
                        region: Region::new(rest_start, self.get_position()),
                        name,
                    });
                    self.chomp();
                    if self.peek() == Some(b',') {
                        self.advance();
                        self.chomp();
                    }
                    if self.peek() == Some(b']') {
                        self.advance();
                        break;
                    }
                    let (row, col) = self.position();
                    return Err(PArray::RestNotLast(row, col));
                }
                _ => {
                    let element = self.specialize(
                        |bump, e, row, col| PArray::Pattern(bump.alloc(e), row, col),
                        |p| p.pattern(),
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
                            return Err(PArray::End(row, col));
                        }
                    }
                }
            }
        }
        Ok(self.add_end(
            start,
            Pattern::Array {
                elements: elements.into_bump_slice(),
                rest,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{assert_pattern_error_snapshot, assert_pattern_snapshot};

    #[test]
    fn empty() {
        assert_pattern_snapshot!("[]");
    }

    #[test]
    fn single() {
        assert_pattern_snapshot!("[a]");
    }

    #[test]
    fn multiple() {
        assert_pattern_snapshot!("[a, b, c]");
    }

    #[test]
    fn rest_anonymous() {
        assert_pattern_snapshot!("[a, ..]");
    }

    #[test]
    fn rest_named() {
        assert_pattern_snapshot!("[first, ..rest]");
    }

    #[test]
    fn rest_only() {
        assert_pattern_snapshot!("[..]");
    }

    #[test]
    fn error_rest_not_last() {
        assert_pattern_error_snapshot!("[..rest, x]");
    }

    #[test]
    fn error_unclosed() {
        assert_pattern_error_snapshot!("[a, b");
    }
}
