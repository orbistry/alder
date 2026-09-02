//! Unit, parenthesized and tuple patterns.
//!
//! See docs/parser-internals.md §5.14.
// OWNER: pattern/tuple.rs (Wave 1)

use alder_region::{Located, Position};
use alder_source::Pattern;

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// At `(`.
    pub(super) fn pattern_tuple(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, error::PTuple<'a>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
