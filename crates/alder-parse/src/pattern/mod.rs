//! Patterns.
//!
//! `pattern()` is `pattern_atom [as name]`; `pattern_atom` dispatches on the
//! first byte (docs/parser-internals.md §5.14). Constructor, tag, tuple,
//! array and record bodies live in the sibling modules.
//!
//! Conventions: `pattern()` chomps trailing whitespace and computes its
//! region before the chomp; `pattern_atom()` leaves the cursor right after
//! the atom except where a helper had to look past whitespace (`Some (x)`,
//! `Rect { .. }`, `^expr`), in which case the node's region still ends at
//! the atom's last byte.
//!
//! See docs/parser-internals.md §5.14 and §10.17 / §10.20.
// OWNER: pattern/mod.rs (Wave 1)

mod array;
mod ctor;
mod record;
mod tuple;

use alder_region::{Located, Position, Region};
use alder_source::{NumberLit, Pattern};
use bumpalo::collections::Vec as BumpVec;

use crate::number::NumberLiteral;
use crate::{Keyword, Parser, error};

impl<'a> Parser<'a> {
    /// `pattern_atom [as name]`. Chomps trailing whitespace.
    pub fn pattern(&mut self) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        let start = self.get_position();
        let atom = self.pattern_atom()?;
        self.chomp();
        if !self.peek_keyword(b"as") {
            return Ok(atom);
        }
        self.advance_by(2);
        self.chomp();
        let name = self.located_lower(error::Pattern::Alias)?;
        let alias = self.pattern_at(
            start,
            name.region.end,
            Pattern::Alias {
                pattern: atom,
                name,
            },
        );
        self.chomp();
        Ok(alias)
    }

    /// `p | q | r` (match arms). Each alternative is a full `pattern()`, so
    /// the cursor ends after trailing whitespace. A `|` that begins `||` or
    /// `|>` is not an alternative separator.
    // Called by `arm_head` (expression/match_.rs, Wave 2); until then only
    // tests reach it.
    #[allow(unused)]
    pub(crate) fn pattern_alternatives(
        &mut self,
    ) -> Result<&'a [&'a Located<Pattern<'a>>], error::Pattern<'a>> {
        let mut patterns = BumpVec::new_in(self.bump);
        patterns.push(self.pattern()?);
        while self.peek() == Some(b'|') && !matches!(self.peek_at(1), Some(b'|' | b'>')) {
            self.advance();
            self.chomp();
            patterns.push(self.pattern()?);
        }
        Ok(patterns.into_bump_slice())
    }

    /// Dispatch on first byte: `_`, lower, upper (ctor), `:`, `^`, `-`/digit, `"`, `(`, `[`, `{`, true/false.
    pub(crate) fn pattern_atom(&mut self) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        let start = self.get_position();
        let (row, col) = self.position();
        match self.peek() {
            Some(b'_') => self.pattern_wildcard(start),
            Some(b) if b.is_ascii_lowercase() => self.pattern_var(start),
            Some(b) if b.is_ascii_uppercase() => self.pattern_ctor(start),
            Some(b':') => self.pattern_tag(start),
            Some(b'^') => self.pattern_pin(start),
            Some(b'0'..=b'9') => self.pattern_number(start, false),
            Some(b'-') if self.peek_at(1).is_some_and(|b| b.is_ascii_digit()) => {
                self.pattern_number(start, true)
            }
            Some(b'"') => self.pattern_string(start),
            Some(b'(') => self.specialize(
                |bump, e, row, col| error::Pattern::Tuple(bump.alloc(e), row, col),
                |p| p.pattern_tuple(start),
            ),
            Some(b'[') => self.specialize(
                |bump, e, row, col| error::Pattern::Array(bump.alloc(e), row, col),
                |p| p.pattern_array(start),
            ),
            Some(b'{') => {
                let (fields, rest) = self.specialize(
                    |bump, e, row, col| error::Pattern::Record(bump.alloc(e), row, col),
                    |p| {
                        p.advance();
                        p.pattern_record_fields()
                    },
                )?;
                Ok(self.add_end(start, Pattern::Record { fields, rest }))
            }
            _ => Err(error::Pattern::Start(row, col)),
        }
    }

    /// A pattern node whose region ends at `end` rather than at the cursor
    /// (for atoms that had to look past trailing whitespace).
    pub(super) fn pattern_at(
        &self,
        start: Position,
        end: Position,
        value: Pattern<'a>,
    ) -> &'a Located<Pattern<'a>> {
        self.alloc(Located::at(Region::new(start, end), value))
    }

    /// At `_`: wildcard, or `WildcardNotVar` for `_foo` (identifiers start
    /// with a letter; §10.17).
    fn pattern_wildcard(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        let (row, col) = self.position();
        let word = self.peek_word();
        if word.len() > 1 {
            self.advance_by(word.len());
            return Err(error::Pattern::WildcardNotVar(
                word,
                word.len() as i32,
                row,
                col,
            ));
        }
        self.advance();
        Ok(self.add_end(start, Pattern::Anything))
    }

    /// At a lowercase letter: `true` / `false`, a reserved word (error), or a variable.
    fn pattern_var(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        let (row, col) = self.position();
        let word = self.peek_word();
        match word {
            "true" | "false" => {
                self.advance_by(word.len());
                Ok(self.add_end(start, Pattern::Bool(word == "true")))
            }
            _ => {
                if let Some(kw) = Keyword::from_word(word) {
                    return Err(error::Pattern::Reserved(kw, row, col));
                }
                // Only a SQL word inside `query { }` can still be refused here.
                // TODO(wave0): `error::Pattern` has no `SqlKeyword(SqlWord, …)`
                // for parity with `Expr::SqlKeyword` (§6.3); `Start` is the
                // nearest existing variant.
                let name = self.lower_name(error::Pattern::Start)?;
                Ok(self.add_end(start, Pattern::Var(name)))
            }
        }
    }

    /// At `^`: pin a whole postfix chain, parsed with query mode off (§10.20).
    fn pattern_pin(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        self.advance();
        let expr = self.specialize(
            |bump, e, row, col| error::Pattern::Pin(bump.alloc(e), row, col),
            |p| p.with_query(false, |p| p.postfix()),
        )?;
        // `postfix()` chomps trailing whitespace; the pin ends where its operand does.
        Ok(self.pattern_at(start, expr.region.end, Pattern::Pin(expr)))
    }

    /// At a digit, or at `-` immediately followed by a digit. A negative
    /// literal negates the value and keeps the sign in the text (§10.17).
    fn pattern_number(
        &mut self,
        start: Position,
        negative: bool,
    ) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        if negative {
            self.advance();
        }
        let literal = self.number_literal(error::Pattern::Start, error::Pattern::Number)?;
        let pattern = match literal {
            NumberLiteral::Number(lit) if negative => Pattern::Number(NumberLit {
                value: -lit.value,
                text: self.alloc_str(&format!("-{}", lit.text)),
            }),
            NumberLiteral::Number(lit) => Pattern::Number(lit),
            NumberLiteral::BigInt(digits) if negative => {
                Pattern::BigInt(self.alloc_str(&format!("-{digits}")))
            }
            NumberLiteral::BigInt(digits) => Pattern::BigInt(digits),
        };
        Ok(self.add_end(start, pattern))
    }

    /// At `"`.
    fn pattern_string(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        let s = self.string_literal(error::Pattern::Start, error::Pattern::String)?;
        Ok(self.add_end(start, Pattern::Str(s)))
    }
}

/// Snapshot test macro for successful pattern parsing.
#[cfg(test)]
macro_rules! assert_pattern_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let result = parser
            .pattern()
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

/// Snapshot test macro for pattern parse errors.
#[cfg(test)]
macro_rules! assert_pattern_error_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let err = parser
            .pattern()
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
pub(crate) use assert_pattern_error_snapshot;
#[cfg(test)]
pub(crate) use assert_pattern_snapshot;

#[cfg(test)]
mod tests {
    /// Like `assert_pattern_snapshot!` but for `pattern_alternatives()`,
    /// which has no other entry point until `arm_head` lands.
    macro_rules! assert_alternatives_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            let result = parser
                .pattern_alternatives()
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

    /// Like `assert_pattern_error_snapshot!` but with query mode on, so a
    /// SQL word is refused (`lower_name`, §5.8).
    macro_rules! assert_query_pattern_error_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            let err = parser
                .with_query(true, |p| p.pattern())
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

    #[test]
    fn wildcard() {
        assert_pattern_snapshot!("_");
    }

    #[test]
    fn variable() {
        assert_pattern_snapshot!("foo");
    }

    #[test]
    fn number() {
        assert_pattern_snapshot!("42");
    }

    #[test]
    fn negative_number() {
        assert_pattern_snapshot!("-1");
    }

    #[test]
    fn bigint() {
        assert_pattern_snapshot!("123n");
    }

    #[test]
    fn string() {
        assert_pattern_snapshot!(r#""hello""#);
    }

    #[test]
    fn bool_true() {
        assert_pattern_snapshot!("true");
    }

    #[test]
    fn bool_false() {
        assert_pattern_snapshot!("false");
    }

    #[test]
    fn unit() {
        assert_pattern_snapshot!("()");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn pin_var() {
        assert_pattern_snapshot!("^expected");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn pin_access() {
        assert_pattern_snapshot!("^user.id");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn pin_call() {
        assert_pattern_snapshot!("^f(x)");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn pin_parens() {
        assert_pattern_snapshot!("^(a + b)");
    }

    #[test]
    fn alias_simple() {
        assert_pattern_snapshot!("x as y");
    }

    #[test]
    fn alias_ctor() {
        assert_pattern_snapshot!("Some(x) as opt");
    }

    #[test]
    fn alias_tuple() {
        assert_pattern_snapshot!("(a, b) as pair");
    }

    #[test]
    fn alternatives_two() {
        assert_alternatives_snapshot!("None | Some(x)");
    }

    #[test]
    fn alternatives_three() {
        assert_alternatives_snapshot!("Red | Green | Blue");
    }

    #[test]
    fn error_wildcard_not_var() {
        assert_pattern_error_snapshot!("_foo");
    }

    #[test]
    fn error_reserved() {
        assert_pattern_error_snapshot!("match");
    }

    #[test]
    fn error_alias_no_name() {
        assert_pattern_error_snapshot!("x as");
    }

    #[test]
    fn error_start() {
        assert_pattern_error_snapshot!("=>");
    }

    #[test]
    fn error_sql_word_in_query() {
        assert_query_pattern_error_snapshot!("select");
    }
}
