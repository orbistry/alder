//! Expressions: the flat binop chain, unary prefixes, the postfix loop and
//! the `primary` dispatch table (docs/parser-internals.md §6.0).
//!
//! See docs/parser-internals.md §5.13.
// OWNER: expression/mod.rs (Wave 2)

mod array;
mod if_;
mod lambda;
mod literal;
mod loop_;
mod match_;
mod path;
mod postfix;
mod record;
mod tag;
mod tuple;

use alder_region::Located;
use alder_source::{BinOp, Expr};

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// Flat binop chain over `unary`. Chomps trailing whitespace.
    pub fn expression(&mut self) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }

    /// `-` / `!` / (query mode) `^` prefix, then `postfix`. A `Start` failure of the
    /// operand becomes `Expr::Unary`; every other operand error propagates (§6.0).
    pub(crate) fn unary(&mut self) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }

    /// `primary` then the postfix loop (§6.0). Chomps trailing whitespace.
    pub(crate) fn postfix(&mut self) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }

    /// Dispatch table on the first byte/word. Does NOT chomp.
    pub(crate) fn primary(&mut self) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        todo!()
    }
}

/// May `op` sit at the start of a continuation line? (`-` only if followed by
/// whitespace; `<` only if not followed by a letter or `>`; everything else yes.)
#[allow(unused)]
fn continues_line(op: BinOp, next: Option<u8>) -> bool {
    todo!()
}

/// Snapshot test macro for successful expression parsing.
#[cfg(test)]
#[allow(unused)]
macro_rules! assert_expression_snapshot {
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

/// Snapshot test macro for expression parse errors.
#[cfg(test)]
#[allow(unused)]
macro_rules! assert_expression_error_snapshot {
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
pub(crate) use assert_expression_error_snapshot;
#[cfg(test)]
#[allow(unused)]
pub(crate) use assert_expression_snapshot;

#[cfg(test)]
mod tests {}
