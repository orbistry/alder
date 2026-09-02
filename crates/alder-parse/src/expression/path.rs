//! Names and paths: Var, Path, PathVar, RecordCtor, macro call dispatch.
//!
//! See docs/parser-internals.md §5.13.
// OWNER: expression/path.rs (Wave 2)

use alder_region::{Located, Position};
use alder_source::{Expr, Path};

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// At a letter: `Var` (adjacent `!(` → `macro_call`), `Path`, or `PathVar`.
    pub(crate) fn name_or_path(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }

    /// At `{` after a `Path` (same line, `record_ctor_allowed()`).
    pub(crate) fn record_ctor(
        &mut self,
        start: Position,
        path: Path<'a>,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
