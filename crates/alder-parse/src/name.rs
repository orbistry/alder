//! Identifier, path, tag and markup-name scanning for Alder.
//!
//! `lower_name` refuses reserved words and, while `in_query()`, SQL words —
//! both without consuming. `raw_lower` and `dashed_name` never refuse a word
//! (docs/parser-internals.md §2.4): module-path segments and element /
//! attribute / close-tag names are keyword-insensitive.
//! See docs/parser-internals.md §2 and §5.8.

use alder_source::{Name, Path};

use crate::keyword::{is_ident_byte, is_reserved, is_sql_word};
use crate::{Col, Parser, Row};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// `[a-z][A-Za-z0-9_]*`, not reserved, not a SQL word while `in_query()`.
    /// Fails without consuming.
    pub(crate) fn lower_name<E>(
        &mut self,
        to_error: impl FnOnce(Row, Col) -> E,
    ) -> Result<&'a str, E> {
        let (row, col) = self.position();
        if !self.peek_lower() {
            return Err(to_error(row, col));
        }
        let word = self.peek_word();
        if is_reserved(word) || (self.in_query() && is_sql_word(word)) {
            return Err(to_error(row, col));
        }
        self.advance_by(word.len());
        Ok(word)
    }

    /// `[A-Z][A-Za-z0-9_]*`. Fails without consuming.
    pub(crate) fn upper_name<E>(
        &mut self,
        to_error: impl FnOnce(Row, Col) -> E,
    ) -> Result<&'a str, E> {
        let (row, col) = self.position();
        if !self.peek_upper() {
            return Err(to_error(row, col));
        }
        let word = self.peek_word();
        self.advance_by(word.len());
        Ok(word)
    }

    /// `lower_name` with its region.
    pub(crate) fn located_lower<E>(
        &mut self,
        to_error: impl FnOnce(Row, Col) -> E,
    ) -> Result<Name<'a>, E> {
        let start = self.get_position();
        let name = self.lower_name(to_error)?;
        Ok(self.located(start, name))
    }

    /// `upper_name` with its region.
    pub(crate) fn located_upper<E>(
        &mut self,
        to_error: impl FnOnce(Row, Col) -> E,
    ) -> Result<Name<'a>, E> {
        let start = self.get_position();
        let name = self.upper_name(to_error)?;
        Ok(self.located(start, name))
    }

    /// `Upper { '::' Upper }`; stops before `::lower` (the expression layer
    /// consumes that as `PathVar`). `Foo::` followed by anything else is
    /// `to_member_error` at the position after the `::`.
    pub(crate) fn path<E>(
        &mut self,
        to_expectation: impl FnOnce(Row, Col) -> E,
        to_member_error: impl FnOnce(Row, Col) -> E,
    ) -> Result<Path<'a>, E> {
        let first = self.located_upper(to_expectation)?;
        let mut segments = vec![first];
        while self.peek() == Some(b':') && self.peek_at(1) == Some(b':') {
            match self.peek_at(2) {
                Some(b) if b.is_ascii_uppercase() => {
                    self.advance_by(2);
                    let segment = self.located_upper(|_, _| unreachable!("peeked uppercase"))?;
                    segments.push(segment);
                }
                Some(b) if b.is_ascii_lowercase() => break,
                _ => {
                    self.advance_by(2);
                    let (row, col) = self.position();
                    return Err(to_member_error(row, col));
                }
            }
        }
        Ok(Path {
            segments: self.alloc_slice_copy(&segments),
        })
    }

    /// `:lower` — the returned name excludes the colon; region includes it.
    /// `to_expectation` when there is no `:`, `to_bad_name` when the `:` is
    /// not immediately followed by a lowercase letter; neither consumes.
    pub(crate) fn tag_name<E>(
        &mut self,
        to_expectation: impl FnOnce(Row, Col) -> E,
        to_bad_name: impl FnOnce(Row, Col) -> E,
    ) -> Result<Name<'a>, E> {
        let (row, col) = self.position();
        if self.peek() != Some(b':') {
            return Err(to_expectation(row, col));
        }
        if !self.peek_at(1).is_some_and(|b| b.is_ascii_lowercase()) {
            return Err(to_bad_name(row, col));
        }
        let start = self.get_position();
        self.advance();
        let name_start = self.pos;
        self.advance();
        self.chomp_inner_chars();
        let name = self.slice_from(name_start);
        Ok(self.located(start, name))
    }

    /// `[a-z][A-Za-z0-9_]*` with NO reserved-word / SQL-word check (§2.4): module-path
    /// segments. Fails without consuming when the first byte is not a lowercase letter.
    pub(crate) fn raw_lower<E>(
        &mut self,
        to_error: impl FnOnce(Row, Col) -> E,
    ) -> Result<Name<'a>, E> {
        let (row, col) = self.position();
        if !self.peek_lower() {
            return Err(to_error(row, col));
        }
        let start = self.get_position();
        let word = self.peek_word();
        self.advance_by(word.len());
        Ok(self.located(start, word))
    }

    /// `raw_lower { '-' raw_lower }` for element, attribute and close-tag names.
    /// Keyword-insensitive: `type`, `for`, `style`, `table`, `select` are names here.
    /// A `-` not followed by a lowercase letter ends the name and is not consumed.
    pub(crate) fn dashed_name<E>(
        &mut self,
        to_error: impl FnOnce(Row, Col) -> E,
    ) -> Result<Name<'a>, E> {
        let start = self.get_position();
        let start_pos = self.pos;
        self.raw_lower(to_error)?;
        while self.peek() == Some(b'-') && self.peek_at(1).is_some_and(|b| b.is_ascii_lowercase()) {
            self.advance();
            self.raw_lower(|_, _| unreachable!("peeked lowercase"))?;
        }
        let name = self.slice_from(start_pos);
        Ok(self.located(start, name))
    }

    /// Is the cursor on a lowercase ASCII letter?
    #[inline]
    pub(crate) fn peek_lower(&self) -> bool {
        self.peek().is_some_and(|b| b.is_ascii_lowercase())
    }

    /// Is the cursor on an uppercase ASCII letter?
    #[inline]
    pub(crate) fn peek_upper(&self) -> bool {
        self.peek().is_some_and(|b| b.is_ascii_uppercase())
    }

    /// Chomp identifier continuation bytes (`[A-Za-z0-9_]`).
    pub(crate) fn chomp_inner_chars(&mut self) {
        while self.peek().is_some_and(is_ident_byte) {
            self.advance();
        }
    }

    /// The source text from `start_pos` (a byte offset) to the cursor.
    pub(crate) fn slice_from(&self, start_pos: usize) -> &'a str {
        let bytes = &self.src[start_pos..self.pos];
        // SAFETY: `src` is valid UTF-8 and every scanner advances by whole
        // characters, so both ends are on character boundaries.
        unsafe { std::str::from_utf8_unchecked(bytes) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::Bump;

    fn with_parser<T>(src: &str, in_query: bool, f: impl FnOnce(&mut Parser<'_>) -> T) -> (T, u16) {
        let bump = Bump::new();
        let text = bump.alloc_str(src);
        let mut parser = Parser::new(&bump, text.as_bytes());
        let result = parser.with_query::<T, ()>(in_query, |p| Ok(f(p))).unwrap();
        let (_, col) = parser.position();
        (result, col)
    }

    fn lower(src: &str, in_query: bool) -> (Result<String, (u16, u16)>, u16) {
        let (r, col) = with_parser(src, in_query, |p| {
            p.lower_name(|r, c| (r, c)).map(str::to_owned)
        });
        (r, col)
    }

    #[test]
    fn lower_simple() {
        assert_eq!(lower("foo bar", false), (Ok("foo".into()), 4));
    }

    #[test]
    fn lower_camel() {
        assert_eq!(lower("fooBar_9(", false), (Ok("fooBar_9".into()), 9));
    }

    #[test]
    fn lower_rejects_reserved() {
        assert_eq!(lower("match x", false), (Err((1, 1)), 1));
        assert_eq!(lower("type", false), (Err((1, 1)), 1));
        // Prefixes are fine.
        assert_eq!(lower("matches", false), (Ok("matches".into()), 8));
    }

    #[test]
    fn lower_rejects_sql_word_only_in_query() {
        assert_eq!(lower("select", true), (Err((1, 1)), 1));
        assert_eq!(lower("select", false), (Ok("select".into()), 7));
        assert_eq!(lower("orderBy", true), (Err((1, 1)), 1));
        assert_eq!(lower("user", true), (Ok("user".into()), 5));
    }

    #[test]
    fn lower_rejects_non_lowercase_start() {
        assert_eq!(lower("Foo", false), (Err((1, 1)), 1));
        assert_eq!(lower("_x", false), (Err((1, 1)), 1));
        assert_eq!(lower("", false), (Err((1, 1)), 1));
    }

    #[test]
    fn upper_simple() {
        let (r, col) = with_parser("Foo::x", false, |p| {
            p.upper_name(|r, c| (r, c)).map(str::to_owned)
        });
        assert_eq!((r, col), (Ok("Foo".into()), 4));
        let (r, col) = with_parser("foo", false, |p| {
            p.upper_name(|r, c| (r, c)).map(str::to_owned)
        });
        assert_eq!((r, col), (Err((1, 1)), 1));
    }

    fn path(src: &str) -> (Result<Vec<String>, String>, u16) {
        with_parser(src, false, |p| {
            p.path(
                |r, c| format!("expect {r}:{c}"),
                |r, c| format!("member {r}:{c}"),
            )
            .map(|path| path.segments.iter().map(|n| n.value.to_owned()).collect())
        })
    }

    #[test]
    fn path_single() {
        assert_eq!(path("Some(x)"), (Ok(vec!["Some".into()]), 5));
    }

    #[test]
    fn path_nested() {
        assert_eq!(
            path("Ui::Button::Primary {"),
            (Ok(vec!["Ui".into(), "Button".into(), "Primary".into()]), 20)
        );
    }

    #[test]
    fn path_stops_before_lower() {
        assert_eq!(path("Show::show(x)"), (Ok(vec!["Show".into()]), 5));
    }

    #[test]
    fn path_dangling_colons_error() {
        assert_eq!(path("Foo::"), (Err("member 1:6".into()), 6));
        assert_eq!(path("Foo::(x)"), (Err("member 1:6".into()), 6));
        assert_eq!(path("foo"), (Err("expect 1:1".into()), 1));
    }

    #[test]
    fn path_region() {
        let bump = Bump::new();
        let text = bump.alloc_str("Option::Some");
        let mut parser = Parser::new(&bump, text.as_bytes());
        let path = parser.path(|_, _| (), |_, _| ()).unwrap();
        let region = path.region();
        assert_eq!((region.start.line, region.start.column), (1, 1));
        assert_eq!((region.end.line, region.end.column), (1, 13));
    }

    fn tag(src: &str) -> (Result<(String, u16, u16), String>, u16) {
        with_parser(src, false, |p| {
            p.tag_name(
                |r, c| format!("expect {r}:{c}"),
                |r, c| format!("bad {r}:{c}"),
            )
            .map(|n| {
                (
                    n.value.to_owned(),
                    n.region.start.column,
                    n.region.end.column,
                )
            })
        })
    }

    #[test]
    fn tag_ok() {
        assert_eq!(
            tag(":not_found(id)"),
            (Ok((("not_found".into()), 1, 11)), 11)
        );
        assert_eq!(tag(":timeout"), (Ok((("timeout".into()), 1, 9)), 9));
        // Tags are names, not bindings: reserved shapes are accepted.
        assert_eq!(tag(":type"), (Ok((("type".into()), 1, 6)), 6));
    }

    #[test]
    fn tag_space_rejected() {
        assert_eq!(tag(": x"), (Err("bad 1:1".into()), 1));
    }

    #[test]
    fn tag_upper_rejected() {
        assert_eq!(tag(":Foo"), (Err("bad 1:1".into()), 1));
        assert_eq!(tag("x"), (Err("expect 1:1".into()), 1));
    }

    fn dashed(src: &str) -> (Result<String, (u16, u16)>, u16) {
        with_parser(src, false, |p| {
            p.dashed_name(|r, c| (r, c)).map(|n| n.value.to_owned())
        })
    }

    #[test]
    fn dashed_name() {
        assert_eq!(dashed("aria-label="), (Ok("aria-label".into()), 11));
        assert_eq!(dashed("my-custom-el>"), (Ok("my-custom-el".into()), 13));
        assert_eq!(dashed("div>"), (Ok("div".into()), 4));
        // A dash not followed by a lowercase letter is left alone.
        assert_eq!(dashed("a-1"), (Ok("a".into()), 2));
        assert_eq!(dashed("a- b"), (Ok("a".into()), 2));
    }

    #[test]
    fn dashed_name_accepts_reserved() {
        assert_eq!(dashed("type="), (Ok("type".into()), 5));
        assert_eq!(dashed("style>"), (Ok("style".into()), 6));
        assert_eq!(dashed("select>"), (Ok("select".into()), 7));
        assert_eq!(dashed("for="), (Ok("for".into()), 4));
    }

    #[test]
    fn dashed_name_rejects_upper() {
        assert_eq!(dashed("Div>"), (Err((1, 1)), 1));
    }

    #[test]
    fn raw_lower_accepts_reserved() {
        let (r, col) = with_parser("test.{", false, |p| {
            p.raw_lower(|r, c| (r, c)).map(|n| n.value.to_owned())
        });
        assert_eq!((r, col), (Ok("test".into()), 5));
    }

    #[test]
    fn raw_lower_accepts_sql_word_in_query() {
        let (r, col) = with_parser("select", true, |p| {
            p.raw_lower(|r, c| (r, c)).map(|n| n.value.to_owned())
        });
        assert_eq!((r, col), (Ok("select".into()), 7));
        let (r, col) = with_parser("Foo", true, |p| {
            p.raw_lower(|r, c| (r, c)).map(|n| n.value.to_owned())
        });
        assert_eq!((r, col), (Err((1, 1)), 1));
    }

    #[test]
    fn located_names_carry_regions() {
        let bump = Bump::new();
        let text = bump.alloc_str("foo Bar");
        let mut parser = Parser::new(&bump, text.as_bytes());
        let lower = parser.located_lower(|_, _| ()).unwrap();
        assert_eq!(lower.value, "foo");
        assert_eq!((lower.region.start.column, lower.region.end.column), (1, 4));
        parser.chomp();
        let upper = parser.located_upper(|_, _| ()).unwrap();
        assert_eq!(upper.value, "Bar");
        assert_eq!((upper.region.start.column, upper.region.end.column), (5, 8));
    }
}
