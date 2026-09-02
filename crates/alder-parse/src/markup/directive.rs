//! `@if` / `@for` / `@match` directives and `child_block`s.
//!
//! See docs/parser-internals.md §5.16.
// OWNER: markup/directive.rs (Wave 3)

use alder_region::Located;
use alder_source::{Child, ChildBlock};

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// At `@`.
    pub(crate) fn directive(&mut self) -> Result<&'a Located<Child<'a>>, error::Child<'a>> {
        todo!()
    }

    /// At `{`.
    pub(crate) fn child_block(
        &mut self,
    ) -> Result<&'a Located<ChildBlock<'a>>, error::ChildBlock<'a>> {
        todo!()
    }

    /// Lookahead past whitespace for `@else` / `@empty` (does not consume).
    pub(crate) fn peek_directive(&mut self, word: &[u8]) -> bool {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
