//! `match` expressions and the arm head shared with `@match`.
//!
//! See docs/parser-internals.md §5.13.
// OWNER: expression/match_.rs (Wave 2)

use alder_region::{Located, Position};
use alder_source::{Expr, Pattern};

use crate::{Col, Parser, Row, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `match`.
    pub(crate) fn match_(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }

    /// `p | q [if guard] =>` — shared with @match (errors mapped by the caller).
    #[allow(clippy::type_complexity)]
    pub(crate) fn arm_head<E>(
        &mut self,
        to_pattern: impl FnOnce(&'a error::Pattern<'a>, Row, Col) -> E,
        to_guard: impl FnOnce(&'a error::Expr<'a>, Row, Col) -> E,
        to_arrow: impl FnOnce(Row, Col) -> E,
    ) -> Result<
        (
            &'a [&'a Located<Pattern<'a>>],
            Option<&'a Located<Expr<'a>>>,
        ),
        E,
    > {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
