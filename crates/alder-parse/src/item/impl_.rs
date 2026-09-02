//! `impl` declarations.
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/impl_.rs (Wave 3)

use alder_source::ImplDecl;

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `impl`. Body items are line-break separated (Impl::SameLine); a `;` after an item → Impl::Semicolon.
    pub(crate) fn impl_decl(&mut self) -> Result<&'a ImplDecl<'a>, error::Impl<'a>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
