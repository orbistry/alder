//! `component` declarations.
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/component.rs (Wave 3)

use alder_source::ComponentDecl;

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `component`.
    pub(crate) fn component_decl(&mut self) -> Result<&'a ComponentDecl<'a>, error::Component<'a>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
