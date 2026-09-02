//! Number and string literal primaries.
//!
//! See docs/parser-internals.md §5.13.
// OWNER: expression/literal.rs (Wave 2)

use alder_region::{Located, Position};
use alder_source::Expr;

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// At a digit: `Expr::Number` or `Expr::BigInt`.
    pub(crate) fn number(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }

    /// At `"`.
    pub(crate) fn string(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
