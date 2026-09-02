//! Template literals: `` `…${expr}…` `` (docs/parser-internals.md §6.1).
//!
//! See docs/parser-internals.md §5.7.
//!
//! Template mode is a byte loop: raw text runs until `` ` ``, `\` or `${`.
//! Newlines are allowed inside the text (`\r\n` is normalized to `\n`),
//! the escapes are the string escapes plus `` \` `` and `\$`, and a `$` not
//! followed by `{` is ordinary text. Text parts are zero-copy slices of the
//! source unless an escape or a CRLF forced a cooked copy
//! (`build_escaped_string(…, true)`); empty text runs (a hole at either
//! end, two adjacent holes, an empty template) produce no part at all.
//!
//! Error positions: `Endless` is reported at the **opening** backtick
//! (§6.1, unlike `StringError::Endless`, which sits at EOF), also when EOF
//! follows a backslash; `Escape` at the backslash; `HoleEmpty` at the `}`
//! that closed `${ }` with nothing inside; `HoleExpr` at the start of the
//! hole's expression; `HoleEnd` where the closing `}` was expected. The
//! wrapping `Expr::Template` carries the position of the opening backtick.
// OWNER: template.rs (Wave 2)

use alder_region::{Located, Position};
use alder_source::{Expr, TemplatePart};
use bumpalo::collections::Vec as BumpVec;

use crate::string::{EscapeResult, utf8_char_width};
use crate::{Parser, error};

// No caller until `expression/mod.rs` (`primary`) and `expression/postfix.rs`
// (`tagged_template`) land; the tests below exercise both entry points. The
// `allow` goes away in Wave 4 (docs/parser-internals.md §9 step 4.2).
#[allow(unused)]
impl<'a> Parser<'a> {
    /// At the opening backtick. Used by primary and by tagged templates.
    ///
    /// Consumes through the closing backtick. Does not chomp.
    pub(crate) fn template_parts(&mut self) -> Result<&'a [TemplatePart<'a>], error::Template<'a>> {
        debug_assert_eq!(self.peek(), Some(b'`'), "template_parts: not at a backtick");
        let (open_row, open_col) = self.position();
        self.advance(); // opening `

        let mut parts = BumpVec::new_in(self.bump);
        let mut text_start = self.pos;
        let mut needs_cook = false;

        loop {
            match self.peek() {
                None => return Err(error::Template::Endless(open_row, open_col)),
                Some(b'`') => {
                    self.push_text(&mut parts, text_start, needs_cook);
                    self.advance(); // closing `
                    return Ok(parts.into_bump_slice());
                }
                Some(b'$') if self.peek_at(1) == Some(b'{') => {
                    self.push_text(&mut parts, text_start, needs_cook);
                    self.advance_by(2); // ${
                    let hole = self.template_hole()?;
                    parts.push(TemplatePart::Expr(hole));
                    text_start = self.pos;
                    needs_cook = false;
                }
                Some(b'\\') => {
                    needs_cook = true;
                    let (row, col) = self.position();
                    self.advance(); // backslash
                    match self.eat_escape(true) {
                        EscapeResult::Normal(width) | EscapeResult::Unicode(width) => {
                            self.advance_by(width);
                        }
                        EscapeResult::EndOfFile => {
                            return Err(error::Template::Endless(open_row, open_col));
                        }
                        EscapeResult::Problem(escape) => {
                            return Err(error::Template::Escape(escape, row, col));
                        }
                    }
                }
                Some(b'\r') => {
                    if self.peek_at(1) == Some(b'\n') {
                        needs_cook = true;
                    }
                    self.advance();
                }
                Some(b) => {
                    self.advance_by(utf8_char_width(b));
                }
            }
        }
    }

    /// `Expr::Template` primary.
    ///
    /// `start` is the position of the opening backtick (the cursor is on
    /// it). Does not chomp, like every primary.
    pub(crate) fn template(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let parts = self
            .template_parts()
            .map_err(|e| error::Expr::Template(self.alloc(e), start.line, start.column))?;
        Ok(self.add_end(start, Expr::Template(parts)))
    }

    /// The text run `src[text_start..pos]` as a part, if it is non-empty.
    fn push_text(
        &self,
        parts: &mut BumpVec<'a, TemplatePart<'a>>,
        text_start: usize,
        needs_cook: bool,
    ) {
        if self.pos == text_start {
            return;
        }
        let text = if needs_cook {
            self.build_escaped_string(text_start, self.pos, true)
        } else {
            self.slice_from(text_start)
        };
        parts.push(TemplatePart::Text(text));
    }

    /// After `${`: whitespace, the expression, whitespace, `}`.
    ///
    /// The hole clears `no_record_ctor` like any bracket (§2.3).
    fn template_hole(&mut self) -> Result<&'a Located<Expr<'a>>, error::Template<'a>> {
        self.chomp();
        if self.peek() == Some(b'}') {
            let (row, col) = self.position();
            return Err(error::Template::HoleEmpty(row, col));
        }
        let expr = self.specialize(
            |bump, e, row, col| error::Template::HoleExpr(bump.alloc(e), row, col),
            |p| p.with_record_ctor(true, |p| p.expression()),
        )?;
        self.chomp();
        self.word1(b'}', error::Template::HoleEnd)?;
        Ok(expr)
    }
}

/// Snapshot test macro for successful template parsing.
// TODO(wave4): §7.1 has this pair call `expression()`. Until
// `expression/mod.rs` dispatches `primary` to `template`, that would send
// all 19 tests into `todo!()` and force `#[ignore]` on every one of them,
// so both macros call `template(start)` directly (plus the trailing chomp
// `expression` does) and only the hole tests are ignored. The snapshots
// are byte-identical either way (`template` already wraps errors as
// `Expr::Template(_, row, col)`). Wave 4 step 4.2 (§9) makes the
// mechanical swap in both macros: replace `.template(start)` with
// `.expression()` and delete the `let start = …` and `parser.chomp();`
// lines; no snapshot changes.
#[cfg(test)]
macro_rules! assert_template_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let start = parser.get_position();
        let result = parser
            .template(start)
            .unwrap_or_else(|e| panic!("expected Ok, got Err: {e:#?}\n\nSource:\n{code}"));
        parser.chomp();
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

/// Snapshot test macro for template parse errors.
#[cfg(test)]
macro_rules! assert_template_error_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let start = parser.get_position();
        let err = parser
            .template(start)
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

// Re-exported for submodules (§7.1); template.rs has none, so the `allow`
// stays until Wave 4 step 4.2, as in `type_.rs`.
#[cfg(test)]
#[allow(unused)]
pub(crate) use assert_template_error_snapshot;
#[cfg(test)]
#[allow(unused)]
pub(crate) use assert_template_snapshot;

#[cfg(test)]
mod tests {
    #[test]
    fn empty() {
        assert_template_snapshot!("``");
    }

    #[test]
    fn text_only() {
        assert_template_snapshot!("`hello world`");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn single_hole() {
        assert_template_snapshot!("`Hello ${name}!`");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn hole_at_start() {
        assert_template_snapshot!("`${name} says hi`");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn hole_at_end() {
        assert_template_snapshot!("`/users/${id}`");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn adjacent_holes() {
        assert_template_snapshot!("`${a}${b}`");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn text_around_holes() {
        assert_template_snapshot!("`/users/${id}/posts/${postId}/edit`");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn nested_template() {
        assert_template_snapshot!("`outer ${`inner ${x}`} done`");
    }

    // Snapshot committed and hand-checked (record 1:5-1:13, `x` 1:7-1:8,
    // `1` 1:10-1:11, no empty text parts); un-ignoring must not change it.
    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn record_in_hole() {
        assert_template_snapshot!("`${ { x: 1 } }`");
    }

    #[test]
    fn escaped_backtick() {
        assert_template_snapshot!(r"`a \` b`");
    }

    #[test]
    fn escaped_dollar() {
        assert_template_snapshot!(r"`cost: \${x}`");
    }

    #[test]
    fn dollar_without_brace() {
        assert_template_snapshot!("`cost: $5`");
    }

    #[test]
    fn multiline_text() {
        assert_template_snapshot!(
            r"
            `line one
            line two`
        "
        );
    }

    #[test]
    fn crlf_normalized() {
        assert_template_snapshot!("`a\r\nb`");
    }

    #[test]
    fn error_endless() {
        assert_template_error_snapshot!("`abc");
    }

    #[test]
    fn error_hole_empty() {
        assert_template_error_snapshot!("`a ${ } b`");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn error_hole_unclosed() {
        assert_template_error_snapshot!("`a ${ x `");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn error_hole_bad_expr() {
        assert_template_error_snapshot!("`a ${ ) } b`");
    }

    #[test]
    fn error_bad_escape() {
        assert_template_error_snapshot!(r"`a \q b`");
    }
}
