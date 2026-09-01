//! String literal parsing for Alder.
//!
//! Ported from Elm's `Parse/String.hs`.

use crate::error::{Escape, StringError};
use crate::{Col, Parser, Row};

/// Internal result type for string parsing.
enum StringResult<'a> {
    Ok(&'a str),
    Err(StringError, Row, Col),
}

impl<'a> Parser<'a> {
    /// Parse a string literal with custom error constructors.
    ///
    /// Mirrors Elm's `String.string`:
    /// ```haskell
    /// string :: (Row -> Col -> x) -> (E.String -> Row -> Col -> x) -> Parser x ES.String
    /// ```
    ///
    /// Handles both single-line (`"..."`) and multi-line (`"""..."""`) strings.
    pub fn string_literal<E>(
        &mut self,
        to_expectation: impl FnOnce(Row, Col) -> E,
        to_error: impl FnOnce(StringError, Row, Col) -> E,
    ) -> Result<&'a str, E> {
        let (row, col) = self.position();

        // Must start with double quote
        if self.peek() != Some(b'"') {
            return Err(to_expectation(row, col));
        }

        self.advance(); // consume first "

        // Check for multi-line string (""")
        if self.peek() == Some(b'"') {
            self.advance(); // consume second "

            if self.peek() == Some(b'"') {
                self.advance(); // consume third "
                // Multi-line string
                let result = self.chomp_multi_string();
                match result {
                    StringResult::Ok(s) => Ok(s),
                    StringResult::Err(e, r, c) => Err(to_error(e, r, c)),
                }
            } else {
                // Empty string ""
                Ok(self.alloc_str(""))
            }
        } else {
            // Single-line string
            let result = self.chomp_single_string();
            match result {
                StringResult::Ok(s) => Ok(s),
                StringResult::Err(e, r, c) => Err(to_error(e, r, c)),
            }
        }
    }

    /// Parse a single-line string (content after opening `"`).
    fn chomp_single_string(&mut self) -> StringResult<'a> {
        let start_pos = self.pos;
        let (start_row, start_col) = self.position();
        let mut needs_escape = false;

        loop {
            match self.peek() {
                None => {
                    // End of file without closing quote
                    return StringResult::Err(StringError::EndlessSingle, start_row, start_col);
                }
                Some(b'\n') => {
                    // Newline in single-line string
                    return StringResult::Err(StringError::EndlessSingle, self.row(), self.col());
                }
                Some(b'"') => {
                    // End of string
                    let end_pos = self.pos;
                    self.advance(); // consume closing "

                    if needs_escape {
                        // Build escaped string
                        return self.build_escaped_string(start_pos, end_pos, false);
                    } else {
                        // Return slice directly
                        let bytes = &self.src[start_pos..end_pos];
                        // SAFETY: We've verified this is valid UTF-8 by scanning byte-by-byte
                        let s = unsafe { std::str::from_utf8_unchecked(bytes) };
                        return StringResult::Ok(s);
                    }
                }
                Some(b'\\') => {
                    needs_escape = true;
                    self.advance(); // consume backslash

                    match self.eat_escape() {
                        EscapeResult::Normal(width) => {
                            self.advance_by(width);
                        }
                        EscapeResult::Unicode(delta) => {
                            self.advance_by(delta);
                        }
                        EscapeResult::Problem(escape) => {
                            return StringResult::Err(
                                StringError::Escape(escape),
                                self.row(),
                                self.col(),
                            );
                        }
                        EscapeResult::EndOfFile => {
                            return StringResult::Err(
                                StringError::EndlessSingle,
                                start_row,
                                start_col,
                            );
                        }
                    }
                }
                Some(b) => {
                    // Regular character - advance by UTF-8 width
                    let width = utf8_char_width(b);
                    self.advance_by(width);
                }
            }
        }
    }

    /// Parse a multi-line string (content after opening `"""`).
    fn chomp_multi_string(&mut self) -> StringResult<'a> {
        let start_pos = self.pos;
        let (start_row, start_col) = self.position();
        let mut needs_escape = false;

        loop {
            match self.peek() {
                None => {
                    return StringResult::Err(StringError::EndlessMulti, start_row, start_col);
                }
                Some(b'"') => {
                    // Check for closing """
                    if self.peek_at(1) == Some(b'"') && self.peek_at(2) == Some(b'"') {
                        let end_pos = self.pos;
                        self.advance_by(3); // consume closing """

                        if needs_escape {
                            return self.build_escaped_string(start_pos, end_pos, true);
                        } else {
                            let bytes = &self.src[start_pos..end_pos];
                            let s = unsafe { std::str::from_utf8_unchecked(bytes) };
                            return StringResult::Ok(s);
                        }
                    } else {
                        self.advance();
                    }
                }
                Some(b'\n') => {
                    // Newlines are allowed in multi-line strings
                    needs_escape = true; // We'll normalize to \n
                    self.advance();
                }
                Some(b'\r') => {
                    // Carriage return - skip it (normalize to just \n)
                    needs_escape = true;
                    self.advance();
                }
                Some(b'\\') => {
                    needs_escape = true;
                    self.advance();

                    match self.eat_escape() {
                        EscapeResult::Normal(width) => {
                            self.advance_by(width);
                        }
                        EscapeResult::Unicode(delta) => {
                            self.advance_by(delta);
                        }
                        EscapeResult::Problem(escape) => {
                            return StringResult::Err(
                                StringError::Escape(escape),
                                self.row(),
                                self.col(),
                            );
                        }
                        EscapeResult::EndOfFile => {
                            return StringResult::Err(
                                StringError::EndlessMulti,
                                start_row,
                                start_col,
                            );
                        }
                    }
                }
                Some(b) => {
                    let width = utf8_char_width(b);
                    self.advance_by(width);
                }
            }
        }
    }

    /// Process escape sequences and build the final string.
    fn build_escaped_string(&self, start: usize, end: usize, is_multi: bool) -> StringResult<'a> {
        let mut result = String::new();
        let mut pos = start;

        while pos < end {
            let b = self.src[pos];

            if b == b'\\' {
                pos += 1;
                if pos >= end {
                    break;
                }

                match self.src[pos] {
                    b'n' => {
                        result.push('\n');
                        pos += 1;
                    }
                    b'r' => {
                        result.push('\r');
                        pos += 1;
                    }
                    b't' => {
                        result.push('\t');
                        pos += 1;
                    }
                    b'"' => {
                        result.push('"');
                        pos += 1;
                    }
                    b'\'' => {
                        result.push('\'');
                        pos += 1;
                    }
                    b'\\' => {
                        result.push('\\');
                        pos += 1;
                    }
                    b'u' => {
                        pos += 1; // skip 'u'
                        if pos < end && self.src[pos] == b'{' {
                            pos += 1; // skip '{'
                            let hex_start = pos;
                            while pos < end && self.src[pos] != b'}' {
                                pos += 1;
                            }
                            let hex_str =
                                unsafe { std::str::from_utf8_unchecked(&self.src[hex_start..pos]) };
                            if let Ok(code) = u32::from_str_radix(hex_str, 16)
                                && let Some(c) = char::from_u32(code)
                            {
                                result.push(c);
                            }
                            pos += 1; // skip '}'
                        }
                    }
                    _ => {
                        pos += 1;
                    }
                }
            } else if is_multi && b == b'\r' {
                // Skip carriage returns in multi-line strings
                pos += 1;
            } else if is_multi && b == b'\n' {
                result.push('\n');
                pos += 1;
            } else {
                // Regular UTF-8 character
                let width = utf8_char_width(b);
                let char_bytes = &self.src[pos..pos + width];
                let s = unsafe { std::str::from_utf8_unchecked(char_bytes) };
                result.push_str(s);
                pos += width;
            }
        }

        StringResult::Ok(self.alloc_str(&result))
    }

    /// Parse an escape sequence after the backslash.
    fn eat_escape(&self) -> EscapeResult {
        match self.peek() {
            None => EscapeResult::EndOfFile,
            Some(b'n') | Some(b'r') | Some(b't') | Some(b'"') | Some(b'\'') | Some(b'\\') => {
                EscapeResult::Normal(1)
            }
            Some(b'u') => self.eat_unicode(),
            Some(_) => EscapeResult::Problem(Escape::Unknown),
        }
    }

    /// Parse a unicode escape sequence `\u{...}`.
    fn eat_unicode(&self) -> EscapeResult {
        // Position is at 'u', need to check for '{'
        if self.peek_at(1) != Some(b'{') {
            return EscapeResult::Problem(Escape::BadUnicodeFormat(2));
        }

        // Count hex digits
        let mut offset = 2; // past 'u{'
        let mut num_digits = 0;
        let mut code: u32 = 0;

        loop {
            match self.peek_at(offset) {
                None => {
                    return EscapeResult::Problem(Escape::BadUnicodeFormat(offset as u16));
                }
                Some(b'}') => {
                    break;
                }
                Some(b) if b.is_ascii_hexdigit() => {
                    let digit = if b.is_ascii_digit() {
                        (b - b'0') as u32
                    } else if (b'a'..=b'f').contains(&b) {
                        (b - b'a' + 10) as u32
                    } else {
                        (b - b'A' + 10) as u32
                    };
                    code = code * 16 + digit;
                    num_digits += 1;
                    offset += 1;
                }
                Some(_) => {
                    return EscapeResult::Problem(Escape::BadUnicodeFormat(offset as u16));
                }
            }
        }

        // Check code validity
        if code > 0x10FFFF {
            return EscapeResult::Problem(Escape::BadUnicodeCode((offset + 1) as u16));
        }

        // Check digit count (must be 4-6)
        if !(4..=6).contains(&num_digits) {
            return EscapeResult::Problem(Escape::BadUnicodeLength {
                code: (offset + 1) as u16,
                expected: if num_digits < 4 { 4 } else { 6 },
                actual: num_digits,
            });
        }

        // Return total length including 'u', '{', digits, '}'
        EscapeResult::Unicode(offset + 1)
    }
}

/// Result of parsing an escape sequence.
enum EscapeResult {
    /// Normal escape like \n, width is 1
    Normal(usize),
    /// Unicode escape \u{...}, delta is total chars consumed
    Unicode(usize),
    /// End of file during escape
    EndOfFile,
    /// Invalid escape
    Problem(Escape),
}

/// Get the width of a UTF-8 character from its first byte.
#[inline]
fn utf8_char_width(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
}
