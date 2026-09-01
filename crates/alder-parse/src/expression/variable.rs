//! Variable expression parsing for Alder.
//!
//! Ported from Elm's `Parse/Variable.hs`.
//!
//! Provides:
//! - `lower_name` - parse a lowercase identifier (for field names, etc.)
//! - `upper_name` - parse an uppercase identifier (for type names, etc.)
//! - `foreign_alpha` - parse a possibly-qualified variable expression

use alder_region::{Located, Position};
use alder_source::{Expr, VarType};

use crate::Parser;
use crate::error;
use crate::keyword;

impl<'a> Parser<'a> {
    // -------------------------------------------------------------------------
    // Primitive name parsers (pub(crate) for reuse by record, pattern, etc.)
    // -------------------------------------------------------------------------

    /// Parse a lowercase name.
    ///
    /// Mirrors Elm's `Var.lower`:
    /// ```haskell
    /// lower :: (Row -> Col -> x) -> Parser x Name.Name
    /// ```
    ///
    /// Parses `[a-z][a-zA-Z0-9_]*`, checks it's not a reserved word.
    pub(crate) fn lower_name<E>(
        &mut self,
        to_error: impl FnOnce(u16, u16) -> E,
    ) -> Result<&'a str, E> {
        let (row, col) = self.position();
        let start_pos = self.pos;

        match self.peek() {
            Some(b) if b.is_ascii_lowercase() => {
                self.advance();
                self.chomp_inner_chars();

                let name = self.slice_from(start_pos);

                if keyword::is_reserved(name) {
                    return Err(to_error(row, col));
                }

                Ok(name)
            }
            _ => Err(to_error(row, col)),
        }
    }

    /// Parse an uppercase name.
    ///
    /// Mirrors Elm's `Var.upper`:
    /// ```haskell
    /// upper :: (Row -> Col -> x) -> Parser x Name.Name
    /// ```
    ///
    /// Parses `[A-Z][a-zA-Z0-9_]*`. No reserved word check for uppercase.
    pub(crate) fn upper_name<E>(
        &mut self,
        to_error: impl FnOnce(u16, u16) -> E,
    ) -> Result<&'a str, E> {
        let (row, col) = self.position();
        let start_pos = self.pos;

        match self.peek() {
            Some(b) if b.is_ascii_uppercase() => {
                self.advance();
                self.chomp_inner_chars();
                Ok(self.slice_from(start_pos))
            }
            _ => Err(to_error(row, col)),
        }
    }

    // -------------------------------------------------------------------------
    // Expression-level variable parsing
    // -------------------------------------------------------------------------

    /// Parse a variable expression.
    ///
    /// Mirrors Elm's `variable` in Expression.hs:
    /// ```haskell
    /// variable start =
    ///   do  var <- Var.foreignAlpha E.Start
    ///       addEnd start var
    /// ```
    pub(crate) fn variable(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let expr = self.foreign_alpha(error::Expr::Start)?;
        Ok(self.add_end(start, expr))
    }

    /// Parse a possibly-qualified variable, returning the expression directly.
    ///
    /// Mirrors Elm's `Variable.foreignAlpha`:
    /// ```haskell
    /// foreignAlpha :: (Row -> Col -> x) -> Parser x Src.Expr_
    /// ```
    ///
    /// Parses:
    /// - `foo` -> Var(LowVar, "foo")
    /// - `Foo` -> Var(CapVar, "Foo")
    /// - `Module.foo` -> VarQual(LowVar, "Module", "foo")
    /// - `Module.Foo` -> VarQual(CapVar, "Module", "Foo")
    /// - `A.B.C.foo` -> VarQual(LowVar, "A.B.C", "foo")
    fn foreign_alpha<E>(&mut self, to_error: impl FnOnce(u16, u16) -> E) -> Result<Expr<'a>, E> {
        let (row, col) = self.position();
        let start_pos = self.pos;

        match self.peek() {
            // Lowercase - simple variable, no qualification possible
            Some(b) if b.is_ascii_lowercase() => {
                // Save state in case this is a reserved keyword
                let saved = self.save_state();

                self.advance();
                self.chomp_inner_chars();

                let name = self.slice_from(start_pos);

                if keyword::is_reserved(name) {
                    // Restore state so one_of sees no input consumed
                    self.restore_state(saved);
                    return Err(to_error(row, col));
                }

                Ok(Expr::Var {
                    kind: VarType::LowVar,
                    name,
                })
            }

            // Uppercase - might be qualified
            Some(b) if b.is_ascii_uppercase() => {
                self.advance();
                self.chomp_inner_chars();

                // Check for qualification chain
                if self.is_dot_upper() {
                    self.chomp_qualified_upper(start_pos, row, col, to_error)
                } else if self.is_dot_lower() {
                    self.parse_qualified_lower(start_pos, row, col, to_error)
                } else {
                    // Simple uppercase
                    let name = self.slice_from(start_pos);
                    Ok(Expr::Var {
                        kind: VarType::CapVar,
                        name,
                    })
                }
            }

            _ => Err(to_error(row, col)),
        }
    }

    // -------------------------------------------------------------------------
    // Helper methods
    // -------------------------------------------------------------------------

    /// Chomp inner characters of an identifier (a-z, A-Z, 0-9, _).
    pub(crate) fn chomp_inner_chars(&mut self) {
        loop {
            match self.peek() {
                Some(b) if b.is_ascii_alphanumeric() || b == b'_' => {
                    self.advance();
                }
                _ => break,
            }
        }
    }

    /// Get a str slice from start_pos to current position.
    pub(crate) fn slice_from(&self, start_pos: usize) -> &'a str {
        let bytes = &self.src[start_pos..self.pos];
        unsafe { std::str::from_utf8_unchecked(bytes) }
    }

    /// Check if current position is a dot followed by uppercase.
    #[inline]
    pub(crate) fn is_dot_upper(&self) -> bool {
        self.peek() == Some(b'.') && matches!(self.peek_at(1), Some(b) if b.is_ascii_uppercase())
    }

    /// Check if current position is a dot followed by lowercase.
    #[inline]
    pub(crate) fn is_dot_lower(&self) -> bool {
        self.peek() == Some(b'.') && matches!(self.peek_at(1), Some(b) if b.is_ascii_lowercase())
    }

    /// Parse Module.name (qualified lowercase).
    fn parse_qualified_lower<E>(
        &mut self,
        start_pos: usize,
        row: u16,
        col: u16,
        to_error: impl FnOnce(u16, u16) -> E,
    ) -> Result<Expr<'a>, E> {
        let module_end = self.pos;
        self.advance(); // consume dot
        let name_start = self.pos;
        self.advance(); // consume first lowercase char
        self.chomp_inner_chars();

        let module = unsafe { std::str::from_utf8_unchecked(&self.src[start_pos..module_end]) };
        let name = unsafe { std::str::from_utf8_unchecked(&self.src[name_start..self.pos]) };

        if keyword::is_reserved(name) {
            return Err(to_error(row, col));
        }

        Ok(Expr::VarQual {
            kind: VarType::LowVar,
            module,
            name,
        })
    }

    /// Chomp through Module.Module... chain, ending in either .Name or .name.
    fn chomp_qualified_upper<E>(
        &mut self,
        start_pos: usize,
        row: u16,
        col: u16,
        to_error: impl FnOnce(u16, u16) -> E,
    ) -> Result<Expr<'a>, E> {
        loop {
            if self.is_dot_upper() {
                self.advance(); // consume dot
                self.advance(); // consume first uppercase char
                self.chomp_inner_chars();
            } else if self.is_dot_lower() {
                return self.parse_qualified_lower(start_pos, row, col, to_error);
            } else {
                // No more dots - this is qualified uppercase: Module.Type
                let name = self.slice_from(start_pos);
                return Ok(Expr::Var {
                    kind: VarType::CapVar,
                    name,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::expression::{assert_expr_error_snapshot, assert_expr_snapshot};

    #[test]
    fn lower_simple() {
        assert_expr_snapshot!("foo");
    }

    #[test]
    fn lower_camel_case() {
        assert_expr_snapshot!("fooBar");
    }

    #[test]
    fn lower_with_numbers() {
        assert_expr_snapshot!("foo123");
    }

    #[test]
    fn lower_with_underscore() {
        assert_expr_snapshot!("foo_bar");
    }

    #[test]
    fn upper_simple() {
        assert_expr_snapshot!("Foo");
    }

    #[test]
    fn upper_constructor() {
        assert_expr_snapshot!("Just");
    }

    #[test]
    fn qualified_lower() {
        assert_expr_snapshot!("Module.foo");
    }

    #[test]
    fn qualified_upper() {
        assert_expr_snapshot!("Module.Foo");
    }

    #[test]
    fn multi_qualified_lower() {
        assert_expr_snapshot!("A.B.C.foo");
    }

    #[test]
    fn multi_qualified_upper() {
        assert_expr_snapshot!("A.B.C.Foo");
    }

    #[test]
    fn error_reserved_if() {
        assert_expr_error_snapshot!("if");
    }

    #[test]
    fn error_reserved_let() {
        assert_expr_error_snapshot!("let");
    }

    #[test]
    fn not_reserved_prefix() {
        assert_expr_snapshot!("letter");
    }
}
