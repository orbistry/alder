//! `let [mut] pattern [: Type] = expr` — shared by items, statements and child blocks.
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/let_.rs (Wave 2)

use alder_source::LetDecl;

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `let`.
    pub(crate) fn let_decl(&mut self) -> Result<&'a LetDecl<'a>, error::Let<'a>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
