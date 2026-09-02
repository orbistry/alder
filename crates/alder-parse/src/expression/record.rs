//! Record literals and the record-vs-block lookahead (docs/parser-internals.md §2.2).
//!
//! See docs/parser-internals.md §5.13.
// OWNER: expression/record.rs (Wave 2)

use alder_region::{Located, Position};
use alder_source::{Expr, RecordField};

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// At `{` (already known to look like a record).
    pub(crate) fn record(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }

    /// After `{`; also RecordCtor and query `set`.
    pub(crate) fn record_fields(&mut self) -> Result<&'a [RecordField<'a>], error::Record<'a>> {
        todo!()
    }

    /// Lookahead at `{` (§2.2).
    pub(crate) fn looks_like_record(&mut self) -> bool {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
