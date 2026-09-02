//! `trait` declarations.
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/trait_.rs (Wave 3)

use alder_source::TraitDecl;

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `trait`. `type_params` is required (missing `[` → Trait::Params(TypeParams::Open)).
    /// Body items are line-break separated (Trait::SameLine); a `;` after an item → Trait::Semicolon.
    pub(crate) fn trait_decl(&mut self) -> Result<&'a TraitDecl<'a>, error::Trait<'a>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
