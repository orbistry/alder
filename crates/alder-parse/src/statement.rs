//! Blocks and statements (docs/parser-internals.md §2.1 rule 3: statements
//! are separated by line breaks, never `;`).
//!
//! See docs/parser-internals.md §5.12.
// OWNER: statement.rs (Wave 2)

use alder_region::{Located, Position};
use alder_source::{Block, Expr, Place, Stmt};

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// At `{`. Always a block. Enforces Block::SameLine; the last `Stmt::Expr`
    /// before `}` becomes `tail`.
    pub fn block(&mut self) -> Result<&'a Located<Block<'a>>, error::Block<'a>> {
        todo!()
    }

    /// One statement; dispatch on let/use/provide/for/while/return/break/continue/assert/`;`,
    /// else `expr_or_assign`.
    pub fn statement(&mut self) -> Result<&'a Located<Stmt<'a>>, error::Stmt<'a>> {
        todo!()
    }

    /// expression, then optional assign_op + value. Shared with lambda bodies.
    pub(crate) fn expr_or_assign(&mut self) -> Result<&'a Located<Stmt<'a>>, error::Stmt<'a>> {
        todo!()
    }

    /// Var followed by Access/TupleAccess/Index steps → Place; otherwise None.
    pub(crate) fn expr_to_place(
        &self,
        expr: &'a Located<Expr<'a>>,
    ) -> Option<&'a Located<Place<'a>>> {
        todo!()
    }

    fn for_stmt(&mut self, start: Position) -> Result<&'a Located<Stmt<'a>>, error::For<'a>> {
        todo!()
    }

    fn while_stmt(&mut self, start: Position) -> Result<&'a Located<Stmt<'a>>, error::While<'a>> {
        todo!()
    }

    fn provide_stmt(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Stmt<'a>>, error::Provide<'a>> {
        todo!()
    }

    /// After `use`. `pub(crate)` (not private as §5.12 shows) because
    /// `markup::directive` dispatches child-block `use` through it (§6.2).
    pub(crate) fn use_stmt(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Stmt<'a>>, error::Stmt<'a>> {
        todo!()
    }

    fn return_stmt(&mut self, start: Position) -> Result<&'a Located<Stmt<'a>>, error::Stmt<'a>> {
        todo!()
    }

    fn break_stmt(&mut self, start: Position) -> Result<&'a Located<Stmt<'a>>, error::Stmt<'a>> {
        todo!()
    }

    fn assert_stmt(&mut self, start: Position) -> Result<&'a Located<Stmt<'a>>, error::Stmt<'a>> {
        todo!()
    }
}

/// Snapshot test macro for successful block parsing.
#[cfg(test)]
#[allow(unused)]
macro_rules! assert_block_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let result = parser
            .block()
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

/// Snapshot test macro for block parse errors.
#[cfg(test)]
#[allow(unused)]
macro_rules! assert_block_error_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let err = parser
            .block()
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
pub(crate) use assert_block_error_snapshot;
#[cfg(test)]
#[allow(unused)]
pub(crate) use assert_block_snapshot;

/// Snapshot test macro for successful statement parsing.
#[cfg(test)]
#[allow(unused)]
macro_rules! assert_statement_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let result = parser
            .statement()
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

/// Snapshot test macro for statement parse errors.
#[cfg(test)]
#[allow(unused)]
macro_rules! assert_statement_error_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let err = parser
            .statement()
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
pub(crate) use assert_statement_error_snapshot;
#[cfg(test)]
#[allow(unused)]
pub(crate) use assert_statement_snapshot;

#[cfg(test)]
mod tests {}
