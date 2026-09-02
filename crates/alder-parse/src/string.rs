//! String literal scanning for Alder: single-line `"…"` with escapes
//! `\n \r \t \0 \" \' \\ \u{…}`. Templates reuse the escape scanners.
//!
//! See docs/parser-internals.md §2 and §5.6.
//!
//! Ported from Elm's `Parse/String.hs` (`"""` multi-line strings dropped,
//! §10.11). Error positions follow Elm: `Endless` is reported where the
//! scan stopped (EOF), `Newline` at the newline byte, and every `Escape`
//! at the backslash; the widths carried by `error::Escape` are measured
//! from that backslash (`\u` → 2, `\u{12` → 5, `\u{12}` → 6).
//!
//! Two deliberate deviations from Elm's `eatUnicode`, for the report
//! renderer (Wave 4) to know about:
//!
//! - `\u{}` (no digits) is `BadUnicodeLength { actual: 0, .. }`, not
//!   `BadUnicodeCode`: Elm's `chompHex` returns `-1` for zero digits and its
//!   `code < 0` check fires first, but "needs at least four digits" is the
//!   message a user wants there. Elm's format → code → length order holds
//!   for every other input.
//! - `error::Escape::BadUnicodeLength { code, expected, actual }` (§4,
//!   verbatim) stands in for Elm's `BadUnicodeLength width numDigits
//!   badCode`: `code` is the **width** of the escape from the backslash
//!   (for the underline), `actual` is the digit count, and `expected` is
//!   the nearest bound (4 or 6); the code point itself is not carried.
//!
//! Columns advance one per byte (`Parser::advance`, §5.1), so non-ASCII
//! text before an error shifts the reported column by the extra bytes
//! (`"é\u{41}"` reports the backslash at 1:4, Elm says 1:3). That is a
//! crate-wide Wave 0 convention shared by every scanner, not a string.rs
//! choice.
//!
//! `EscapeResult` is not `Copy`: `error::Escape` (§4, verbatim) is not `Clone`.
// OWNER: string.rs (Wave 1)

use crate::error::{Escape, StringError};
use crate::{Col, Parser, Row};

/// Result of scanning an escape sequence after the backslash.
///
/// The cursor is on the byte after the backslash; widths count from there,
/// so `Normal(1)` covers `n` in `\n` and `Unicode(7)` covers `u{1F600}`.
#[derive(Debug)]
pub(crate) enum EscapeResult {
    /// Normal escape like `\n`; width in bytes.
    Normal(usize),
    /// Unicode escape `\u{…}`; total bytes consumed.
    Unicode(usize),
    /// End of file during escape.
    EndOfFile,
    /// Invalid escape.
    Problem(Escape),
}

impl<'a> Parser<'a> {
    /// `"…"` single-line.
    ///
    /// Fails without consuming (`to_expectation`) when the cursor is not on
    /// a `"`. Otherwise the scan is committed: EOF → `Endless` at the cursor,
    /// a newline → `Newline` at the newline byte, a bad escape →
    /// `Escape(…)` at its backslash. Returns a zero-copy slice of the source
    /// unless an escape forced a cooked copy.
    pub(crate) fn string_literal<E>(
        &mut self,
        to_expectation: impl FnOnce(Row, Col) -> E,
        to_error: impl FnOnce(StringError, Row, Col) -> E,
    ) -> Result<&'a str, E> {
        if self.peek() != Some(b'"') {
            let (row, col) = self.position();
            return Err(to_expectation(row, col));
        }
        self.advance(); // opening "

        let start = self.pos;
        let mut needs_cook = false;

        loop {
            match self.peek() {
                None => {
                    let (row, col) = self.position();
                    return Err(to_error(StringError::Endless, row, col));
                }
                Some(b'\n') => {
                    let (row, col) = self.position();
                    return Err(to_error(StringError::Newline, row, col));
                }
                Some(b'"') => {
                    let end = self.pos;
                    let text = if needs_cook {
                        self.build_escaped_string(start, end, false)
                    } else {
                        self.slice_from(start)
                    };
                    self.advance(); // closing "
                    return Ok(text);
                }
                Some(b'\\') => {
                    needs_cook = true;
                    let (row, col) = self.position();
                    self.advance(); // backslash
                    match self.eat_escape(false) {
                        EscapeResult::Normal(width) | EscapeResult::Unicode(width) => {
                            self.advance_by(width);
                        }
                        EscapeResult::EndOfFile => {
                            let (row, col) = self.position();
                            return Err(to_error(StringError::Endless, row, col));
                        }
                        EscapeResult::Problem(escape) => {
                            return Err(to_error(StringError::Escape(escape), row, col));
                        }
                    }
                }
                Some(b) => {
                    self.advance_by(utf8_char_width(b));
                }
            }
        }
    }

    /// Scan the escape after a backslash (`template` adds `` \` `` and `\$`).
    ///
    /// Does not consume; the cursor must be on the byte after the backslash.
    pub(crate) fn eat_escape(&self, template: bool) -> EscapeResult {
        match self.peek() {
            None => EscapeResult::EndOfFile,
            Some(b'n' | b'r' | b't' | b'0' | b'"' | b'\'' | b'\\') => EscapeResult::Normal(1),
            Some(b'`' | b'$') if template => EscapeResult::Normal(1),
            Some(b'u') => self.eat_unicode(),
            Some(_) => EscapeResult::Problem(Escape::Unknown),
        }
    }

    /// Scan `\u{…}` at `u`.
    ///
    /// Does not consume. Accepts 4 to 6 hex digits naming a Unicode scalar
    /// value (surrogates are refused: a `&str` cannot hold them). The widths
    /// in the `Escape` payloads count from the backslash, as in Elm, and
    /// saturate at `u16::MAX`. `\u{}` is a length error, not a code error
    /// (module docs).
    pub(crate) fn eat_unicode(&self) -> EscapeResult {
        // Cursor is on `u`; the backslash is one byte behind it.
        if self.peek_at(1) != Some(b'{') {
            return EscapeResult::Problem(Escape::BadUnicodeFormat(2));
        }

        let mut offset = 2; // past `u{`
        let mut num_digits: i32 = 0;
        let mut code: u32 = 0;
        while let Some(b) = self.peek_at(offset) {
            if !b.is_ascii_hexdigit() {
                break;
            }
            let digit = (b as char).to_digit(16).unwrap_or(0);
            code = code.saturating_mul(16).saturating_add(digit);
            num_digits += 1;
            offset += 1;
        }
        // `offset` is now the width of `u{digits`; `+ 1` adds the backslash.
        // Saturate rather than wrap on an absurdly long digit run (Elm's
        // `fromIntegral` to `Word16` wraps; the `code` accumulator above
        // already saturates).
        let width = u16::try_from(offset + 1).unwrap_or(u16::MAX);

        if self.peek_at(offset) != Some(b'}') {
            return EscapeResult::Problem(Escape::BadUnicodeFormat(width));
        }
        if char::from_u32(code).is_none() {
            return EscapeResult::Problem(Escape::BadUnicodeCode(width.saturating_add(1)));
        }
        if !(4..=6).contains(&num_digits) {
            // `code` carries the width `\u{digits}` (see the module docs).
            return EscapeResult::Problem(Escape::BadUnicodeLength {
                code: width.saturating_add(1),
                expected: if num_digits < 4 { 4 } else { 6 },
                actual: num_digits,
            });
        }

        // `u{digits}`
        EscapeResult::Unicode(offset + 1)
    }

    /// Cook the escapes in `src[start..end]` into an arena string.
    ///
    /// The range must already have been scanned (every escape valid, both
    /// ends on character boundaries). With `template`, `` \` `` and `\$`
    /// are escapes too and `\r\n` is normalized to `\n`.
    pub(crate) fn build_escaped_string(&self, start: usize, end: usize, template: bool) -> &'a str {
        let src = &self.src[start..end];
        let mut out = String::with_capacity(src.len());
        let mut run_start = 0;
        let mut pos = 0;

        let flush = |out: &mut String, from: usize, to: usize| {
            out.push_str(
                std::str::from_utf8(&src[from..to])
                    .expect("string scanner left a partial character"),
            );
        };

        while pos < src.len() {
            match src[pos] {
                b'\\' => {
                    flush(&mut out, run_start, pos);
                    pos += 1;
                    let Some(&b) = src.get(pos) else {
                        break;
                    };
                    pos += 1;
                    match b {
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'0' => out.push('\0'),
                        b'"' => out.push('"'),
                        b'\'' => out.push('\''),
                        b'\\' => out.push('\\'),
                        b'`' if template => out.push('`'),
                        b'$' if template => out.push('$'),
                        b'u' if src.get(pos) == Some(&b'{') => {
                            pos += 1;
                            let hex_start = pos;
                            while pos < src.len() && src[pos] != b'}' {
                                pos += 1;
                            }
                            let hex = std::str::from_utf8(&src[hex_start..pos])
                                .expect("hex digits are ASCII");
                            let c = u32::from_str_radix(hex, 16)
                                .ok()
                                .and_then(char::from_u32)
                                .unwrap_or(char::REPLACEMENT_CHARACTER);
                            out.push(c);
                            pos += 1; // `}`
                        }
                        // Unreachable after a successful scan; keep the byte.
                        _ => {
                            pos -= 1;
                        }
                    }
                    run_start = pos;
                }
                b'\r' if template && src.get(pos + 1) == Some(&b'\n') => {
                    flush(&mut out, run_start, pos);
                    pos += 1;
                    run_start = pos;
                }
                _ => {
                    pos += 1;
                }
            }
        }
        flush(&mut out, run_start, src.len());

        self.alloc_str(&out)
    }
}

/// Byte width of the UTF-8 character starting with `b`.
///
/// A stray continuation byte counts as 1 so a scanner can never overshoot.
pub(crate) fn utf8_char_width(b: u8) -> usize {
    if b < 0xC0 {
        // ASCII, or a stray continuation byte.
        1
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::Bump;

    /// Parse a string literal; returns the value or `"<error> at row:col"`,
    /// plus the cursor (row, col) afterwards.
    fn string(src: &str) -> (Result<String, String>, (u32, u32)) {
        let bump = Bump::new();
        let text = bump.alloc_str(src);
        let mut parser = Parser::new(&bump, text.as_bytes());
        let result = parser
            .string_literal(
                |r, c| format!("expect at {r}:{c}"),
                |e, r, c| format!("{e:?} at {r}:{c}"),
            )
            .map(str::to_owned);
        (result, parser.position())
    }

    fn escape(src: &str, template: bool) -> String {
        let bump = Bump::new();
        let text = bump.alloc_str(src);
        let parser = Parser::new(&bump, text.as_bytes());
        format!("{:?}", parser.eat_escape(template))
    }

    fn cooked(src: &str, template: bool) -> String {
        let bump = Bump::new();
        let text = bump.alloc_str(src);
        let parser = Parser::new(&bump, text.as_bytes());
        parser
            .build_escaped_string(0, src.len(), template)
            .to_owned()
    }

    // ---- string_literal -----------------------------------------------------

    #[test]
    fn simple() {
        assert_eq!(string(r#""hello" x"#), (Ok("hello".into()), (1, 8)));
    }

    #[test]
    fn empty() {
        assert_eq!(string(r#""""#), (Ok(String::new()), (1, 3)));
    }

    #[test]
    fn with_escape() {
        assert_eq!(string(r#""a\nb""#), (Ok("a\nb".into()), (1, 7)));
    }

    #[test]
    fn all_simple_escapes() {
        assert_eq!(
            string(r#""\n\r\t\0\"\'\\""#),
            (Ok("\n\r\t\0\"'\\".into()), (1, 17))
        );
    }

    #[test]
    fn unicode() {
        assert_eq!(string(r#""\u{1F600}""#), (Ok("😀".into()), (1, 12)));
    }

    #[test]
    fn unicode_four_digits() {
        assert_eq!(string(r#""\u{0041}""#), (Ok("A".into()), (1, 11)));
    }

    #[test]
    fn unicode_amid_text() {
        assert_eq!(string(r#""a\u{00e9}b""#), (Ok("aéb".into()), (1, 13)));
    }

    #[test]
    fn multibyte_text_is_zero_copy() {
        // Columns advance per byte (`é` is two), as `advance_by` does.
        assert_eq!(string(r#""café""#), (Ok("café".into()), (1, 8)));
    }

    #[test]
    fn single_quote_is_plain() {
        assert_eq!(string(r#""it's""#), (Ok("it's".into()), (1, 7)));
    }

    #[test]
    fn template_escapes_are_not_string_escapes() {
        assert_eq!(
            string(r#""\`""#),
            (Err("Escape(Unknown) at 1:2".into()), (1, 3))
        );
        assert_eq!(
            string(r#""\$""#),
            (Err("Escape(Unknown) at 1:2".into()), (1, 3))
        );
    }

    #[test]
    fn error_expectation_without_consuming() {
        assert_eq!(string("x"), (Err("expect at 1:1".into()), (1, 1)));
        assert_eq!(string(""), (Err("expect at 1:1".into()), (1, 1)));
    }

    #[test]
    fn error_endless() {
        assert_eq!(string(r#""hello"#), (Err("Endless at 1:7".into()), (1, 7)));
    }

    #[test]
    fn error_endless_after_backslash() {
        assert_eq!(string(r#""ab\"#), (Err("Endless at 1:5".into()), (1, 5)));
    }

    #[test]
    fn error_newline() {
        assert_eq!(string("\"ab\ncd\""), (Err("Newline at 1:4".into()), (1, 4)));
    }

    #[test]
    fn error_bad_escape() {
        assert_eq!(
            string(r#""ab\qcd""#),
            (Err("Escape(Unknown) at 1:4".into()), (1, 5))
        );
    }

    #[test]
    fn error_bad_unicode_format() {
        // `\u` not followed by `{`: width covers `\u`.
        assert_eq!(
            string(r#""\uA""#),
            (Err("Escape(BadUnicodeFormat(2)) at 1:2".into()), (1, 3))
        );
        // Missing `}`: width covers `\u{00`.
        assert_eq!(
            string(r#""\u{00 41}""#),
            (Err("Escape(BadUnicodeFormat(5)) at 1:2".into()), (1, 3))
        );
    }

    #[test]
    fn error_bad_unicode_code() {
        assert_eq!(
            string(r#""\u{110000}""#),
            (Err("Escape(BadUnicodeCode(10)) at 1:2".into()), (1, 3))
        );
        // Surrogates are not scalar values.
        assert_eq!(
            string(r#""\u{D800}""#),
            (Err("Escape(BadUnicodeCode(8)) at 1:2".into()), (1, 3))
        );
    }

    #[test]
    fn error_bad_unicode_length() {
        assert_eq!(
            string(r#""\u{41}""#),
            (
                Err("Escape(BadUnicodeLength { code: 6, expected: 4, actual: 2 }) at 1:2".into()),
                (1, 3)
            )
        );
        assert_eq!(
            string(r#""\u{0000041}""#),
            (
                Err("Escape(BadUnicodeLength { code: 11, expected: 6, actual: 7 }) at 1:2".into()),
                (1, 3)
            )
        );
    }

    #[test]
    fn error_bad_unicode_empty() {
        // No digits at all: a length error (Elm would say `BadUnicodeCode`).
        assert_eq!(
            string(r#""\u{}""#),
            (
                Err("Escape(BadUnicodeLength { code: 4, expected: 4, actual: 0 }) at 1:2".into()),
                (1, 3)
            )
        );
    }

    #[test]
    fn error_bad_unicode_width_saturates() {
        // A digit run longer than `u16::MAX` saturates the width instead of
        // wrapping; the digit count itself is exact.
        let src = format!("\"\\u{{{}}}\"", "0".repeat(70_000));
        assert_eq!(
            string(&src),
            (
                Err(
                    "Escape(BadUnicodeLength { code: 65535, expected: 6, actual: 70000 }) at 1:2"
                        .into()
                ),
                (1, 3)
            )
        );
    }

    // ---- eat_escape / eat_unicode ------------------------------------------

    #[test]
    fn eat_escape_normal() {
        assert_eq!(escape("n", false), "Normal(1)");
        assert_eq!(escape("0", false), "Normal(1)");
        assert_eq!(escape("\\", false), "Normal(1)");
    }

    #[test]
    fn eat_escape_template_only() {
        assert_eq!(escape("`", true), "Normal(1)");
        assert_eq!(escape("$", true), "Normal(1)");
        assert_eq!(escape("`", false), "Problem(Unknown)");
        assert_eq!(escape("$", false), "Problem(Unknown)");
    }

    #[test]
    fn eat_escape_eof() {
        assert_eq!(escape("", false), "EndOfFile");
    }

    #[test]
    fn eat_escape_unicode_width() {
        // `u{1F600}` is 8 bytes; the caller advances past exactly those.
        assert_eq!(escape("u{1F600}\"", false), "Unicode(8)");
        assert_eq!(escape("u{0041}", false), "Unicode(7)");
    }

    #[test]
    fn eat_unicode_eof_is_format_error() {
        assert_eq!(escape("u", false), "Problem(BadUnicodeFormat(2))");
        assert_eq!(escape("u{00", false), "Problem(BadUnicodeFormat(5))");
    }

    // ---- build_escaped_string ----------------------------------------------

    #[test]
    fn cooked_plain() {
        assert_eq!(cooked("héllo", false), "héllo");
    }

    #[test]
    fn cooked_escapes() {
        assert_eq!(cooked(r"a\tb\u{1F600}c\\", false), "a\tb😀c\\");
    }

    #[test]
    fn cooked_template_escapes() {
        assert_eq!(cooked(r"\`\$", true), "`$");
    }

    #[test]
    fn cooked_template_crlf_normalized() {
        assert_eq!(cooked("a\r\nb\rc", true), "a\nb\rc");
        // Strings never see CRLF; the flag is what enables it.
        assert_eq!(cooked("a\r\nb", false), "a\r\nb");
    }

    // ---- utf8_char_width ---------------------------------------------------

    #[test]
    fn char_widths() {
        assert_eq!(utf8_char_width(b'a'), 1);
        assert_eq!(utf8_char_width("é".as_bytes()[0]), 2);
        assert_eq!(utf8_char_width("€".as_bytes()[0]), 3);
        assert_eq!(utf8_char_width("😀".as_bytes()[0]), 4);
        assert_eq!(utf8_char_width(0x80), 1);
    }
}
