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
mod tests {
    use super::*;
    use alder_region::Position;
    use bumpalo::Bump;

    /// Interior text with its region, or the error rendered as text with its
    /// position; plus where the cursor stopped.
    type Outcome = (Result<(String, Region), (String, u16, u16)>, Position);

    type Region = ((u16, u16), (u16, u16));

    fn raw(src: &str, open: u8, close: u8) -> Outcome {
        let bump = Bump::new();
        let text = bump.alloc_str(src);
        let mut parser = Parser::new(&bump, text.as_bytes());
        let result = parser
            .raw_balanced(open, close, |err, row, col| (describe(&err), row, col))
            .map(|located| {
                let start = located.region.start;
                let end = located.region.end;
                (
                    located.value.to_owned(),
                    ((start.line, start.column), (end.line, end.column)),
                )
            });
        (result, parser.get_position())
    }

    fn parens(src: &str) -> Outcome {
        raw(src, b'(', b')')
    }

    fn braces(src: &str) -> Outcome {
        raw(src, b'{', b'}')
    }

    fn describe(err: &RawTokens) -> String {
        match err {
            RawTokens::Unbalanced(b) => format!("unbalanced {:?}", *b as char),
            RawTokens::Endless => "endless".to_owned(),
            RawTokens::String(StringError::Endless) => "string endless".to_owned(),
            RawTokens::String(StringError::Newline) => "string newline".to_owned(),
            RawTokens::String(StringError::Escape(escape)) => format!("string escape {escape:?}"),
        }
    }

    fn ok(text: &str, region: Region) -> Result<(String, Region), (String, u16, u16)> {
        Ok((text.to_owned(), region))
    }

    fn err(what: &str, row: u16, col: u16) -> Result<(String, Region), (String, u16, u16)> {
        Err((what.to_owned(), row, col))
    }

    fn at(line: u16, column: u16) -> Position {
        Position::new(line, column)
    }

    // ---- success -----------------------------------------------------------

    #[test]
    fn empty() {
        assert_eq!(parens("()"), (ok("", ((1, 2), (1, 2))), at(1, 3)));
    }

    #[test]
    fn simple() {
        assert_eq!(parens("(a, b)"), (ok("a, b", ((1, 2), (1, 6))), at(1, 7)));
    }

    #[test]
    fn stops_at_close() {
        // Only the balanced span is consumed; whatever follows is left alone.
        assert_eq!(parens("(x) + 1"), (ok("x", ((1, 2), (1, 3))), at(1, 4)));
    }

    #[test]
    fn braces_body() {
        assert_eq!(
            braces("{ x + 1 }"),
            (ok(" x + 1 ", ((1, 2), (1, 9))), at(1, 10))
        );
    }

    #[test]
    fn nested_same_kind() {
        assert_eq!(
            parens("(f(g(x)))"),
            (ok("f(g(x))", ((1, 2), (1, 9))), at(1, 10))
        );
    }

    #[test]
    fn nested_mixed_kinds() {
        assert_eq!(
            braces("{ [a, (b, c)], { d: e } }"),
            (ok(" [a, (b, c)], { d: e } ", ((1, 2), (1, 25))), at(1, 26))
        );
    }

    #[test]
    fn other_closer_is_text_at_depth_zero() {
        // `]` and `}` are only unbalanced when something is open; the outer
        // `close` byte itself is what we are looking for.
        assert_eq!(parens("(a)]"), (ok("a", ((1, 2), (1, 3))), at(1, 4)));
    }

    #[test]
    fn multiline() {
        assert_eq!(
            braces("{\n    let l = left\n    l\n}"),
            (
                ok("\n    let l = left\n    l\n", ((1, 2), (4, 1))),
                at(4, 2)
            )
        );
    }

    #[test]
    fn multibyte_text() {
        // Columns count bytes, like every other scanner.
        assert_eq!(parens("(héllo)"), (ok("héllo", ((1, 2), (1, 8))), at(1, 9)));
    }

    #[test]
    fn string_hides_closer() {
        assert_eq!(
            parens(r#"(")")"#),
            (ok(r#"")""#, ((1, 2), (1, 5))), at(1, 6))
        );
    }

    #[test]
    fn string_hides_opener() {
        assert_eq!(
            parens(r#"("(")"#),
            (ok(r#""(""#, ((1, 2), (1, 5))), at(1, 6))
        );
    }

    #[test]
    #[ignore = "waits for string.rs"]
    fn string_escaped_quote() {
        assert_eq!(
            parens(r#"("a\")b")"#),
            (ok(r#""a\")b""#, ((1, 2), (1, 9))), at(1, 10))
        );
    }

    #[test]
    #[ignore = "waits for string.rs"]
    fn string_unicode_escape() {
        assert_eq!(
            parens(r#"("\u{1F600}")"#),
            (ok(r#""\u{1F600}""#, ((1, 2), (1, 13))), at(1, 14))
        );
    }

    #[test]
    fn template_hides_closer() {
        assert_eq!(parens("(`)`)"), (ok("`)`", ((1, 2), (1, 5))), at(1, 6)));
    }

    #[test]
    fn template_multiline() {
        assert_eq!(
            parens("(`a\nb`)"),
            (ok("`a\nb`", ((1, 2), (2, 3))), at(2, 4))
        );
    }

    #[test]
    fn template_hole_is_code() {
        // The hole's `}` closes the hole, not the outer body; brackets and
        // strings inside it are balanced like code.
        assert_eq!(
            braces(r#"{ `x ${ f({ a: ")" }) } y` }"#),
            (
                ok(r#" `x ${ f({ a: ")" }) } y` "#, ((1, 2), (1, 28))),
                at(1, 29)
            )
        );
    }

    #[test]
    fn template_hole_nested_template() {
        assert_eq!(
            parens("(`a ${ `b ${ c }` } d`)"),
            (ok("`a ${ `b ${ c }` } d`", ((1, 2), (1, 23))), at(1, 24))
        );
    }

    #[test]
    fn template_dollar_without_brace() {
        assert_eq!(
            parens("(`$5 }`)"),
            (ok("`$5 }`", ((1, 2), (1, 8))), at(1, 9))
        );
    }

    #[test]
    #[ignore = "waits for string.rs"]
    fn template_escaped_backtick() {
        assert_eq!(
            parens(r"(`a\`b`)"),
            (ok(r"`a\`b`", ((1, 2), (1, 8))), at(1, 9))
        );
    }

    #[test]
    fn comment_hides_closer() {
        assert_eq!(
            braces("{ // ) }\n}"),
            (ok(" // ) }\n", ((1, 2), (2, 1))), at(2, 2))
        );
    }

    #[test]
    fn comment_hides_quote() {
        assert_eq!(
            braces("{ // \"\n}"),
            (ok(" // \"\n", ((1, 2), (2, 1))), at(2, 2))
        );
    }

    #[test]
    fn single_slash_is_text() {
        assert_eq!(parens("(a / b)"), (ok("a / b", ((1, 2), (1, 7))), at(1, 8)));
    }

    // ---- errors ------------------------------------------------------------

    #[test]
    fn error_not_at_open() {
        assert_eq!(parens("x"), (err("endless", 1, 1), at(1, 1)));
        assert_eq!(parens(""), (err("endless", 1, 1), at(1, 1)));
    }

    #[test]
    fn error_endless() {
        assert_eq!(parens("(a, b"), (err("endless", 1, 1), at(1, 6)));
    }

    #[test]
    fn error_endless_nested() {
        // Reported at the opener `raw_balanced` was asked to balance.
        assert_eq!(braces("{\n  [a"), (err("endless", 1, 1), at(2, 5)));
    }

    #[test]
    fn error_endless_in_hole() {
        assert_eq!(parens("(`${ a"), (err("endless", 1, 1), at(1, 7)));
    }

    #[test]
    fn error_unbalanced() {
        assert_eq!(parens("(a]"), (err("unbalanced ']'", 1, 3), at(1, 3)));
    }

    #[test]
    fn error_unbalanced_nested() {
        assert_eq!(parens("([a)]"), (err("unbalanced ')'", 1, 4), at(1, 4)));
    }

    #[test]
    fn error_unbalanced_in_hole() {
        assert_eq!(
            parens("(`${ ) }`)"),
            (err("unbalanced ')'", 1, 6), at(1, 6))
        );
    }

    #[test]
    fn error_string_endless() {
        assert_eq!(
            parens(r#"(a, "b)"#),
            (err("string endless", 1, 5), at(1, 8))
        );
    }

    #[test]
    fn error_string_newline() {
        assert_eq!(
            parens("(\"a\nb\")"),
            (err("string newline", 1, 4), at(1, 4))
        );
    }

    #[test]
    #[ignore = "waits for string.rs"]
    fn error_string_bad_escape() {
        assert_eq!(
            parens(r#"("\q")"#),
            (err("string escape Unknown", 1, 3), at(1, 4))
        );
    }

    #[test]
    #[ignore = "waits for string.rs"]
    fn error_string_escape_at_eof() {
        assert_eq!(parens(r#"("\"#), (err("string endless", 1, 2), at(1, 4)));
    }

    #[test]
    fn error_template_endless() {
        assert_eq!(parens("(a, `b)"), (err("string endless", 1, 5), at(1, 8)));
    }
}
