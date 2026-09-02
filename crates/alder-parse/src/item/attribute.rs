//! `#[name]` / `#[name(args)]` attributes.
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/attribute.rs (Wave 3)

use alder_region::Located;
use alder_source::Attribute;

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// Zero or more attributes, each followed by whitespace.
    pub(crate) fn attributes(
        &mut self,
    ) -> Result<&'a [Located<Attribute<'a>>], error::Attribute<'a>> {
        todo!()
    }

    /// At `#`.
    pub(crate) fn attribute(&mut self) -> Result<Located<Attribute<'a>>, error::Attribute<'a>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
