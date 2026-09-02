//! Typed markup: elements, fragments, attributes and children
//! (docs/parser-internals.md §6.2).
//!
//! See docs/parser-internals.md §5.16.
//!
//! Markup mode is entered from `primary` on `<`. Tag, attribute and
//! close-tag names are read with `dashed_name` (keyword-insensitive, §2.4)
//! or `path` (components). Attributes are code mode (whitespace between
//! them is chomped); children are **text mode**: nothing is chomped, every
//! byte that is not `<`, `{`, `}` or a directive-starting `@` is text, and
//! the only text dropped is a whitespace-only run containing a newline
//! (the JSX rule, §10.22).
//!
//! `@` is position-dependent, and deliberately so. At a **child start**
//! (right after `>`, a hole, a nested element or a directive block) `@` +
//! identifier is a directive: `if` / `for` / `match` parse, `else` /
//! `empty` are stray, any other word is `UnknownDirective` (§6.2, and the
//! 7.2 test `error_directive_unknown`) so a typo like `@esle` is reported
//! rather than rendered. **Inside a text run** only the five directive
//! words (plus a non-identifier byte) end the run; every other `@` is text
//! (§2, §10.22: `a@b.com`, `<p> @iffy</p>`). So `<p>@iffy</p>` is an error
//! while `<p> @iffy</p>` is text — the same rule language.md states for
//! disambiguation ("text never starts with `@if` …"): a run never *starts*
//! with a directive-shaped `@word`, and `{"@"}` is the literal escape.
//!
//! Close tags: the name must follow `</` directly (HTML's rule: `</ p>` is
//! `CloseName`), but whitespace and comments may separate the name from
//! `>` (`</p >`, `</div\n>`), as they may in an open tag before `>`.
//!
//! Error positions: `Name` at the byte after `<`; `Attr` at the attribute
//! name; `TagEnd` at the byte that is neither an attribute, `>` nor `/>`;
//! `Child` where the children began (after the open tag's `>`; the nested
//! `Child` error has the exact spot); `CloseName` at the byte after `</`;
//! `CloseMismatch` at the close name; `CloseEnd` where `>` was expected;
//! `Unclosed` / `FragmentUnclosed` at the opening `<`. The wrapping
//! `Expr::Markup` carries the position of the opening `<`.
// OWNER: markup/mod.rs (Wave 3)

mod directive;

use alder_region::{Located, Position, Region};
use alder_source::{Attr, AttrValue, Child, Element, ElementName, Expr, Markup};
use bumpalo::collections::Vec as BumpVec;

use crate::keyword::is_ident_byte;
use crate::{Parser, error};

/// What ends a `children` loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChildTerminator {
    /// `</` — inside an element.
    CloseTag,
    /// `}` — inside a `child_block`.
    Brace,
}

/// The `@` words that end a text run (§2: `@` followed by one of these and
/// then a non-identifier byte). `if` / `for` / `match` start a directive;
/// `else` / `empty` continue one (or are stray).
const DIRECTIVE_WORDS: [&str; 5] = ["if", "for", "match", "else", "empty"];

impl<'a> Parser<'a> {
    /// At `<`. Produces Expr::Markup; does not chomp (postfix loop does).
    pub(crate) fn markup(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let (row, col) = (start.line, start.column);
        let markup = if self.peek_at(1) == Some(b'>') {
            self.fragment().map(Markup::Fragment)
        } else {
            self.element().map(Markup::Element)
        }
        .map_err(|e| error::Expr::Markup(self.alloc(e), row, col))?;
        Ok(self.add_end(start, Expr::Markup(self.alloc(markup))))
    }

    /// At `<name`. Consumes through `/>` or the matching close tag's `>`.
    pub(crate) fn element(&mut self) -> Result<&'a Element<'a>, error::Markup<'a>> {
        let open = self.get_position();
        self.word1(b'<', error::Markup::Name)?;
        let name = self.element_name()?;
        self.chomp();
        let (attrs, self_closing) = self.attrs()?;
        let children = if self_closing {
            &[][..]
        } else {
            let children = self.element_children()?;
            if self.is_eof() {
                return Err(error::Markup::Unclosed {
                    name: self.element_name_text(name.value),
                    row: open.line,
                    col: open.column,
                });
            }
            self.closing_tag(name)?;
            children
        };
        Ok(self.alloc(Element {
            name,
            attrs,
            children,
            self_closing,
        }))
    }

    /// After `<`: `dashed_name` (keyword-insensitive) or a component `path`.
    fn element_name(&mut self) -> Result<Located<ElementName<'a>>, error::Markup<'a>> {
        let start = self.get_position();
        match self.peek() {
            Some(b) if b.is_ascii_lowercase() => {
                let name = self.dashed_name(error::Markup::Name)?;
                Ok(Located::at(name.region, ElementName::Tag(name.value)))
            }
            Some(b) if b.is_ascii_uppercase() => {
                let path = self.path(error::Markup::Name, error::Markup::Name)?;
                Ok(self.located(start, ElementName::Component(path)))
            }
            _ => Err(error::Markup::Name(start.line, start.column)),
        }
    }

    /// The name as written, for `Unclosed` / `CloseMismatch` messages.
    /// Multi-segment component names are re-joined in the arena.
    fn element_name_text(&self, name: ElementName<'a>) -> &'a str {
        match name {
            ElementName::Tag(tag) => tag,
            ElementName::Component(path) => match path.segments {
                [single] => single.value,
                segments => {
                    let mut text = bumpalo::collections::String::new_in(self.bump);
                    for (i, segment) in segments.iter().enumerate() {
                        if i > 0 {
                            text.push_str("::");
                        }
                        text.push_str(segment.value);
                    }
                    text.into_bump_str()
                }
            },
        }
    }

    /// At `<>`. Consumes through `</>`.
    pub(crate) fn fragment(&mut self) -> Result<&'a [&'a Located<Child<'a>>], error::Markup<'a>> {
        let open = self.get_position();
        self.word2(b'<', b'>', error::Markup::Name)?;
        let children = self.element_children()?;
        if self.is_eof() {
            return Err(error::Markup::FragmentUnclosed(open.line, open.column));
        }
        // `children` stopped at `</`.
        self.advance_by(2);
        // `</name>` closing a fragment: say which name, not just "expected `>`".
        let name_start = self.pos;
        let (row, col) = self.position();
        if self.close_name_text(name_start)?.is_some() {
            return Err(error::Markup::CloseMismatch {
                expected: "",
                found: self.slice_from(name_start),
                row,
                col,
            });
        }
        self.chomp();
        self.word1(b'>', error::Markup::CloseEnd)?;
        Ok(children)
    }

    /// Attributes up to and including `>` or `/>`; the bool is `self_closing`.
    /// Code mode: whitespace and comments between attributes are chomped.
    pub(crate) fn attrs(&mut self) -> Result<(&'a [Attr<'a>], bool), error::Markup<'a>> {
        let mut attrs = BumpVec::new_in(self.bump);
        loop {
            self.chomp();
            let (row, col) = self.position();
            match self.peek() {
                Some(b'>') => {
                    self.advance();
                    return Ok((attrs.into_bump_slice(), false));
                }
                Some(b'/') if self.peek_at(1) == Some(b'>') => {
                    self.advance_by(2);
                    return Ok((attrs.into_bump_slice(), true));
                }
                Some(b) if b.is_ascii_lowercase() => {
                    let attr = self.specialize(
                        |bump, e, row, col| error::Markup::Attr(bump.alloc(e), row, col),
                        |p| p.attr(),
                    )?;
                    attrs.push(attr);
                }
                _ => return Err(error::Markup::TagEnd(row, col)),
            }
        }
    }

    /// At a lowercase letter: `name [ '=' ( string | '{' expression '}' ) ]`.
    /// Whitespace may surround the `=`, as in HTML and JSX.
    fn attr(&mut self) -> Result<Attr<'a>, error::Attr<'a>> {
        let name = self.dashed_name(|_, _| unreachable!("peeked lowercase"))?;
        let has_value = self.lookahead(|p| {
            p.chomp();
            p.peek() == Some(b'=')
        });
        if !has_value {
            return Ok(Attr { name, value: None });
        }
        self.chomp();
        self.advance(); // `=`
        self.chomp();
        let value = match self.peek() {
            Some(b'"') => {
                let start = self.get_position();
                let text = self.string_literal(error::Attr::Value, error::Attr::String)?;
                AttrValue::Str(self.located(start, text))
            }
            Some(b'{') => {
                self.advance();
                self.chomp();
                let expr = self.specialize(
                    |bump, e, row, col| error::Attr::Expr(bump.alloc(e), row, col),
                    |p| p.with_record_ctor(true, |p| p.expression()),
                )?;
                // `expression()` chomped.
                self.word1(b'}', error::Attr::ExprEnd)?;
                AttrValue::Expr(expr)
            }
            _ => {
                let (row, col) = self.position();
                return Err(error::Attr::Value(row, col));
            }
        };
        Ok(Attr {
            name,
            value: Some(value),
        })
    }

    /// The children of an element or fragment, up to `</` or EOF; a failing
    /// child is `Markup::Child` positioned where the children began (right
    /// after the open tag's `>`); the nested `Child` error carries the
    /// precise spot.
    fn element_children(&mut self) -> Result<&'a [&'a Located<Child<'a>>], error::Markup<'a>> {
        self.specialize(
            |bump, e, row, col| error::Markup::Child(bump.alloc(e), row, col),
            |p| p.children(ChildTerminator::CloseTag),
        )
    }

    /// Text mode loop until `</` (CloseTag) or `}` (Brace). Stops at EOF
    /// too; the caller reports `Unclosed` / `FragmentUnclosed` / `End`.
    ///
    /// Elements and fragments call this with `CloseTag`. A `child_block`
    /// cannot: its items are `ChildItem`s (`let` / `use` statements or
    /// children, §3 / §10.23), not `Child`ren, so `child_items` in
    /// `directive.rs` runs the same loop over `at_terminator(Brace)` and
    /// `child(Brace)` and adds the statement case.
    pub(crate) fn children(
        &mut self,
        term: ChildTerminator,
    ) -> Result<&'a [&'a Located<Child<'a>>], error::Child<'a>> {
        let mut children = BumpVec::new_in(self.bump);
        loop {
            if self.at_terminator(term) {
                return Ok(children.into_bump_slice());
            }
            if let Some(child) = self.child(term)? {
                children.push(child);
            }
        }
    }

    /// EOF, `</` (CloseTag) or `}` (Brace).
    pub(super) fn at_terminator(&self, term: ChildTerminator) -> bool {
        match (self.peek(), term) {
            (None, _) => true,
            (Some(b'<'), ChildTerminator::CloseTag) => self.peek_at(1) == Some(b'/'),
            (Some(b'}'), ChildTerminator::Brace) => true,
            _ => false,
        }
    }

    /// One child; None = droppable whitespace run. The caller has already
    /// checked `at_terminator(term)`, so a `</` or `}` seen here is the
    /// *other* terminator — a close tag inside a `child_block`, a bare `}`
    /// in an element — and an error; `term` only guards that invariant.
    pub(crate) fn child(
        &mut self,
        term: ChildTerminator,
    ) -> Result<Option<&'a Located<Child<'a>>>, error::Child<'a>> {
        // Counts one nesting level (§10.44): elements, fragments and
        // directives all recurse through here.
        self.nest(error::Child::TooDeep, |p| p.child_unguarded(term))
    }

    fn child_unguarded(
        &mut self,
        term: ChildTerminator,
    ) -> Result<Option<&'a Located<Child<'a>>>, error::Child<'a>> {
        debug_assert!(
            !self.at_terminator(term),
            "child() called on its own terminator {term:?} at {:?}",
            self.position()
        );
        let start = self.get_position();
        let (row, col) = self.position();
        match self.peek() {
            Some(b'<') if self.peek_at(1) == Some(b'/') => Err(error::Child::Element(
                self.alloc(error::Markup::CloseName(row, col + 2)),
                row,
                col,
            )),
            Some(b'<') => {
                let child = if self.peek_at(1) == Some(b'>') {
                    self.fragment().map(Child::Fragment)
                } else {
                    self.element().map(Child::Element)
                }
                .map_err(|e| error::Child::Element(self.alloc(e), row, col))?;
                Ok(Some(self.add_end(start, child)))
            }
            Some(b'{') => {
                self.advance();
                let expr = self.hole()?;
                Ok(Some(self.add_end(start, Child::Hole(expr))))
            }
            Some(b'}') => Err(error::Child::StrayBrace(row, col)),
            Some(b'@') if self.peek_at(1).is_some_and(|b| b.is_ascii_alphabetic()) => {
                self.directive().map(Some)
            }
            _ => Ok(self.text()),
        }
    }

    /// After `{`: whitespace, the expression, whitespace, `}`. The hole
    /// clears `no_record_ctor` like any bracket (§2.3).
    fn hole(&mut self) -> Result<&'a Located<Expr<'a>>, error::Child<'a>> {
        self.chomp();
        if self.peek() == Some(b'}') {
            let (row, col) = self.position();
            return Err(error::Child::HoleEmpty(row, col));
        }
        let expr = self.specialize(
            |bump, e, row, col| error::Child::Hole(bump.alloc(e), row, col),
            |p| p.with_record_ctor(true, |p| p.expression()),
        )?;
        // `expression()` chomped.
        self.word1(b'}', error::Child::HoleEnd)?;
        Ok(expr)
    }

    /// A text run until `<`, `{`, `}`, EOF or a directive-starting `@`.
    /// None when the run is whitespace-only and contains a newline.
    ///
    /// `}` ends a run under both terminators: in an element it is the next
    /// child's `StrayBrace`. The bytes are advanced one at a time (newline
    /// tracking); every stop byte is ASCII, so the slice ends on a character
    /// boundary.
    fn text(&mut self) -> Option<&'a Located<Child<'a>>> {
        let start = self.get_position();
        let start_pos = self.pos;
        let mut only_whitespace = true;
        let mut has_newline = false;
        while let Some(b) = self.peek() {
            match b {
                b'<' | b'{' | b'}' => break,
                b'@' if self.at_directive_word() => break,
                b'\n' => has_newline = true,
                b' ' | b'\t' | b'\r' => {}
                _ => only_whitespace = false,
            }
            self.advance();
        }
        if only_whitespace && has_newline {
            return None;
        }
        let text = self.slice_from(start_pos);
        Some(self.add_end(start, Child::Text(text)))
    }

    /// At `@`: is one of the directive words next, followed by a
    /// non-identifier byte? (`@if x` yes; `@iffy`, `a@b.com` no.)
    fn at_directive_word(&self) -> bool {
        let rest = &self.remaining()[1..];
        DIRECTIVE_WORDS.iter().any(|word| {
            rest.starts_with(word.as_bytes())
                && !rest.get(word.len()).copied().is_some_and(is_ident_byte)
        })
    }

    /// At `</`: the close tag matching `name`.
    ///
    /// The close name is lexed by its own shape (`dashed_name` for a
    /// lowercase start, `path` for an uppercase one) and compared as raw
    /// text with the opener, so `<div></Div>` is a `CloseMismatch` rather
    /// than a `CloseName`. Whitespace (and comments) may precede the `>`,
    /// as in an open tag; `CloseEnd` is reported after them.
    fn closing_tag(&mut self, name: Located<ElementName<'a>>) -> Result<(), error::Markup<'a>> {
        self.word2(b'<', b'/', error::Markup::CloseName)?;
        let name_start = self.pos;
        let (row, col) = self.position();
        let Some(found) = self.close_name_text(name_start)? else {
            return Err(error::Markup::CloseName(row, col));
        };
        let expected = self.element_name_text(name.value);
        if found != expected {
            return Err(error::Markup::CloseMismatch {
                expected,
                found,
                row,
                col,
            });
        }
        self.chomp();
        self.word1(b'>', error::Markup::CloseEnd)
    }

    /// After `</`: the raw text of the name there, `None` (nothing
    /// consumed) when no name starts at the cursor.
    fn close_name_text(&mut self, name_start: usize) -> Result<Option<&'a str>, error::Markup<'a>> {
        match self.peek() {
            Some(b) if b.is_ascii_lowercase() => {
                self.dashed_name(error::Markup::CloseName)?;
            }
            Some(b) if b.is_ascii_uppercase() => {
                self.path(error::Markup::CloseName, error::Markup::CloseName)?;
            }
            _ => return Ok(None),
        }
        Ok(Some(self.slice_from(name_start)))
    }

    /// Spaces, tabs, CR and LF only — not comments, which are text in
    /// child position. For the two places text mode looks past layout
    /// whitespace: `@else` / `@empty` after a block and `let` / `use` at
    /// a child-block item start.
    pub(super) fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.advance();
        }
    }

    /// A child node spanning `start`..`end`.
    fn child_at(&self, start: Position, end: Position, value: Child<'a>) -> &'a Located<Child<'a>> {
        self.alloc(Located::at(Region::new(start, end), value))
    }
}

/// Snapshot test macro for successful markup parsing.
#[cfg(test)]
macro_rules! assert_markup_snapshot {
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

/// Snapshot test macro for markup parse errors.
#[cfg(test)]
macro_rules! assert_markup_error_snapshot {
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
pub(crate) use assert_markup_error_snapshot;
#[cfg(test)]
pub(crate) use assert_markup_snapshot;

#[cfg(test)]
mod tests {
    // ---- elements ---------------------------------------------------------

    #[test]
    fn element_empty() {
        assert_markup_snapshot!("<div></div>");
    }

    #[test]
    fn element_self_closing() {
        assert_markup_snapshot!("<br />");
    }

    #[test]
    fn element_self_closing_no_space() {
        assert_markup_snapshot!("<br/>");
    }

    #[test]
    fn element_text() {
        assert_markup_snapshot!("<p>hello</p>");
    }

    #[test]
    fn element_hole() {
        assert_markup_snapshot!("<p>{name}</p>");
    }

    #[test]
    fn element_text_and_holes() {
        assert_markup_snapshot!("<p>{props.label}: {count} ({double})</p>");
    }

    #[test]
    fn element_nested() {
        assert_markup_snapshot!("<div><p>a</p><p>b</p></div>");
    }

    #[test]
    fn element_attr_string() {
        assert_markup_snapshot!(r#"<a href="/home">home</a>"#);
    }

    #[test]
    fn element_attr_expr() {
        assert_markup_snapshot!("<ul class={styles.list}></ul>");
    }

    #[test]
    fn element_attr_boolean() {
        assert_markup_snapshot!("<text bold>Tasks</text>");
    }

    #[test]
    fn element_attr_dashed() {
        assert_markup_snapshot!(r#"<button aria-label="Close" data-id={id} />"#);
    }

    #[test]
    fn element_attr_reserved_name() {
        assert_markup_snapshot!(r#"<Field name="password" type="password" />"#);
    }

    #[test]
    fn element_attr_reserved_names_html() {
        assert_markup_snapshot!(r#"<label for="x" style="color: red">Name</label>"#);
    }

    #[test]
    fn element_attr_spaces_around_equals() {
        assert_markup_snapshot!(r#"<a href = "/x" class = {c} />"#);
    }

    #[test]
    fn element_attrs_multiline() {
        assert_markup_snapshot!(
            r#"
            <input
                type="text"
                value={v}
                disabled
            />
            "#
        );
    }

    #[test]
    fn element_reserved_tag_name() {
        assert_markup_snapshot!("<table><tr><td>x</td></tr></table>");
    }

    // `style` is a reserved word but an ordinary tag name (§2.4); its
    // children are ordinary children (raw-text elements are not part of M1).
    #[test]
    fn element_reserved_tag_name_style() {
        assert_markup_snapshot!("<style>{css}</style>");
    }

    // A `{` in text is always a hole (§6.2), so a literal CSS body cannot be
    // written as text: `{ }` is an empty hole.
    #[test]
    fn error_style_css_body_is_hole() {
        assert_markup_error_snapshot!("<style>.a { }</style>");
    }

    #[test]
    fn element_attr_lambda_assign() {
        assert_markup_snapshot!("<button onClick={fn() count += 1}>+</button>");
    }

    #[test]
    fn element_attr_expr_record_ctor() {
        assert_markup_snapshot!("<Box size={Size::Fixed { px: 8 }} />");
    }

    #[test]
    fn element_component() {
        assert_markup_snapshot!("<Spinner />");
    }

    #[test]
    fn element_component_children() {
        assert_markup_snapshot!("<Card>text</Card>");
    }

    #[test]
    fn element_component_path() {
        assert_markup_snapshot!(r#"<Ui::Button label="x">go</Ui::Button>"#);
    }

    #[test]
    fn element_custom_dashed() {
        assert_markup_snapshot!(r#"<my-widget data-id="1"></my-widget>"#);
    }

    #[test]
    fn fragment() {
        assert_markup_snapshot!("<><p>a</p><p>b</p></>");
    }

    #[test]
    fn fragment_empty() {
        assert_markup_snapshot!("<></>");
    }

    #[test]
    fn fragment_nested_in_element() {
        assert_markup_snapshot!("<div><>a</></div>");
    }

    // ---- close tags -------------------------------------------------------

    #[test]
    fn close_tag_space_before_end() {
        assert_markup_snapshot!("<p>x</p >");
    }

    #[test]
    fn close_tag_newline_before_end() {
        assert_markup_snapshot!("<div></div\n>");
    }

    #[test]
    fn fragment_close_space_before_end() {
        assert_markup_snapshot!("<>x</ >");
    }

    // ---- text -------------------------------------------------------------

    #[test]
    fn whitespace_only_lines_dropped() {
        assert_markup_snapshot!(
            r#"
            <ul>
                <li>a</li>
                <li>b</li>
            </ul>
            "#
        );
    }

    #[test]
    fn text_keeps_inner_spaces() {
        assert_markup_snapshot!("<p>  a   b  </p>");
    }

    #[test]
    fn text_multiline_kept_verbatim() {
        assert_markup_snapshot!(
            r#"
            <p>
                two
                lines
            </p>
            "#
        );
    }

    #[test]
    fn text_with_at_sign() {
        assert_markup_snapshot!("<a>mail a@b.com or @iffy</a>");
    }

    // `@iffy` after a space is inside a text run, so it is text; the same
    // word at a child start is `UnknownDirective` (module doc,
    // `error_directive_unknown_after_hole`).
    #[test]
    fn text_at_word_after_space() {
        assert_markup_snapshot!("<p> @iffy</p>");
    }

    #[test]
    fn text_with_punctuation() {
        assert_markup_snapshot!("<p>Hi, there! (see http://x.y/z) = 1 + 2 / 3;</p>");
    }

    #[test]
    fn text_between_elements_same_line_kept() {
        assert_markup_snapshot!("<p><b>a</b> <i>b</i></p>");
    }

    #[test]
    fn text_unicode() {
        assert_markup_snapshot!("<p>héllo → wörld</p>");
    }

    // ---- holes ------------------------------------------------------------

    #[test]
    fn hole_record() {
        assert_markup_snapshot!("<Chart data={{ x: 1 }}>{{ y }}</Chart>");
    }

    #[test]
    fn hole_string_literal_at() {
        assert_markup_snapshot!(r#"<p>{"@"}</p>"#);
    }

    #[test]
    fn hole_with_spaces() {
        assert_markup_snapshot!("<p>{ a + b }</p>");
    }

    #[test]
    fn hole_nested_markup() {
        assert_markup_snapshot!("<p>{if x { <b>y</b> } else { <i>n</i> }}</p>");
    }

    // ---- markup inside code ----------------------------------------------

    #[test]
    fn markup_in_match_arm() {
        assert_markup_snapshot!("match s { A => <p>a</p>, B => <p>b</p> }");
    }

    #[test]
    fn markup_as_block_tail() {
        assert_markup_snapshot!(
            r#"
            {
                let x = 1
                <p>{x}</p>
            }
            "#
        );
    }

    #[test]
    fn markup_after_newline_new_stmt() {
        assert_markup_snapshot!(
            r#"
            {
                x
                <div />
            }
            "#
        );
    }

    #[test]
    fn markup_in_call_arg() {
        assert_markup_snapshot!("render(<p>x</p>)");
    }

    #[test]
    fn markup_with_trailing_whitespace() {
        assert_markup_snapshot!("<p>x</p>  \n");
    }

    // ---- errors -----------------------------------------------------------

    #[test]
    fn error_name() {
        assert_markup_error_snapshot!("<div>< </div>");
    }

    #[test]
    fn error_name_component_dangling() {
        assert_markup_error_snapshot!("<Ui:: />");
    }

    #[test]
    fn error_tag_end() {
        assert_markup_error_snapshot!("<div =>");
    }

    #[test]
    fn error_tag_end_slash_not_close() {
        assert_markup_error_snapshot!("<div / >");
    }

    #[test]
    fn error_close_mismatch() {
        assert_markup_error_snapshot!("<div></span>");
    }

    #[test]
    fn error_close_mismatch_case() {
        assert_markup_error_snapshot!("<div></Div>");
    }

    #[test]
    fn error_close_mismatch_component_path() {
        assert_markup_error_snapshot!("<Ui::Button></Ui::Btn>");
    }

    #[test]
    fn error_close_name() {
        assert_markup_error_snapshot!("<div></>");
    }

    // The close name must follow `</` directly (HTML's rule).
    #[test]
    fn error_close_name_space() {
        assert_markup_error_snapshot!("<div></ div>");
    }

    // `CloseEnd` is reported after the whitespace, at the offending byte.
    #[test]
    fn error_close_end() {
        assert_markup_error_snapshot!("<div></div x>");
    }

    #[test]
    fn error_unclosed() {
        assert_markup_error_snapshot!("<div>text");
    }

    // The inner `<p>` reaches EOF: `Unclosed` for `p`, not for `div`.
    #[test]
    fn error_unclosed_nested() {
        assert_markup_error_snapshot!("<div><p>x");
    }

    // `</div>` closes the inner `<p>`: a mismatch, not an unclosed tag.
    #[test]
    fn error_close_mismatch_nested() {
        assert_markup_error_snapshot!("<div><p>x</div>");
    }

    #[test]
    fn error_fragment_unclosed() {
        assert_markup_error_snapshot!("<>hi");
    }

    #[test]
    fn error_fragment_closed_by_name() {
        assert_markup_error_snapshot!("<>hi</div>");
    }

    #[test]
    fn error_stray_close_brace() {
        assert_markup_error_snapshot!("<div>}</div>");
    }

    #[test]
    fn error_attr_value() {
        assert_markup_error_snapshot!("<div class=>");
    }

    #[test]
    fn error_attr_string() {
        assert_markup_error_snapshot!(r#"<div class="x>"#);
    }

    #[test]
    fn error_attr_expr() {
        assert_markup_error_snapshot!("<div class={)}>");
    }

    // `<div class={x >` would be `x > …` with a missing operand; the `}`
    // check fires only once the expression has stopped.
    #[test]
    fn error_attr_expr_unclosed() {
        assert_markup_error_snapshot!(r#"<div class={x id="y">"#);
    }

    #[test]
    fn error_hole_empty() {
        assert_markup_error_snapshot!("<p>{}</p>");
    }

    #[test]
    fn error_hole_unclosed() {
        assert_markup_error_snapshot!("<p>{x y}</p>");
    }

    // `{x</p>` reads as the comparison `x < /p` — a missing operand, not a
    // missing `}` (§11: `<` after an operand is an operator).
    #[test]
    fn error_hole_unclosed_at_close_tag() {
        assert_markup_error_snapshot!("<p>{x</p>");
    }

    #[test]
    fn error_hole_bad_expr() {
        assert_markup_error_snapshot!("<p>{else}</p>");
    }

    #[test]
    fn error_unexpected_close_at_start() {
        assert_markup_error_snapshot!("</div>");
    }

    // ---- docs examples (language.md, web.md) ------------------------------

    #[test]
    fn docs_language_ul_directives() {
        assert_markup_snapshot!(
            r#"
            <ul class={styles.list}>
                @for item in items; key item.id {
                    <li>{item.name}</li>
                } @empty {
                    <li>Nothing here</li>
                }
                @if status.loading {
                    <Spinner />
                } @else if status.failed {
                    <p>Something went wrong</p>
                } @else {
                    <p>{count} items</p>
                }
                @match status {
                    Loading => <Spinner />,
                    Ready(n) => <span>{n}</span>,
                }
            </ul>
            "#
        );
    }

    #[test]
    fn docs_language_counter_button() {
        assert_markup_snapshot!(
            r#"
            <button onClick={fn() count += 1}>
                {props.label}: {count} ({double})
            </button>
            "#
        );
    }

    #[test]
    fn docs_web_page_h1() {
        assert_markup_snapshot!("<h1>{props.data.user.name}</h1>");
    }

    #[test]
    fn docs_web_user_card_button() {
        assert_markup_snapshot!("<button onClick={fn() deleteUser(props.id)}>Delete</button>");
    }

    #[test]
    fn docs_web_style_class() {
        assert_markup_snapshot!("<div class={card}>...</div>");
    }

    #[test]
    fn docs_web_form_fields() {
        assert_markup_snapshot!(
            r#"
            <Form action={signUp}>
                <Field name="email" />
                <Field name="password" type="password" />
            </Form>
            "#
        );
    }

    #[test]
    fn docs_web_tui_box() {
        assert_markup_snapshot!(
            r#"
            <box direction="column" border="round">
                <text bold>Tasks</text>
                @for (task, i) in tasks; key task {
                    <text inverse={i == selected}>{task}</text>
                }
            </box>
            "#
        );
    }
}
