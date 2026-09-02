//! Unit, parenthesized and tuple expressions.
//!
//! See docs/parser-internals.md §5.13.
// OWNER: expression/tuple.rs (Wave 2)

use alder_region::{Located, Position};
use alder_source::Expr;

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// At `(`: unit / parenthesized / tuple.
    pub(crate) fn tuple(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
