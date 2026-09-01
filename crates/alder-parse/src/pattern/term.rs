//! Atomic pattern parsing for Alder.
//!
//! Ported from Elm's `Pattern.termHelp`.
//!
//! Handles: wildcard, variable, constructor (no args), number, string.

use alder_region::{Position, Region};
use alder_source::Pattern;

use crate::Parser;
use crate::error;

impl<'a> Parser<'a> {
    /// Parse termHelp patterns: wildcard, var, ctor, number, string.
    ///
    /// Mirrors Elm's `termHelp`.
    pub(super) fn pattern_term_help(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        self.one_of(
            error::Pattern::Start,
            vec![
                // Wildcard: _
                Box::new(|p: &mut Parser<'a>| p.pattern_wildcard(start)),
                // Variable: lowercase name
                Box::new(|p: &mut Parser<'a>| p.pattern_var(start)),
                // Constructor: uppercase name (no args in term)
                Box::new(|p: &mut Parser<'a>| p.pattern_ctor(start)),
                // Number literal
                Box::new(|p: &mut Parser<'a>| p.pattern_number(start)),
                // String literal
                Box::new(|p: &mut Parser<'a>| p.pattern_string(start)),
            ],
        )
    }

    /// Parse a wildcard pattern: `_`
    fn pattern_wildcard(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        let (row, col) = self.position();

        if self.peek() != Some(b'_') {
            return Err(error::Pattern::Start(row, col));
        }

        self.advance();

        // Check that it's not followed by more identifier chars (like `_foo`)
        if matches!(self.peek(), Some(b) if b.is_ascii_alphanumeric() || b == b'_') {
            // It's a variable starting with underscore, not a wildcard
            let start_pos = self.pos - 1; // include the underscore
            self.chomp_inner_chars();
            let name = self.slice_from(start_pos);
            let width = (self.col - col) as i32;
            return Err(error::Pattern::WildcardNotVar(name, width, row, col));
        }

        Ok(self.add_end(start, Pattern::Anything))
    }

    /// Parse a variable pattern: lowercase name
    fn pattern_var(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        let name = self.lower_name(error::Pattern::Start)?;
        Ok(self.add_end(start, Pattern::Var(name)))
    }

    /// Parse a constructor pattern (no args in term).
    pub(super) fn pattern_ctor(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        let (row, col) = self.position();
        let ctor_start = self.pos;

        match self.peek() {
            Some(b) if b.is_ascii_uppercase() => {
                self.advance();
                self.chomp_inner_chars();

                // Check for qualified name
                if self.is_dot_upper() || self.is_dot_lower() {
                    self.pattern_ctor_qualified(start, ctor_start, row, col)
                } else {
                    // Simple unqualified constructor
                    let name = self.slice_from(ctor_start);
                    let end = self.get_position();
                    let region = Region::new(start, end);
                    let empty: &'a [&'a Located<Pattern<'a>>] = &[];
                    Ok(self.add_end(
                        start,
                        Pattern::Ctor {
                            region,
                            name,
                            args: empty,
                        },
                    ))
                }
            }
            _ => Err(error::Pattern::Start(row, col)),
        }
    }

    /// Parse a qualified constructor pattern.
    fn pattern_ctor_qualified(
        &mut self,
        start: Position,
        ctor_start: usize,
        row: u16,
        col: u16,
    ) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        // Keep chomping Module.Module... until we hit the final name
        loop {
            if self.is_dot_upper() {
                self.advance(); // consume dot
                self.advance(); // consume first uppercase char
                self.chomp_inner_chars();
            } else if self.is_dot_lower() {
                // Qualified lowercase - this is an error for patterns
                // (you can't have Module.foo as a pattern)
                return Err(error::Pattern::Start(row, col));
            } else {
                // No more dots - we have the full qualified name
                break;
            }
        }

        // Split into module and name
        let full = self.slice_from(ctor_start);
        // Find the last dot to split module.Name
        if let Some(last_dot) = full.rfind('.') {
            let module = &full[..last_dot];
            let name = &full[last_dot + 1..];
            let end = self.get_position();
            let region = Region::new(start, end);
            let empty: &'a [&'a Located<Pattern<'a>>] = &[];
            Ok(self.add_end(
                start,
                Pattern::CtorQual {
                    region,
                    module,
                    name,
                    args: empty,
                },
            ))
        } else {
            // No dot means unqualified
            let end = self.get_position();
            let region = Region::new(start, end);
            let empty: &'a [&'a Located<Pattern<'a>>] = &[];
            Ok(self.add_end(
                start,
                Pattern::Ctor {
                    region,
                    name: full,
                    args: empty,
                },
            ))
        }
    }

    /// Parse a number pattern.
    fn pattern_number(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        let n = self.number_literal(error::Pattern::Start, |num_err, row, col| {
            error::Pattern::Number(num_err, row, col)
        })?;
        Ok(self.add_end(start, Pattern::Int(n)))
    }

    /// Parse a string pattern.
    fn pattern_string(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        let s = self.string_literal(error::Pattern::Start, |str_err, row, col| {
            error::Pattern::String(str_err, row, col)
        })?;
        Ok(self.add_end(start, Pattern::Str(s)))
    }
}

use alder_region::Located;

#[cfg(test)]
mod tests {
    use super::super::{assert_pattern_error_snapshot, assert_pattern_snapshot};

    // Wildcard
    #[test]
    fn wildcard() {
        assert_pattern_snapshot!("_");
    }

    // Variables
    #[test]
    fn variable() {
        assert_pattern_snapshot!("foo");
    }

    #[test]
    fn variable_with_numbers() {
        assert_pattern_snapshot!("foo123");
    }

    // Constructors
    #[test]
    fn ctor_simple() {
        assert_pattern_snapshot!("Nothing");
    }

    #[test]
    fn ctor_qualified() {
        assert_pattern_snapshot!("Maybe.Nothing");
    }

    #[test]
    fn ctor_multi_qualified() {
        assert_pattern_snapshot!("Data.Maybe.Nothing");
    }

    // Literals
    #[test]
    fn int_literal() {
        assert_pattern_snapshot!("42");
    }

    #[test]
    fn string_literal() {
        assert_pattern_snapshot!(r#""hello""#);
    }

    // Errors
    #[test]
    fn error_wildcard_not_var() {
        assert_pattern_error_snapshot!("_foo");
    }
}
