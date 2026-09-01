//! Keyword handling for Alder.
//!
//! Ported from Elm's `Parse/Keyword.hs` and `Parse/Variable.hs`.

use crate::Parser;

/// Reserved words that cannot be used as variable names.
pub const RESERVED: &[&str] = &[
    "if", "then", "else", "case", "of", "let", "in", "type", "module", "where", "import",
    "exposing", "as", "port",
];

/// Check if a name is a reserved keyword.
#[inline]
pub fn is_reserved(name: &str) -> bool {
    RESERVED.contains(&name)
}

use crate::{Col, Row};

impl<'a> Parser<'a> {
    /// Parse the `if` keyword.
    ///
    /// Mirrors Elm's `Keyword.if_`.
    pub fn keyword_if<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E> {
        self.keyword(b"if", to_error)
    }

    /// Parse the `then` keyword.
    ///
    /// Mirrors Elm's `Keyword.then_`.
    pub fn keyword_then<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E> {
        self.keyword(b"then", to_error)
    }

    /// Parse the `else` keyword.
    ///
    /// Mirrors Elm's `Keyword.else_`.
    pub fn keyword_else<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E> {
        self.keyword(b"else", to_error)
    }

    /// Parse the `case` keyword.
    ///
    /// Mirrors Elm's `Keyword.case_`.
    pub fn keyword_case<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E> {
        self.keyword(b"case", to_error)
    }

    /// Parse the `of` keyword.
    ///
    /// Mirrors Elm's `Keyword.of_`.
    pub fn keyword_of<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E> {
        self.keyword(b"of", to_error)
    }

    /// Parse the `let` keyword.
    ///
    /// Mirrors Elm's `Keyword.let_`.
    pub fn keyword_let<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E> {
        self.keyword(b"let", to_error)
    }

    /// Parse the `in` keyword.
    ///
    /// Mirrors Elm's `Keyword.in_`.
    pub fn keyword_in<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E> {
        self.keyword(b"in", to_error)
    }

    /// Parse the `type` keyword.
    ///
    /// Mirrors Elm's `Keyword.type_`.
    pub fn keyword_type<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E> {
        self.keyword(b"type", to_error)
    }

    /// Parse the `alias` keyword.
    ///
    /// Mirrors Elm's `Keyword.alias_`.
    pub fn keyword_alias<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E> {
        self.keyword(b"alias", to_error)
    }

    /// Parse the `infix` keyword.
    ///
    /// Mirrors Elm's `Keyword.infix_`.
    pub fn keyword_infix<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E> {
        self.keyword(b"infix", to_error)
    }

    /// Parse the `left` keyword (for infix associativity).
    ///
    /// Mirrors Elm's `Keyword.left_`.
    pub fn keyword_left<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E> {
        self.keyword(b"left", to_error)
    }

    /// Parse the `right` keyword (for infix associativity).
    ///
    /// Mirrors Elm's `Keyword.right_`.
    pub fn keyword_right<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E> {
        self.keyword(b"right", to_error)
    }

    /// Parse the `non` keyword (for infix associativity).
    ///
    /// Mirrors Elm's `Keyword.non_`.
    pub fn keyword_non<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E> {
        self.keyword(b"non", to_error)
    }

    /// Parse the `import` keyword.
    ///
    /// Mirrors Elm's `Keyword.import_`.
    pub fn keyword_import<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E> {
        self.keyword(b"import", to_error)
    }

    /// Parse the `as` keyword.
    ///
    /// Mirrors Elm's `Keyword.as_`.
    pub fn keyword_as<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E> {
        self.keyword(b"as", to_error)
    }

    /// Parse the `exposing` keyword.
    ///
    /// Mirrors Elm's `Keyword.exposing_`.
    pub fn keyword_exposing<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E> {
        self.keyword(b"exposing", to_error)
    }

    /// Parse the `module` keyword.
    ///
    /// Mirrors Elm's `Keyword.module_`.
    pub fn keyword_module<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E> {
        self.keyword(b"module", to_error)
    }

    /// Generic keyword parser that checks bytes match and no identifier continuation follows.
    ///
    /// Mirrors Elm's `k2`, `k3`, `k4` etc. but generalized.
    fn keyword<E>(&mut self, kw: &[u8], to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E> {
        // Check if we have enough bytes
        if self.pos + kw.len() > self.src.len() {
            let (row, col) = self.position();
            return Err(to_error(row, col));
        }

        // Check if bytes match
        if &self.src[self.pos..self.pos + kw.len()] != kw {
            let (row, col) = self.position();
            return Err(to_error(row, col));
        }

        // Check that no identifier continuation follows (like Elm's getInnerWidth == 0)
        let next_pos = self.pos + kw.len();
        if next_pos < self.src.len() {
            let next_byte = self.src[next_pos];
            if is_inner_char(next_byte) {
                let (row, col) = self.position();
                return Err(to_error(row, col));
            }
        }

        // Success - advance position and update column
        for _ in 0..kw.len() {
            self.advance();
        }
        Ok(())
    }
}

/// Check if a byte is a valid identifier continuation character.
///
/// Matches: a-z, A-Z, 0-9, _
#[inline]
fn is_inner_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}
