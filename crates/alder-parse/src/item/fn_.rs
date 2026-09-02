//! `fn` declarations, parameter lists and `where` clauses.
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/fn_.rs (Wave 3; `params` / `where_clause` may land in Wave 2, see §9 step 2.4)

use alder_source::{Constraint, FnDecl, Param};

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `fn`; body optional.
    pub(crate) fn fn_decl(&mut self) -> Result<&'a FnDecl<'a>, error::Fn<'a>> {
        todo!()
    }

    /// At `(`; shared by lambda/component.
    pub(crate) fn params(&mut self) -> Result<&'a [Param<'a>], error::Params<'a>> {
        todo!()
    }

    /// After `where`; may be empty.
    pub(crate) fn where_clause(&mut self) -> Result<&'a [Constraint<'a>], error::Where<'a>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
