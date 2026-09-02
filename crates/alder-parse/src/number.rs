//! Number literal scanning for Alder (JS semantics: decimal, `0x` hex,
//! fraction, exponent, `n` BigInt suffix).
//!
//! See docs/parser-internals.md §2 and §5.5.
//!
//! Shape: `digits [ '.' digits ] [ ('e'|'E') ['+'|'-'] digits ] [ 'n' ]` or
//! `'0' ('x'|'X') hexdigits [ 'n' ]`. A leading `0` may only be followed by
//! `.`, `e`, `x` or `n` (`007` → `NoLeadingZero`). An `e` is an exponent only
//! when digits (optionally signed) follow it; otherwise the number ends before
//! the `e` so that `chomp_number` leaves `em` as the unit of `1em`, while
//! `number_literal` turns that dangling `e` into `Exponent`.
//!
//! Every error leaves the cursor on the offending byte, and the returned
//! position is that byte: `007` → col 2, `1.` → col 2 (the dot), `1e` → col 3,
//! `0x` → col 3, `123abc` → col 4, `1.5n` → col 4 (the `n`).
//!
//! Only `chomp_number` accepts a leading `-` (§6.4: `margin: -8px` is
//! `Dimension { -8 "-8", "px" }`); `number_literal` leaves `-` to the unary
//! operator, and a float followed by `.digit` (`1.5.5`) ends before the second
//! dot, which the postfix layer then reads as a tuple index (§10.10).
// OWNER: number.rs (Wave 1)

use alder_region::Located;
use alder_source::NumberLit;

use crate::error;
use crate::keyword::is_ident_byte;
use crate::{Col, Parser, Row};

// The scanners have no callers until Wave 2 (`expression/literal.rs`,
// `expression/postfix.rs`, `pattern/`) and Wave 3 (`style.rs`) land; the
// `allow` goes away in Wave 4 (docs/parser-internals.md §9 step 4.2).
#[allow(unused)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum NumberLiteral<'a> {
    Number(NumberLit<'a>),
    /// Source text without the trailing `n` (`0xFF` for `0xFFn`).
    BigInt(&'a str),
}

#[allow(unused)]
impl<'a> Parser<'a> {
    /// Digit-led literal with Elm's dirty-end check (`123abc` → Number::End).
    ///
    /// `to_expectation` fires without consuming when the cursor is not on a
    /// digit; `to_error` fires at the offending byte once a digit has been
    /// consumed (committed failure, as in Elm's `Number.number`).
    pub(crate) fn number_literal<E>(
        &mut self,
        to_expectation: impl FnOnce(Row, Col) -> E,
        to_error: impl FnOnce(error::Number, Row, Col) -> E,
    ) -> Result<NumberLiteral<'a>, E> {
        let (row, col) = self.position();
        if !self.peek_digit() {
            return Err(to_expectation(row, col));
        }

        let literal = match self.chomp_number() {
            Ok(literal) => literal,
            Err(problem) => {
                let (row, col) = self.position();
                return Err(to_error(problem, row, col));
            }
        };

        match self.peek() {
            // `1e`, `1e+`, `1ex`: a decimal number ending right before an `e`
            // is a malformed exponent. Report after the `e`, where Elm does.
            Some(b'e' | b'E') if matches!(literal, NumberLiteral::Number(_)) => {
                self.advance();
                let (row, col) = self.position();
                Err(to_error(error::Number::Exponent, row, col))
            }
            Some(b) if is_ident_byte(b) => {
                let (row, col) = self.position();
                Err(to_error(error::Number::End, row, col))
            }
            _ => Ok(literal),
        }
    }

    /// Committed numeric prefix without the dirty-end check (style dimensions read the unit after it).
    ///
    /// The cursor must be on a digit, or on a `-` immediately followed by a
    /// digit: the `-` is consumed, negates `value` and stays in `text`
    /// (`-8px` → `-8` / `"-8"`, cursor on `p`), which is the whole of the
    /// §6.4 negative-dimension recipe — the style owner calls this at the
    /// `-`. Stops before any byte that cannot continue the literal (`16px` →
    /// `16`, cursor on `p`; `1em` → `1`, cursor on `e`). Malformed shapes
    /// still fail: `007`, `0x`, `1.`, `1.5n`.
    pub(crate) fn chomp_number(&mut self) -> Result<NumberLiteral<'a>, error::Number> {
        let start = self.pos;
        if self.peek() == Some(b'-') && self.peek_at(1).is_some_and(|b| b.is_ascii_digit()) {
            self.advance();
        }
        debug_assert!(self.peek_digit(), "chomp_number called off a digit");

        if self.peek() == Some(b'0') {
            self.advance();
            match self.peek() {
                Some(b'x' | b'X') => {
                    self.advance();
                    return self.chomp_hex(start);
                }
                Some(b) if b.is_ascii_digit() => return Err(error::Number::NoLeadingZero),
                _ => {}
            }
        } else {
            self.chomp_digits();
        }

        let mut is_integer = true;

        if self.peek() == Some(b'.') {
            if !self.peek_at(1).is_some_and(|b| b.is_ascii_digit()) {
                return Err(error::Number::Dot);
            }
            self.advance();
            self.chomp_digits();
            is_integer = false;
        }

        if let Some(b'e' | b'E') = self.peek() {
            let digits_at = match self.peek_at(1) {
                Some(b'+' | b'-') => 2,
                _ => 1,
            };
            if self.peek_at(digits_at).is_some_and(|b| b.is_ascii_digit()) {
                self.advance_by(digits_at);
                self.chomp_digits();
                is_integer = false;
            }
        }

        let text = self.slice_from(start);

        if self.peek() == Some(b'n') {
            if !is_integer {
                return Err(error::Number::BigIntFraction);
            }
            self.advance();
            return Ok(NumberLiteral::BigInt(text));
        }

        let value = text
            .parse::<f64>()
            .expect("scanner validated the decimal literal");
        Ok(NumberLiteral::Number(NumberLit { value, text }))
    }

    /// Bare digit run for tuple indices (`t.0`). None without consuming if no digit.
    ///
    /// Saturates at `u32::MAX` rather than failing: the run is still consumed.
    pub(crate) fn digits(&mut self) -> Option<Located<u32>> {
        if !self.peek_digit() {
            return None;
        }
        let start = self.get_position();
        let mut n: u32 = 0;
        while let Some(b) = self.peek().filter(u8::is_ascii_digit) {
            n = n.saturating_mul(10).saturating_add(u32::from(b - b'0'));
            self.advance();
        }
        Some(self.located(start, n))
    }

    /// Hex digits after `0x`; `start` is the byte offset of the literal's
    /// first byte (the leading `0`, or the `-` before it).
    fn chomp_hex(&mut self, start: usize) -> Result<NumberLiteral<'a>, error::Number> {
        let digits_start = self.pos;
        // Exact while it fits; beyond 128 bits fall back to rounding per digit.
        let mut exact: Option<u128> = Some(0);
        let mut value = 0f64;
        while let Some(b) = self.peek().filter(u8::is_ascii_hexdigit) {
            let digit = hex_value(b);
            exact = exact
                .and_then(|n| n.checked_mul(16))
                .and_then(|n| n.checked_add(u128::from(digit)));
            value = value * 16.0 + f64::from(digit);
            self.advance();
        }
        if self.pos == digits_start {
            return Err(error::Number::HexDigit);
        }
        let text = self.slice_from(start);
        if self.peek() == Some(b'n') {
            self.advance();
            return Ok(NumberLiteral::BigInt(text));
        }
        let magnitude = exact.map_or(value, |n| n as f64);
        // `text` starts at the `-` when `chomp_number` consumed one.
        let value = if text.starts_with('-') {
            -magnitude
        } else {
            magnitude
        };
        Ok(NumberLiteral::Number(NumberLit { value, text }))
    }

    /// Consume `[0-9]*`.
    fn chomp_digits(&mut self) {
        while self.peek_digit() {
            self.advance();
        }
    }

    /// Is the cursor on an ASCII digit?
    #[inline]
    fn peek_digit(&self) -> bool {
        self.peek().is_some_and(|b| b.is_ascii_digit())
    }
}

/// Value of an ASCII hex digit.
#[inline]
fn hex_value(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => unreachable!("caller checked is_ascii_hexdigit"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::Bump;

    /// Comparable shape of a scanned literal.
    #[derive(Debug, PartialEq)]
    enum Lit {
        Num(f64, String),
        Big(String),
    }

    impl From<NumberLiteral<'_>> for Lit {
        fn from(literal: NumberLiteral<'_>) -> Self {
            match literal {
                NumberLiteral::Number(n) => Lit::Num(n.value, n.text.to_owned()),
                NumberLiteral::BigInt(text) => Lit::Big(text.to_owned()),
            }
        }
    }

    fn with_parser<T>(src: &str, f: impl FnOnce(&mut Parser<'_>) -> T) -> (T, (u16, u16)) {
        let bump = Bump::new();
        let text = bump.alloc_str(src);
        let mut parser = Parser::new(&bump, text.as_bytes());
        let result = f(&mut parser);
        (result, parser.position())
    }

    /// `number_literal` outcome plus the cursor position afterwards.
    /// Errors render as `"<problem> <row>:<col>"`, expectations as `"expect <row>:<col>"`.
    fn literal(src: &str) -> (Result<Lit, String>, (u16, u16)) {
        with_parser(src, |p| {
            p.number_literal(
                |r, c| format!("expect {r}:{c}"),
                |e, r, c| format!("{e:?} {r}:{c}"),
            )
            .map(Lit::from)
        })
    }

    /// `chomp_number` outcome plus the cursor position afterwards.
    fn chomp(src: &str) -> (Result<Lit, String>, (u16, u16)) {
        with_parser(src, |p| {
            p.chomp_number()
                .map(Lit::from)
                .map_err(|e| format!("{e:?}"))
        })
    }

    fn num(value: f64, text: &str) -> Result<Lit, String> {
        Ok(Lit::Num(value, text.to_owned()))
    }

    fn big(text: &str) -> Result<Lit, String> {
        Ok(Lit::Big(text.to_owned()))
    }

    #[test]
    fn int_simple() {
        assert_eq!(literal("42"), (num(42.0, "42"), (1, 3)));
        assert_eq!(literal("123 + x"), (num(123.0, "123"), (1, 4)));
        assert_eq!(literal("7)"), (num(7.0, "7"), (1, 2)));
    }

    #[test]
    fn int_zero() {
        assert_eq!(literal("0"), (num(0.0, "0"), (1, 2)));
        assert_eq!(literal("0,"), (num(0.0, "0"), (1, 2)));
    }

    #[test]
    fn int_hex() {
        assert_eq!(literal("0xFF"), (num(255.0, "0xFF"), (1, 5)));
        assert_eq!(literal("0x1a2B]"), (num(6699.0, "0x1a2B"), (1, 7)));
        assert_eq!(literal("0X10"), (num(16.0, "0X10"), (1, 5)));
        assert_eq!(literal("0x0"), (num(0.0, "0x0"), (1, 4)));
    }

    #[test]
    fn int_hex_large_is_rounded_like_js() {
        // 2^64 - 1 does not fit a double: JS gives 18446744073709552000.
        assert_eq!(
            literal("0xFFFFFFFFFFFFFFFF"),
            (
                num(18446744073709551615u128 as f64, "0xFFFFFFFFFFFFFFFF"),
                (1, 19)
            )
        );
    }

    #[test]
    fn int_hex_beyond_u128_rounds_per_digit() {
        // 40 hex digits overflow the exact u128 path; 16^40 - 1 rounds to 2^160.
        let src = "0x".to_owned() + &"F".repeat(40);
        assert_eq!(literal(&src), (num(2f64.powi(160), &src), (1, 43)));
    }

    #[test]
    fn float_simple() {
        assert_eq!(literal("1.5"), (num(1.5, "1.5"), (1, 4)));
        assert_eq!(literal("0.25;"), (num(0.25, "0.25"), (1, 5)));
        assert_eq!(literal("10.00"), (num(10.0, "10.00"), (1, 6)));
    }

    #[test]
    fn float_stops_before_second_dot() {
        // `1.5.5` / `1e5.5` end at the second dot; the postfix layer decides
        // what `.5` means (§10.10), the scanner does not.
        assert_eq!(literal("1.5.5"), (num(1.5, "1.5"), (1, 4)));
        assert_eq!(literal("1e5.5"), (num(100000.0, "1e5"), (1, 4)));
        assert_eq!(literal("1..2"), (Err("Dot 1:2".into()), (1, 2)));
    }

    #[test]
    fn float_exponent() {
        assert_eq!(literal("1e3"), (num(1000.0, "1e3"), (1, 4)));
        assert_eq!(literal("2.5E2 "), (num(250.0, "2.5E2"), (1, 6)));
        assert_eq!(literal("0e0"), (num(0.0, "0e0"), (1, 4)));
    }

    #[test]
    fn float_exponent_sign() {
        assert_eq!(literal("1e+3"), (num(1000.0, "1e+3"), (1, 5)));
        assert_eq!(literal("1e-3"), (num(0.001, "1e-3"), (1, 5)));
        assert_eq!(literal("1.5e-1)"), (num(0.15, "1.5e-1"), (1, 7)));
    }

    #[test]
    fn bigint() {
        assert_eq!(literal("123n"), (big("123"), (1, 5)));
        assert_eq!(literal("0n"), (big("0"), (1, 3)));
        assert_eq!(
            literal("9007199254740993n)"),
            (big("9007199254740993"), (1, 18))
        );
    }

    #[test]
    fn bigint_hex() {
        assert_eq!(literal("0xFFn"), (big("0xFF"), (1, 6)));
        assert_eq!(literal("0x1en,"), (big("0x1e"), (1, 6)));
    }

    #[test]
    fn digits_run() {
        let (index, pos) = with_parser("0.1", |p| p.digits());
        let index = index.expect("a digit run");
        assert_eq!(index.value, 0);
        assert_eq!((index.region.start.column, index.region.end.column), (1, 2));
        assert_eq!(pos, (1, 2));

        let (index, pos) = with_parser("12x", |p| p.digits());
        assert_eq!(index.map(|i| i.value), Some(12));
        assert_eq!(pos, (1, 3));

        // Bare run: leading zeros and trailing letters are not its business.
        let (index, pos) = with_parser("007abc", |p| p.digits());
        assert_eq!(index.map(|i| i.value), Some(7));
        assert_eq!(pos, (1, 4));
    }

    #[test]
    fn digits_none_without_consuming() {
        let (index, pos) = with_parser("x", |p| p.digits());
        assert!(index.is_none());
        assert_eq!(pos, (1, 1));

        let (index, pos) = with_parser("", |p| p.digits());
        assert!(index.is_none());
        assert_eq!(pos, (1, 1));
    }

    #[test]
    fn digits_saturate_on_overflow() {
        let (index, pos) = with_parser("99999999999", |p| p.digits());
        assert_eq!(index.map(|i| i.value), Some(u32::MAX));
        assert_eq!(pos, (1, 12));
    }

    #[test]
    fn expectation_without_consuming() {
        assert_eq!(literal("x"), (Err("expect 1:1".into()), (1, 1)));
        assert_eq!(literal(".5"), (Err("expect 1:1".into()), (1, 1)));
        assert_eq!(literal("-1"), (Err("expect 1:1".into()), (1, 1)));
        assert_eq!(literal(""), (Err("expect 1:1".into()), (1, 1)));
    }

    #[test]
    fn error_leading_zero() {
        assert_eq!(literal("007"), (Err("NoLeadingZero 1:2".into()), (1, 2)));
        assert_eq!(literal("00"), (Err("NoLeadingZero 1:2".into()), (1, 2)));
        assert_eq!(literal("01.5"), (Err("NoLeadingZero 1:2".into()), (1, 2)));
    }

    #[test]
    fn error_hex_no_digits() {
        assert_eq!(literal("0x"), (Err("HexDigit 1:3".into()), (1, 3)));
        assert_eq!(literal("0xG"), (Err("HexDigit 1:3".into()), (1, 3)));
        assert_eq!(literal("0x n"), (Err("HexDigit 1:3".into()), (1, 3)));
    }

    #[test]
    fn error_trailing_dot() {
        assert_eq!(literal("1."), (Err("Dot 1:2".into()), (1, 2)));
        assert_eq!(literal("1.x"), (Err("Dot 1:2".into()), (1, 2)));
        assert_eq!(literal("0."), (Err("Dot 1:2".into()), (1, 2)));
        assert_eq!(literal("12.e5"), (Err("Dot 1:3".into()), (1, 3)));
    }

    #[test]
    fn error_bad_exponent() {
        assert_eq!(literal("1e"), (Err("Exponent 1:3".into()), (1, 3)));
        assert_eq!(literal("1e+"), (Err("Exponent 1:3".into()), (1, 3)));
        assert_eq!(literal("1ex"), (Err("Exponent 1:3".into()), (1, 3)));
        assert_eq!(literal("1.5e"), (Err("Exponent 1:5".into()), (1, 5)));
        assert_eq!(literal("2E-x"), (Err("Exponent 1:3".into()), (1, 3)));
    }

    #[test]
    fn error_dirty_end() {
        assert_eq!(literal("123abc"), (Err("End 1:4".into()), (1, 4)));
        assert_eq!(literal("0b1"), (Err("End 1:2".into()), (1, 2)));
        assert_eq!(literal("1_000"), (Err("End 1:2".into()), (1, 2)));
        assert_eq!(literal("1.5px"), (Err("End 1:4".into()), (1, 4)));
        assert_eq!(literal("0xFFg"), (Err("End 1:5".into()), (1, 5)));
        assert_eq!(literal("12nx"), (Err("End 1:4".into()), (1, 4)));
    }

    #[test]
    fn error_bigint_fraction() {
        assert_eq!(literal("1.5n"), (Err("BigIntFraction 1:4".into()), (1, 4)));
        assert_eq!(literal("1e5n"), (Err("BigIntFraction 1:4".into()), (1, 4)));
    }

    #[test]
    fn chomp_number_stops_before_unit() {
        assert_eq!(chomp("16px"), (num(16.0, "16"), (1, 3)));
        assert_eq!(chomp("1.5rem"), (num(1.5, "1.5"), (1, 4)));
        assert_eq!(chomp("100%"), (num(100.0, "100"), (1, 4)));
        // `e` is only an exponent when digits follow it.
        assert_eq!(chomp("1em"), (num(1.0, "1"), (1, 2)));
        assert_eq!(chomp("2e3em"), (num(2000.0, "2e3"), (1, 4)));
        assert_eq!(chomp("0xFFn;"), (big("0xFF"), (1, 6)));
    }

    #[test]
    fn chomp_number_negative_dimension() {
        // §6.4: `margin: -8px` — the `-` negates `value` and stays in `text`.
        assert_eq!(chomp("-8px"), (num(-8.0, "-8"), (1, 3)));
        assert_eq!(chomp("-1.5rem"), (num(-1.5, "-1.5"), (1, 5)));
        assert_eq!(chomp("-0.5em"), (num(-0.5, "-0.5"), (1, 5)));
        assert_eq!(chomp("-2e1%"), (num(-20.0, "-2e1"), (1, 5)));
        assert_eq!(chomp("-0x10"), (num(-16.0, "-0x10"), (1, 6)));
        // `-0.0 == 0.0`, so check the sign bit explicitly.
        let (zero, pos) = with_parser("-0", |p| match p.chomp_number() {
            Ok(NumberLiteral::Number(n)) => (n.value.is_sign_negative(), n.text.to_owned()),
            other => panic!("expected a number, got {other:?}"),
        });
        assert_eq!(zero, (true, "-0".to_owned()));
        assert_eq!(pos, (1, 3));
        assert_eq!(chomp("-8n"), (big("-8"), (1, 4)));
    }

    #[test]
    fn chomp_number_negative_errors_at_offending_byte() {
        assert_eq!(chomp("-007"), (Err("NoLeadingZero".into()), (1, 3)));
        assert_eq!(chomp("-0x"), (Err("HexDigit".into()), (1, 4)));
        assert_eq!(chomp("-1."), (Err("Dot".into()), (1, 3)));
        assert_eq!(chomp("-1.5n"), (Err("BigIntFraction".into()), (1, 5)));
    }

    #[test]
    fn chomp_number_still_rejects_malformed() {
        assert_eq!(chomp("007px"), (Err("NoLeadingZero".into()), (1, 2)));
        assert_eq!(chomp("0xpx"), (Err("HexDigit".into()), (1, 3)));
        assert_eq!(chomp("1.px"), (Err("Dot".into()), (1, 2)));
        assert_eq!(chomp("1.5n"), (Err("BigIntFraction".into()), (1, 4)));
    }
}
