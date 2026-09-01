//! Operator/symbol parsing for Alder.
//!
//! Ported from Elm's `Parse/Symbol.hs`.

use alder_region::{Located, Region};

use crate::Parser;
use crate::error::BadOperator;

impl<'a> Parser<'a> {
    /// Parse an operator (binary op or symbolic identifier).
    ///
    /// Mirrors Elm's `Symbol.operator`:
    /// ```haskell
    /// operator :: (Row -> Col -> x) -> (BadOperator -> Row -> Col -> x) -> Parser x Name.Name
    /// ```
    ///
    /// Valid operator characters: `+-/*=.<>:&|^?%!`
    ///
    /// Reserved operators that cannot be parsed standalone:
    /// - `.` (dot)
    /// - `|` (pipe - reserved for case branches)
    /// - `->` (arrow - reserved for lambdas/types)
    /// - `=` (equals - reserved for definitions)
    /// - `:` (colon - reserved for type annotations)
    pub(crate) fn operator<E>(
        &mut self,
        to_expectation: impl FnOnce(u16, u16) -> E,
        to_error: impl FnOnce(BadOperator, u16, u16) -> E,
    ) -> Result<&'a str, E> {
        let (row, col) = self.position();
        let start_pos = self.pos;

        // Chomp all operator characters
        while let Some(b) = self.peek() {
            if is_binop_char(b) {
                self.advance();
            } else {
                break;
            }
        }

        // No operator characters found
        if self.pos == start_pos {
            return Err(to_expectation(row, col));
        }

        let op = self.slice_from(start_pos);

        // Check for reserved operators
        match op {
            "." => Err(to_error(BadOperator::Dot, row, col)),
            "|" => Err(to_error(BadOperator::Pipe, row, col)),
            "->" => Err(to_error(BadOperator::Arrow, row, col)),
            "=" => Err(to_error(BadOperator::Equals, row, col)),
            ":" => Err(to_error(BadOperator::HasType, row, col)),
            _ => Ok(op),
        }
    }

    /// Parse an operator and wrap it in a Located.
    pub(crate) fn add_location_operator<E>(
        &mut self,
        to_expectation: impl FnOnce(u16, u16) -> E,
        to_error: impl FnOnce(BadOperator, u16, u16) -> E,
    ) -> Result<&'a Located<&'a str>, E> {
        let start = self.get_position();
        let op = self.operator(to_expectation, to_error)?;
        let end = self.get_position();
        Ok(self.alloc(Located::at(Region::new(start, end), op)))
    }
}

/// Check if a byte is a valid operator character.
///
/// Valid: `+-/*=.<>:&|^?%!`
#[inline]
pub fn is_binop_char(b: u8) -> bool {
    matches!(
        b,
        b'+' | b'-'
            | b'/'
            | b'*'
            | b'='
            | b'.'
            | b'<'
            | b'>'
            | b':'
            | b'&'
            | b'|'
            | b'^'
            | b'?'
            | b'%'
            | b'!'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_binop_char() {
        assert!(is_binop_char(b'+'));
        assert!(is_binop_char(b'-'));
        assert!(is_binop_char(b'*'));
        assert!(is_binop_char(b'/'));
        assert!(is_binop_char(b'<'));
        assert!(is_binop_char(b'>'));
        assert!(is_binop_char(b'|'));
        assert!(is_binop_char(b'&'));
        assert!(!is_binop_char(b'a'));
        assert!(!is_binop_char(b' '));
    }
}
