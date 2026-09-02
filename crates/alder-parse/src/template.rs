//! Template literals: `` `…${expr}…` `` (docs/parser-internals.md §6.1).
//!
//! See docs/parser-internals.md §5.7.
// OWNER: template.rs (Wave 2)

use alder_region::{Located, Position};
use alder_source::{Expr, TemplatePart};

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// At the opening backtick. Used by primary and by tagged templates.
    pub(crate) fn template_parts(&mut self) -> Result<&'a [TemplatePart<'a>], error::Template<'a>> {
        todo!()
    }

    /// `Expr::Template` primary.
    pub(crate) fn template(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }
}

/// Snapshot test macro for successful template parsing.
#[cfg(test)]
#[allow(unused)]
macro_rules! assert_template_snapshot {
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

/// Snapshot test macro for template parse errors.
#[cfg(test)]
#[allow(unused)]
macro_rules! assert_template_error_snapshot {
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
pub(crate) use assert_template_error_snapshot;
#[cfg(test)]
#[allow(unused)]
pub(crate) use assert_template_snapshot;

#[cfg(test)]
mod tests {}
