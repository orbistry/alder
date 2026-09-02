//! Raw balanced token text for macro bodies and `name!( … )` calls
//! (docs/parser-internals.md §6.5).
//!
//! The scanner never builds an AST: it walks bytes from an opener to its
//! matching closer, keeping `()[]{}` balanced and stepping over the three
//! places where a bracket byte is not a bracket — `"…"` strings, `` `…` ``
//! templates (whose `${ … }` holes are code again) and `//` comments — and
//! hands back the interior as a zero-copy slice. `quote` / `unquote` /
//! `stringify` are M5's problem; in M1 the text is opaque.
//!
//! See docs/parser-internals.md §5.9.
// OWNER: raw.rs (Wave 1)

use alder_region::Located;

use crate::error::{RawTokens, StringError};
use crate::string::EscapeResult;
use crate::{Col, Parser, Row};

/// A raw-scan failure with the position it should be reported at.
type RawError = (RawTokens, Row, Col);

// `raw_balanced` is called from `expression/postfix.rs` (`name!(`) and
// `item/macro_.rs` (`macro … { }`), both still stubs; the `allow` goes away
// in Wave 4 (docs/parser-internals.md §9 step 4.2).
#[allow(unused)]
impl<'a> Parser<'a> {
    /// At `open`. Consumes through the matching `close`, honoring nested
    /// `()[]{}`, strings, templates and `//` comments. Returns the interior text.
    ///
    /// The region covers the interior only (the byte after `open` through the
    /// byte before `close`); the delimiters are consumed but not part of the
    /// value. Nothing is chomped afterwards. Error positions:
    ///
    /// - not at `open` → `Endless` at the cursor, nothing consumed (the body
    ///   never opened, so it certainly never closed);
    /// - EOF anywhere inside → `Endless` at `open`;
    /// - a closer that is not the one expected for the innermost open bracket
    ///   (`( ]`) → `Unbalanced(byte)` at that byte;
    /// - a string or template problem → `String(_)` positioned as
    ///   `string_literal` would: `Endless` at the opening quote / backtick,
    ///   `Newline` at the newline, `Escape` at the backslash.
    pub(crate) fn raw_balanced<E>(
        &mut self,
        open: u8,
        close: u8,
        to_error: impl FnOnce(RawTokens, Row, Col) -> E,
    ) -> Result<Located<&'a str>, E> {
        let (open_row, open_col) = self.position();
        if self.peek() != Some(open) {
            return Err(to_error(RawTokens::Endless, open_row, open_col));
        }
        self.advance();

        let start = self.get_position();
        let start_pos = self.pos;
        match self.raw_code(close, open_row, open_col) {
            Ok(()) => {
                // The cursor sits on `close`; slice before consuming it so the
                // region ends where the text does.
                let text = self.slice_from(start_pos);
                let located = self.located(start, text);
                self.advance();
                Ok(located)
            }
            Err((err, row, col)) => Err(to_error(err, row, col)),
        }
    }

    /// Code mode: scan until `close` at nesting depth zero, leaving the cursor
    /// on it. `open_row` / `open_col` locate the construct being balanced for
    /// `Endless`.
    fn raw_code(&mut self, close: u8, open_row: Row, open_col: Col) -> Result<(), RawError> {
        loop {
            match self.peek() {
                None => return Err((RawTokens::Endless, open_row, open_col)),
                Some(b) if b == close => return Ok(()),
                Some(b'(') => self.raw_nested(b')', open_row, open_col)?,
                Some(b'[') => self.raw_nested(b']', open_row, open_col)?,
                Some(b'{') => self.raw_nested(b'}', open_row, open_col)?,
                Some(b @ (b')' | b']' | b'}')) => {
                    let (row, col) = self.position();
                    return Err((RawTokens::Unbalanced(b), row, col));
                }
                Some(b'"') => self.raw_string()?,
                Some(b'`') => self.raw_template(open_row, open_col)?,
                Some(b'/') if self.peek_at(1) == Some(b'/') => self.raw_comment(),
                Some(_) => self.advance(),
            }
        }
    }

    /// At a nested opener: consume it, scan through its matching `close`.
    fn raw_nested(&mut self, close: u8, open_row: Row, open_col: Col) -> Result<(), RawError> {
        self.advance();
        self.raw_code(close, open_row, open_col)?;
        self.advance();
        Ok(())
    }

    /// At `//`: skip to the end of the line (the newline itself is left for
    /// the code loop, which treats it like any other byte).
    fn raw_comment(&mut self) {
        while let Some(b) = self.peek() {
            if b == b'\n' {
                return;
            }
            self.advance();
        }
    }

    /// At `"`: step over a single-line string, consuming the closing quote.
    fn raw_string(&mut self) -> Result<(), RawError> {
        let (quote_row, quote_col) = self.position();
        self.advance();
        loop {
            match self.peek() {
                None => return Err((string_err(StringError::Endless), quote_row, quote_col)),
                Some(b'\n') => {
                    let (row, col) = self.position();
                    return Err((string_err(StringError::Newline), row, col));
                }
                Some(b'"') => {
                    self.advance();
                    return Ok(());
                }
                Some(b'\\') => self.raw_escape(false, quote_row, quote_col)?,
                Some(_) => self.advance(),
            }
        }
    }

    /// At `` ` ``: step over a template, consuming the closing backtick.
    /// Newlines are text; `${ … }` holes are code and may nest anything.
    fn raw_template(&mut self, open_row: Row, open_col: Col) -> Result<(), RawError> {
        let (tick_row, tick_col) = self.position();
        self.advance();
        loop {
            match self.peek() {
                None => return Err((string_err(StringError::Endless), tick_row, tick_col)),
                Some(b'`') => {
                    self.advance();
                    return Ok(());
                }
                Some(b'\\') => self.raw_escape(true, tick_row, tick_col)?,
                Some(b'$') if self.peek_at(1) == Some(b'{') => {
                    self.advance();
                    self.raw_nested(b'}', open_row, open_col)?;
                }
                Some(_) => self.advance(),
            }
        }
    }

    /// At `\` inside a string (`template == false`) or template: consume the
    /// backslash and the escape it introduces, using the string scanner's
    /// escape rules.
    fn raw_escape(&mut self, template: bool, lit_row: Row, lit_col: Col) -> Result<(), RawError> {
        let (slash_row, slash_col) = self.position();
        self.advance();
        match self.eat_escape(template) {
            EscapeResult::Normal(width) | EscapeResult::Unicode(width) => {
                self.advance_by(width);
                Ok(())
            }
            EscapeResult::EndOfFile => Err((string_err(StringError::Endless), lit_row, lit_col)),
            EscapeResult::Problem(escape) => Err((
                string_err(StringError::Escape(escape)),
                slash_row,
                slash_col,
            )),
        }
    }
}

fn string_err(err: StringError) -> RawTokens {
    RawTokens::String(err)
}

#[cfg(test)]
mod tests {}
