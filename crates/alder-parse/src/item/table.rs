//! `table` declarations and the modifier lists shared with `schema` rules.
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/table.rs (Wave 3)

use alder_source::{Modifier, TableDecl};

use crate::{Col, Parser, Row, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `table`.
    pub(crate) fn table_decl(&mut self) -> Result<&'a TableDecl<'a>, error::Table<'a>> {
        todo!()
    }

    /// `name [ '(' expr { ',' expr } ')' ]` — shared with schema rules.
    pub(crate) fn modifier<E>(
        &mut self,
        to_arg_error: impl Fn(&'a error::Expr<'a>, Row, Col) -> E + Copy,
        to_end_error: impl FnOnce(Row, Col) -> E,
    ) -> Result<Modifier<'a>, E> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
