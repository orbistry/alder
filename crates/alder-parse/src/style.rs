//! `style { }` blocks (docs/parser-internals.md §6.4).
//!
//! `style` then `{` (`Style::Open`); entries `key: value` with an optional
//! `,` after each (§6.4, SPEC's `[ ',' ]`). Like match arms (§2.1 rule 3
//! exempts comma-separated members from the line-break rule), a
//! comma-less entry may follow the previous value on the next line
//! (`style_newline_separated`) or on the same line
//! (`style_no_comma_same_line`): after a value the next byte must be `,`,
//! `}`, or the start of a key (`"` or a lowercase letter), else
//! `Style::End`. A key is a `lower_name` (`padding`) or a string
//! (`":hover"`, `"@media (max-width: 600px)"`). A value is:
//!
//! - `{` → a nested `style_block` (never a record; §10.27);
//! - a digit, or a `-` immediately followed by a digit → a dimension
//!   attempt via `chomp_number` (the `-` negates `value` and stays in
//!   `text`, so `margin: -8px` is `Dimension { -8 "-8", "px" }`). A run of
//!   ASCII letters or `%` right after the number is the unit. A space
//!   before a run of letters (`16 px`) is `Style::Dimension(Number::End)`
//!   at the space, where the unit had to start; a spaced `%` is not a
//!   unit but the `%` operator (`width: 10 % 3` is `BinOps`), so only
//!   letters take part in that check. A number with no unit restores the
//!   saved state and falls through;
//! - otherwise `expression()` — `opacity: 1` is `Expr::Number`, `margin: -x`
//!   is `Negate`, `color: theme.text` is an access.
//!
//! `chomp_number` takes an `e` only when digits follow, so `a: 1e` is
//! `Dimension { 1, "e" }` rather than `Number::Exponent`; that is what
//! makes `1em` / `1ex` work and is accepted as the literal reading of
//! §6.4 (`style_dangling_exponent_is_dimension`).
//!
//! Keyword-led like `loop`, `Expr::Style` carries the position of `style`.
//! Values run with record constructors re-enabled (§2.3: braces reset the
//! restriction), so `border: Border::Solid { width: 1 }` works after a
//! `let card = style { … }` inside an `if` head.
//!
//! See docs/parser-internals.md §5.18.
// OWNER: style.rs (Wave 3)

use alder_region::{Located, Position};
use alder_source::{Expr, Style, StyleEntry, StyleKey, StyleValue};
use bumpalo::collections::Vec as BumpVec;

use crate::number::NumberLiteral;
use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// After `style`.
    pub(crate) fn style(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        self.chomp();
        let style = self.specialize(
            |bump, e, _, _| error::Expr::Style(bump.alloc(e), start.line, start.column),
            |p| p.style_block(),
        )?;
        // `style_block` consumes the `}` without chomping, so the region
        // ends right after it (the postfix loop chomps; §5.1).
        Ok(self.add_end(start, Expr::Style(style)))
    }

    /// At `{`. Consumes the closing `}`; does not chomp after it.
    pub(crate) fn style_block(&mut self) -> Result<&'a Style<'a>, error::Style<'a>> {
        self.word1(b'{', error::Style::Open)?;
        self.chomp();
        let mut entries = BumpVec::new_in(self.bump);
        loop {
            if self.peek() == Some(b'}') {
                self.advance();
                break;
            }

            let key_start = self.get_position();
            let key = if self.peek() == Some(b'"') {
                StyleKey::Str(self.string_literal(error::Style::Key, error::Style::KeyString)?)
            } else {
                StyleKey::Ident(self.lower_name(error::Style::Key)?)
            };
            let key = self.located(key_start, key);
            self.chomp();
            self.word1(b':', error::Style::Colon)?;
            self.chomp();
            let value = self.style_value()?;
            entries.push(StyleEntry { key, value });

            // A nested block and a dimension end at their last byte;
            // `expression()` has already chomped. Either way the separator
            // decision needs the whitespace gone.
            self.chomp();
            match self.peek() {
                Some(b',') => {
                    self.advance();
                    self.chomp();
                }
                Some(b'}') => {
                    self.advance();
                    break;
                }
                // The comma is optional: a key may start right here.
                Some(b'"') => {}
                Some(b) if b.is_ascii_lowercase() => {}
                _ => {
                    let (row, col) = self.position();
                    return Err(error::Style::End(row, col));
                }
            }
        }
        Ok(self.alloc(Style {
            entries: entries.into_bump_slice(),
        }))
    }

    /// `{` → nested style; digit, or `-` + digit → dimension attempt; otherwise `expression()`.
    fn style_value(&mut self) -> Result<StyleValue<'a>, error::Style<'a>> {
        match self.peek() {
            Some(b'{') => {
                let nested = self.specialize(
                    |bump, e, row, col| error::Style::Nested(bump.alloc(e), row, col),
                    |p| p.style_block(),
                )?;
                return Ok(StyleValue::Nested(nested));
            }
            Some(b) if b.is_ascii_digit() || (b == b'-' && self.peek_digit_at(1)) => {
                if let Some(dimension) = self.dimension()? {
                    return Ok(dimension);
                }
            }
            _ => {}
        }
        let expr = self.specialize(
            |bump, e, row, col| error::Style::Value(bump.alloc(e), row, col),
            |p| p.with_record_ctor(true, |p| p.expression()),
        )?;
        Ok(StyleValue::Expr(expr))
    }

    /// At a digit or a `-` followed by a digit. `Ok(None)` restores the
    /// cursor: the number carried no unit and the value is an ordinary
    /// expression (a BigInt is never a dimension either).
    fn dimension(&mut self) -> Result<Option<StyleValue<'a>>, error::Style<'a>> {
        let saved = self.save_state();
        let number = match self.chomp_number() {
            Ok(NumberLiteral::Number(number)) => number,
            Ok(NumberLiteral::BigInt(_)) => {
                self.restore_state(saved);
                return Ok(None);
            }
            Err(problem) => {
                // `chomp_number` leaves the cursor on the offending byte.
                let (row, col) = self.position();
                return Err(error::Style::Dimension(problem, row, col));
            }
        };

        let unit_start = self.pos;
        while self.peek().is_some_and(is_unit_byte) {
            self.advance();
        }
        if self.pos > unit_start {
            let unit = self.slice_from(unit_start);
            return Ok(Some(StyleValue::Dimension { number, unit }));
        }

        // `16 px`: a unit separated from its number by spaces on the same
        // line. Report where the unit had to start. Only letters count: a
        // spaced `%` is the rem operator (`10 % 3`), left to `expression()`.
        let mut offset = 0;
        while matches!(self.peek_at(offset), Some(b' ' | b'\t')) {
            offset += 1;
        }
        if offset > 0
            && self
                .peek_at(offset)
                .is_some_and(|b| b.is_ascii_alphabetic())
        {
            let (row, col) = self.position();
            return Err(error::Style::Dimension(error::Number::End, row, col));
        }

        self.restore_state(saved);
        Ok(None)
    }

    /// Is the byte at `offset` an ASCII digit?
    #[inline]
    fn peek_digit_at(&self, offset: usize) -> bool {
        self.peek_at(offset).is_some_and(|b| b.is_ascii_digit())
    }
}

/// A byte of a dimension unit: ASCII letters (`px`, `rem`, `vh`) or `%`.
#[inline]
fn is_unit_byte(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'%'
}

/// Snapshot test macro for successful style parsing.
#[cfg(test)]
macro_rules! assert_style_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let result = parser
            .expression()
            .unwrap_or_else(|e| panic!("expected Ok, got Err: {e:#?}\n\nSource:\n{code}"));
        assert!(
            parser.is_eof(),
            "unconsumed input at {:?}\n\nSource:\n{code}",
            parser.position()
        );
        insta::with_settings!({
            description => code,
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }};
}

/// Snapshot test macro for style parse errors.
#[cfg(test)]
macro_rules! assert_style_error_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let err = parser
            .expression()
            .err()
            .unwrap_or_else(|| panic!("expected Err, got Ok\n\nSource:\n{code}"));
        insta::with_settings!({
            description => code,
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(err);
        });
    }};
}

#[cfg(test)]
mod tests {
    // ---- blocks

    #[test]
    fn style_empty() {
        assert_style_snapshot!("style {}");
    }

    #[test]
    fn style_empty_spaced() {
        assert_style_snapshot!("style { }");
    }

    #[test]
    fn style_newline_before_brace() {
        assert_style_snapshot!(
            r#"
            style
            { padding: 16px }
            "#
        );
    }

    #[test]
    fn style_trailing_comma() {
        assert_style_snapshot!("style { padding: 16px, }");
    }

    #[test]
    fn style_multiline() {
        assert_style_snapshot!(
            r#"
            style {
                padding: 16px,
                color: theme.text,
            }
            "#
        );
    }

    #[test]
    fn style_newline_separated() {
        assert_style_snapshot!(
            r#"
            style {
                padding: 16px
                color: red
            }
            "#
        );
    }

    #[test]
    fn style_newline_separated_expr_value() {
        assert_style_snapshot!(
            r#"
            style {
                color: theme.text
                padding: 8px
            }
            "#
        );
    }

    #[test]
    fn style_newline_separated_nested() {
        assert_style_snapshot!(
            r#"
            style {
                ":hover": { color: red }
                "@media (max-width: 600px)": { padding: 8px }
            }
            "#
        );
    }

    #[test]
    fn style_no_comma_same_line() {
        assert_style_snapshot!("style { padding: 16px color: red }");
    }

    #[test]
    fn style_with_comments() {
        assert_style_snapshot!(
            r#"
            style {
                // outer spacing
                padding: 16px, // trailing
                color: red,
            }
            "#
        );
    }

    // ---- dimensions

    #[test]
    fn style_dimension() {
        assert_style_snapshot!("style { padding: 16px }");
    }

    #[test]
    fn style_percent() {
        assert_style_snapshot!("style { width: 100% }");
    }

    #[test]
    fn style_float_dimension() {
        assert_style_snapshot!("style { lineHeight: 1.5rem }");
    }

    #[test]
    fn style_em_dimension() {
        assert_style_snapshot!("style { fontSize: 1em }");
    }

    #[test]
    fn style_exponent_dimension() {
        assert_style_snapshot!("style { width: 1e3px }");
    }

    #[test]
    fn style_negative_dimension() {
        assert_style_snapshot!("style { margin: -8px }");
    }

    #[test]
    fn style_negative_float_dimension() {
        assert_style_snapshot!("style { margin: -0.5rem }");
    }

    #[test]
    fn style_zero_dimension() {
        assert_style_snapshot!("style { margin: 0px }");
    }

    #[test]
    fn style_dangling_exponent_is_dimension() {
        assert_style_snapshot!("style { a: 1e }");
    }

    #[test]
    fn style_exponent_unitless() {
        assert_style_snapshot!("style { a: 1e5 }");
    }

    // ---- expression values

    #[test]
    fn style_unitless_number() {
        assert_style_snapshot!("style { opacity: 1 }");
    }

    #[test]
    fn style_unitless_float() {
        assert_style_snapshot!("style { opacity: 0.5 }");
    }

    #[test]
    fn style_unitless_negative_number() {
        assert_style_snapshot!("style { zIndex: -1 }");
    }

    #[test]
    fn style_bigint_is_expr() {
        assert_style_snapshot!("style { zIndex: 1n }");
    }

    #[test]
    fn style_negative_expr() {
        assert_style_snapshot!("style { margin: -x }");
    }

    #[test]
    fn style_expr_value() {
        assert_style_snapshot!("style { color: theme.text }");
    }

    #[test]
    fn style_string_value() {
        assert_style_snapshot!(r#"style { display: "flex" }"#);
    }

    #[test]
    fn style_call_value() {
        assert_style_snapshot!("style { color: rgb(0, 0, 0) }");
    }

    #[test]
    fn style_binop_value() {
        assert_style_snapshot!("style { width: base * 2 }");
    }

    #[test]
    fn style_rem_operator() {
        assert_style_snapshot!("style { width: 10 % 3 }");
    }

    #[test]
    fn style_rem_operator_tight_right() {
        assert_style_snapshot!("style { width: 10 %3 }");
    }

    #[test]
    fn style_record_ctor_value() {
        assert_style_snapshot!("style { border: Border::Solid { width: 1 } }");
    }

    // ---- nested blocks

    #[test]
    fn style_string_key_nested() {
        assert_style_snapshot!(r#"style { ":hover": { color: theme.accent } }"#);
    }

    #[test]
    fn style_media_nested() {
        assert_style_snapshot!(r#"style { "@media (max-width: 600px)": { padding: 8px } }"#);
    }

    #[test]
    fn style_ident_key_nested() {
        assert_style_snapshot!("style { hover: { color: red } }");
    }

    #[test]
    fn style_nested_empty() {
        assert_style_snapshot!(r#"style { ":hover": {} }"#);
    }

    #[test]
    fn style_nested_deep() {
        assert_style_snapshot!(
            r#"
            style {
                "@media (max-width: 600px)": {
                    ":hover": { padding: 4px },
                },
            }
            "#
        );
    }

    #[test]
    fn style_docs_card() {
        assert_style_snapshot!(
            r#"
            style {
                padding: 16px,
                color: theme.text,
                ":hover": { color: theme.accent },
                "@media (max-width: 600px)": { padding: 8px },
            }
            "#
        );
    }

    // ---- errors

    #[test]
    fn error_open() {
        assert_style_error_snapshot!("style card");
    }

    #[test]
    fn error_open_eof() {
        assert_style_error_snapshot!("style");
    }

    #[test]
    fn error_key_number() {
        assert_style_error_snapshot!("style { 16: px }");
    }

    #[test]
    fn error_key_uppercase() {
        assert_style_error_snapshot!("style { Padding: 16px }");
    }

    #[test]
    fn error_key_reserved() {
        assert_style_error_snapshot!("style { type: 1 }");
    }

    #[test]
    fn error_key_string_endless() {
        assert_style_error_snapshot!(r#"style { ":hover: { color: red } }"#);
    }

    #[test]
    fn error_missing_colon() {
        assert_style_error_snapshot!("style { padding 16px }");
    }

    #[test]
    fn error_equals_not_colon() {
        assert_style_error_snapshot!("style { padding = 16px }");
    }

    #[test]
    fn error_missing_value() {
        assert_style_error_snapshot!("style { padding: }");
    }

    #[test]
    fn error_value_reserved() {
        assert_style_error_snapshot!("style { padding: else }");
    }

    #[test]
    fn error_unit_space() {
        assert_style_error_snapshot!("style { padding: 16 px }");
    }

    #[test]
    fn error_dimension_leading_zero() {
        assert_style_error_snapshot!("style { padding: 007px }");
    }

    #[test]
    fn error_dimension_trailing_dot() {
        assert_style_error_snapshot!("style { padding: 1.px }");
    }

    #[test]
    fn error_negative_dimension_bad_number() {
        assert_style_error_snapshot!("style { margin: -0x }");
    }

    #[test]
    fn error_entry_separator() {
        assert_style_error_snapshot!("style { padding: 16px 8px }");
    }

    #[test]
    fn error_semicolon_separator() {
        assert_style_error_snapshot!("style { padding: 16px; color: red }");
    }

    #[test]
    fn error_newline_non_key() {
        assert_style_error_snapshot!(
            r#"
            style {
                padding: 16px
                (x)
            }
            "#
        );
    }

    #[test]
    fn error_double_comma() {
        assert_style_error_snapshot!("style { padding: 16px,, color: red }");
    }

    #[test]
    fn error_unclosed() {
        assert_style_error_snapshot!("style { padding: 16px");
    }

    #[test]
    fn error_unclosed_empty() {
        assert_style_error_snapshot!("style {");
    }

    #[test]
    fn error_nested_unclosed() {
        assert_style_error_snapshot!(r#"style { ":hover": { color: red }"#);
    }

    #[test]
    fn error_nested_missing_colon() {
        assert_style_error_snapshot!(r#"style { ":hover": { color red } }"#);
    }
}
