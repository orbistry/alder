//! Constructor and tag patterns.
//!
//! See docs/parser-internals.md §5.14.
// OWNER: pattern/ctor.rs (Wave 1)

use alder_region::{Located, Position};
use alder_source::Pattern;

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// At an uppercase letter: `None`, `Some(x)`, `Option::Some(x)`, `Rect { .. }`.
    pub(crate) fn pattern_ctor(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        todo!()
    }

    /// At `:`: `:tag` / `:tag(p, …)`.
    pub(crate) fn pattern_tag(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
