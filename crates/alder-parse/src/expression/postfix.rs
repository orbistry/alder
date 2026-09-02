//! Postfix operators: calls, indexing, `.field` / `.0` / `.await`, tagged templates.
//!
//! See docs/parser-internals.md §5.13.
// OWNER: expression/postfix.rs (Wave 2)

use alder_region::Located;
use alder_source::Expr;

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// At `(`; accepts `_` placeholders as whole arguments.
    pub(crate) fn call_args(&mut self) -> Result<&'a [&'a Located<Expr<'a>>], error::Call<'a>> {
        todo!()
    }

    /// At `[`.
    pub(crate) fn index(
        &mut self,
        target: &'a Located<Expr<'a>>,
    ) -> Result<&'a Located<Expr<'a>>, error::Index<'a>> {
        todo!()
    }

    /// At `.`: field / digits / await.
    pub(crate) fn dot_suffix(
        &mut self,
        target: &'a Located<Expr<'a>>,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }

    /// At an adjacent backtick.
    pub(crate) fn tagged_template(
        &mut self,
        tag: &'a Located<Expr<'a>>,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
