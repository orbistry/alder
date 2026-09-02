//! Items: attributes, visibility and every top-level declaration form.
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/mod.rs (Wave 3)

mod attribute;
mod component;
mod enum_;
mod error_;
mod fn_;
mod impl_;
mod import;
mod let_;
mod macro_;
mod schema;
mod table;
mod test;
mod trait_;
mod type_alias;

use alder_region::Located;
use alder_source::Item;

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// attributes* [pub] item_body. Chomps trailing whitespace.
    pub fn item(&mut self) -> Result<&'a Located<Item<'a>>, error::Item<'a>> {
        todo!()
    }

    /// Items until `}` (for `tests { }`); `}` is consumed. Same line-break rule as
    /// `module()` → Tests::SameLine. `item()` itself reports a `;` as Item::Semicolon.
    pub(crate) fn items_until_close(
        &mut self,
    ) -> Result<&'a [&'a Located<Item<'a>>], error::Tests<'a>> {
        todo!()
    }
}

/// Snapshot test macro for successful item parsing.
#[cfg(test)]
#[allow(unused)]
macro_rules! assert_item_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let result = parser
            .item()
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

/// Snapshot test macro for item parse errors.
#[cfg(test)]
#[allow(unused)]
macro_rules! assert_item_error_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let err = parser
            .item()
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
pub(crate) use assert_item_error_snapshot;
#[cfg(test)]
#[allow(unused)]
pub(crate) use assert_item_snapshot;

#[cfg(test)]
mod tests {}
