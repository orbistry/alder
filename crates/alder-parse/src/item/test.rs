//! `test "name" { }` declarations and `tests { }` blocks.
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/test.rs (Wave 3)

use alder_region::Located;
use alder_source::{Item, TestDecl};

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `test`.
    pub(crate) fn test_decl(&mut self) -> Result<&'a TestDecl<'a>, error::Test<'a>> {
        todo!()
    }

    /// After `tests`.
    pub(crate) fn tests_block(&mut self) -> Result<&'a [&'a Located<Item<'a>>], error::Tests<'a>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
