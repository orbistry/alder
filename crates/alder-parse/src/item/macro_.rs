//! `macro` declarations (raw bodies until M5) and `comptime` blocks.
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/macro_.rs (Wave 3)

use alder_region::Located;
use alder_source::{Block, MacroDecl};

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `macro`.
    pub(crate) fn macro_decl(&mut self) -> Result<&'a MacroDecl<'a>, error::Macro> {
        todo!()
    }

    /// After `comptime`.
    pub(crate) fn comptime_block(&mut self) -> Result<&'a Located<Block<'a>>, error::Block<'a>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
