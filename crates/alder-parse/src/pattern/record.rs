//! Record patterns `{ a, b: p, .. }` — shared with `CtorRecord`.
//!
//! See docs/parser-internals.md §5.14.
// OWNER: pattern/record.rs (Wave 1)

use alder_region::Region;
use alder_source::FieldPattern;

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `{`.
    pub(super) fn pattern_record_fields(
        &mut self,
    ) -> Result<(&'a [FieldPattern<'a>], Option<Region>), error::PRecord<'a>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
