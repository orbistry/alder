//! `loop { }`, `state(expr)` and `name!( … )` macro calls.
//!
//! See docs/parser-internals.md §5.13.
// OWNER: expression/loop_.rs (Wave 2)

use alder_region::{Located, Position};
use alder_source::{Expr, Name};

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `loop`.
    pub(crate) fn loop_(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }

    /// After `state`.
    pub(crate) fn state(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }

    /// At `!(` immediately after a lowercase name.
    pub(crate) fn macro_call(
        &mut self,
        start: Position,
        name: Name<'a>,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
