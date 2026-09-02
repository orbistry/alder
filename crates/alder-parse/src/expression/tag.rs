//! `:tag` / `:tag(args)` expressions.
//!
//! The argument list may be separated from the name by whitespace but must
//! start on the same line, exactly as `pattern/ctor.rs` reads `:tag(p)`,
//! and takes at least one argument (`:tag()` is `Tag::Arg(Start)` at the
//! `)`). A `(` on a later line is a new statement (§2.1 rule 1). Arguments
//! are parsed with record constructors re-enabled (§2.3).
//!
//! Errors are `Expr::Tag` at the tag's `:`, including `Tag::Name` for a `:`
//! not followed by a lowercase letter (raised by `primary`).
//!
//! See docs/parser-internals.md §5.13.
// OWNER: expression/tag.rs (Wave 2)

use alder_region::{Located, Position};
use alder_source::Expr;
use bumpalo::collections::Vec as BumpVec;

use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// At `:` followed by a lowercase letter.
    pub(crate) fn tag(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let bump = self.bump;
        let name = self.tag_name(error::Expr::Start, |row, col| {
            error::Expr::Tag(bump.alloc(error::Tag::Name(row, col)), row, col)
        })?;
        let end = self.get_position();
        self.chomp();
        if self.peek() == Some(b'(') && !self.newline_since(end) {
            let args = self.specialize(
                |bump, e, _, _| error::Expr::Tag(bump.alloc(e), start.line, start.column),
                |p| p.with_record_ctor(true, |p| p.tag_args()),
            )?;
            Ok(self.add_end(start, Expr::Tag { name, args }))
        } else {
            Ok(self.expr_at(start, end, Expr::Tag { name, args: &[] }))
        }
    }

    /// At `(`: `( expression { ',' expression } [','] )`, at least one
    /// argument. Consumes the closing `)` and nothing after it.
    fn tag_args(&mut self) -> Result<&'a [&'a Located<Expr<'a>>], error::Tag<'a>> {
        self.advance();
        self.chomp();
        let mut args = BumpVec::new_in(self.bump);
        loop {
            let arg = self.specialize(
                |bump, e, row, col| error::Tag::Arg(bump.alloc(e), row, col),
                |p| p.expression(),
            )?;
            args.push(arg);
            match self.peek() {
                Some(b',') => {
                    self.advance();
                    self.chomp();
                    if self.peek() == Some(b')') {
                        self.advance();
                        break;
                    }
                }
                Some(b')') => {
                    self.advance();
                    break;
                }
                _ => {
                    let (row, col) = self.position();
                    return Err(error::Tag::End(row, col));
                }
            }
        }
        Ok(args.into_bump_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{assert_expression_error_snapshot, assert_expression_snapshot};

    #[test]
    fn tag_bare() {
        assert_expression_snapshot!(":timeout");
    }

    #[test]
    fn tag_with_arg() {
        assert_expression_snapshot!(":not_found(id)");
    }

    #[test]
    fn tag_with_args() {
        assert_expression_snapshot!(":invalid(field, \"reason\")");
    }

    #[test]
    fn tag_in_call() {
        assert_expression_snapshot!("Err(:not_found(id))");
    }

    #[test]
    fn error_tag_no_name() {
        assert_expression_error_snapshot!(":Foo");
    }

    #[test]
    fn error_tag_unclosed() {
        assert_expression_error_snapshot!(":not_found(id");
    }
}
