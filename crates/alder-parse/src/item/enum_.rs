//! `enum` declarations.
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/enum_.rs (Wave 3)

use alder_source::EnumDecl;

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `enum`. Record payloads reuse `field_types()`; a `Some(ext)` result is Enum::VariantRecordExt.
    pub(crate) fn enum_decl(&mut self) -> Result<&'a EnumDecl<'a>, error::Enum<'a>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
