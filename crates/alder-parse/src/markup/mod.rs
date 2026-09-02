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
        if self.peek() == Some(b'>') {
            self.advance();
            return Ok(children);
        }
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
        Err(error::Markup::CloseEnd(row, col))
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
    fn at_terminator(&self, term: ChildTerminator) -> bool {
        match (self.peek(), term) {
            (None, _) => true,
            (Some(b'<'), ChildTerminator::CloseTag) => self.peek_at(1) == Some(b'/'),
            (Some(b'}'), ChildTerminator::Brace) => true,
            _ => false,
        }
    }

    /// One child; None = droppable whitespace run. The cursor is not on the
    /// loop's terminator (the caller checked it, which is why `term` is not
    /// consulted here), so `</` and `}` are the wrong terminator: a close
    /// tag inside a `child_block`, a bare `}` in an element.
    pub(crate) fn child(
        &mut self,
        _term: ChildTerminator,
    ) -> Result<Option<&'a Located<Child<'a>>>, error::Child<'a>> {
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
    /// than a `CloseName`.
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
mod tests {}
