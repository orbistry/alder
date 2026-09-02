//! `error` groups.
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/error_.rs (Wave 3)

use alder_source::ErrorDecl;

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `error`.
    pub(crate) fn error_decl(&mut self) -> Result<&'a ErrorDecl<'a>, error::ErrorDecl<'a>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
