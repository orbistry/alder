//! Array literals.
//!
//! See docs/parser-internals.md §5.13.
// OWNER: expression/array.rs (Wave 2)

use alder_region::{Located, Position};
use alder_source::Expr;

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// At `[`.
    pub(crate) fn array(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
