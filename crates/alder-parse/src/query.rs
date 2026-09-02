//! `query { }` blocks (docs/parser-internals.md §6.3).
//!
//! See docs/parser-internals.md §5.17.
// OWNER: query.rs (Wave 3)

use alder_region::{Located, Position};
use alder_source::{Expr, Query, Select, TableRef};

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `query`: `{` … `}` under `with_query(true, …)`.
    pub(crate) fn query(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }

    fn query_body(&mut self) -> Result<&'a Query<'a>, error::Query<'a>> {
        todo!()
    }

    fn select(&mut self) -> Result<&'a Select<'a>, error::Select<'a>> {
        todo!()
    }

    fn insert(&mut self) -> Result<Query<'a>, error::Insert<'a>> {
        todo!()
    }

    fn update(&mut self) -> Result<Query<'a>, error::Update<'a>> {
        todo!()
    }

    fn delete(&mut self) -> Result<Query<'a>, error::Delete<'a>> {
        todo!()
    }

    fn table_ref(&mut self) -> Result<TableRef<'a>, error::TableRef> {
        todo!()
    }

    /// `^` + postfix parsed with `with_query(false, …)` so `^select` and `^{ a, b }` work.
    /// The operand is the whole postfix chain (`^user.id` pins `user.id`; §10.20).
    pub(crate) fn pinned_value(&mut self) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }
}

/// Snapshot test macro for successful query parsing.
#[cfg(test)]
#[allow(unused)]
macro_rules! assert_query_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let result = parser
            .expression()
            .unwrap_or_else(|e| panic!("expected Ok, got Err: {e:#?}\n\nSource:\n{code}"));
        assert!(
            parser.is_eof(),
            "unconsumed input at {:?}\n\nSource:\n{code}",
            parser.position()
        );
        insta::with_settings!({
            description => code,
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }};
}

/// Snapshot test macro for query parse errors.
#[cfg(test)]
#[allow(unused)]
macro_rules! assert_query_error_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let err = parser
            .expression()
            .err()
            .unwrap_or_else(|| panic!("expected Err, got Ok\n\nSource:\n{code}"));
        insta::with_settings!({
            description => code,
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(err);
        });
    }};
}

#[cfg(test)]
#[allow(unused)]
pub(crate) use assert_query_error_snapshot;
#[cfg(test)]
#[allow(unused)]
pub(crate) use assert_query_snapshot;

#[cfg(test)]
mod tests {}
