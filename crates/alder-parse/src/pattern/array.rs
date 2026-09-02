//! Array patterns `[a, b, ..rest]`.
//!
//! See docs/parser-internals.md §5.14.
// OWNER: pattern/array.rs (Wave 1)

use alder_region::{Located, Position};
use alder_source::Pattern;

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// At `[`.
    pub(crate) fn pattern_array(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, error::PArray<'a>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
