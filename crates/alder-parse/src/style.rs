//! `style { }` blocks (docs/parser-internals.md §6.4).
//!
//! See docs/parser-internals.md §5.18.
// OWNER: style.rs (Wave 3)

use alder_region::{Located, Position};
use alder_source::{Expr, Style, StyleValue};

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `style`.
    pub(crate) fn style(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }

    /// At `{`.
    pub(crate) fn style_block(&mut self) -> Result<&'a Style<'a>, error::Style<'a>> {
        todo!()
    }

    /// `{` → nested style; digit, or `-` + digit → dimension attempt; otherwise `expression()`.
    fn style_value(&mut self) -> Result<StyleValue<'a>, error::Style<'a>> {
        todo!()
    }
}

/// Snapshot test macro for successful style parsing.
#[cfg(test)]
#[allow(unused)]
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
#[allow(unused)]
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
#[allow(unused)]
pub(crate) use assert_style_error_snapshot;
#[cfg(test)]
#[allow(unused)]
pub(crate) use assert_style_snapshot;

#[cfg(test)]
mod tests {}
