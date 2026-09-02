//! `schema` declarations.
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/schema.rs (Wave 3)

use alder_source::SchemaDecl;

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `schema`.
    pub(crate) fn schema_decl(&mut self) -> Result<&'a SchemaDecl<'a>, error::Schema<'a>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
