//! Typed markup: elements, fragments, attributes and children
//! (docs/parser-internals.md §6.2).
//!
//! See docs/parser-internals.md §5.16.
// OWNER: markup/mod.rs (Wave 3)

mod directive;

use alder_region::{Located, Position};
use alder_source::{Attr, Child, Element, ElementName, Expr};

use crate::{Parser, error};

/// What ends a `children` loop.
#[allow(unused)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChildTerminator {
    /// `</` — inside an element.
    CloseTag,
    /// `}` — inside a `child_block`.
    Brace,
}

#[allow(unused)]
impl<'a> Parser<'a> {
    /// At `<`. Produces Expr::Markup; does not chomp (postfix loop does).
    pub(crate) fn markup(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }

    /// At `<name`.
    pub(crate) fn element(&mut self) -> Result<&'a Element<'a>, error::Markup<'a>> {
        todo!()
    }

    /// At `<>`.
    pub(crate) fn fragment(&mut self) -> Result<&'a [&'a Located<Child<'a>>], error::Markup<'a>> {
        todo!()
    }

    /// Attributes up to and including `>` or `/>`; the bool is `self_closing`.
    pub(crate) fn attrs(&mut self) -> Result<(&'a [Attr<'a>], bool), error::Markup<'a>> {
        todo!()
    }

    /// Text mode loop until `</` (CloseTag) or `}` (Brace).
    pub(crate) fn children(
        &mut self,
        term: ChildTerminator,
    ) -> Result<&'a [&'a Located<Child<'a>>], error::Child<'a>> {
        todo!()
    }

    /// One child; None = droppable whitespace run.
    pub(crate) fn child(
        &mut self,
        term: ChildTerminator,
    ) -> Result<Option<&'a Located<Child<'a>>>, error::Child<'a>> {
        todo!()
    }

    /// At `</`: the close tag matching `name`.
    fn closing_tag(&mut self, name: Located<ElementName<'a>>) -> Result<(), error::Markup<'a>> {
        todo!()
    }
}

/// Snapshot test macro for successful markup parsing.
#[cfg(test)]
#[allow(unused)]
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
#[allow(unused)]
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
#[allow(unused)]
pub(crate) use assert_markup_error_snapshot;
#[cfg(test)]
#[allow(unused)]
pub(crate) use assert_markup_snapshot;

#[cfg(test)]
mod tests {}
