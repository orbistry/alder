//! `#[name]` / `#[name(args)]` attributes.
//!
//! Grammar (SPEC.md):
//! `attribute = '#[' lower_ident [ '(' [ expression { ',' expression } ] ')' ] ']' ;`
//!
//! Arguments are ordinary expressions: `#[derive(Show, Eq)]` yields two
//! `Expr::Path`s and `#[extern("m", "n")]` two `Expr::Str`s; what an
//! attribute means is decided later. The name is a `lower_name` (a reserved
//! word such as `#[test]` is `Attribute::Name`). Error positions: `Open` at
//! the `#`, `Name` / `Arg` / `ArgEnd` / `End` at the offending byte,
//! `Dangling` at the EOF or `}` that follows the last attribute.
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/attribute.rs (Wave 3)

use alder_region::Located;
use alder_source::Attribute;
use bumpalo::collections::Vec as BumpVec;

use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// Zero or more attributes, each followed by whitespace.
    pub(crate) fn attributes(
        &mut self,
    ) -> Result<&'a [Located<Attribute<'a>>], error::Attribute<'a>> {
        let mut attributes = BumpVec::new_in(self.bump);
        while self.peek() == Some(b'#') {
            attributes.push(self.attribute()?);
            self.chomp();
        }
        if !attributes.is_empty() && matches!(self.peek(), None | Some(b'}')) {
            let (row, col) = self.position();
            return Err(error::Attribute::Dangling(row, col));
        }
        Ok(attributes.into_bump_slice())
    }

    /// At `#`.
    pub(crate) fn attribute(&mut self) -> Result<Located<Attribute<'a>>, error::Attribute<'a>> {
        let start = self.get_position();
        let (row, col) = self.position();
        if self.peek() != Some(b'#') || self.peek_at(1) != Some(b'[') {
            return Err(error::Attribute::Open(row, col));
        }
        self.advance_by(2);
        self.chomp();
        let name = self.located_lower(error::Attribute::Name)?;
        self.chomp();
        let args = if self.peek() == Some(b'(') {
            self.attribute_args()?
        } else {
            &[]
        };
        self.word1(b']', error::Attribute::End)?;
        Ok(self.located(start, Attribute { name, args }))
    }

    /// At `(`: comma-separated expressions through `)`, then trailing whitespace.
    fn attribute_args(
        &mut self,
    ) -> Result<&'a [&'a Located<alder_source::Expr<'a>>], error::Attribute<'a>> {
        self.advance();
        self.chomp();
        let mut args = BumpVec::new_in(self.bump);
        loop {
            if self.peek() == Some(b')') {
                self.advance();
                break;
            }
            args.push(self.specialize(
                |bump, e, row, col| error::Attribute::Arg(bump.alloc(e), row, col),
                |p| p.expression(),
            )?);
            match self.peek() {
                Some(b',') => {
                    self.advance();
                    self.chomp();
                }
                Some(b')') => {
                    self.advance();
                    break;
                }
                _ => {
                    let (row, col) = self.position();
                    return Err(error::Attribute::ArgEnd(row, col));
                }
            }
        }
        self.chomp();
        Ok(args.into_bump_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{assert_item_error_snapshot, assert_item_snapshot};

    #[test]
    fn attr_bare() {
        assert_item_snapshot!(
            r#"
            #[extern]
            type Response
            "#
        );
    }

    #[test]
    fn attr_args() {
        assert_item_snapshot!(
            r#"
            #[extern("node:crypto", "randomUUID")]
            type Uuid
            "#
        );
    }

    #[test]
    fn attr_derive() {
        assert_item_snapshot!(
            r#"
            #[derive(Show, Eq, Json)]
            type Point = { x: Number, y: Number }
            "#
        );
    }

    #[test]
    fn attr_multiple() {
        assert_item_snapshot!(
            r#"
            #[derive(Show)]
            #[durable_object]
            type Counter = { count: Number }
            "#
        );
    }

    #[test]
    fn attr_empty_args() {
        assert_item_snapshot!(
            r#"
            #[derive()]
            type Unit
            "#
        );
    }

    #[test]
    fn attr_trailing_comma() {
        assert_item_snapshot!(
            r#"
            #[derive(Show, Eq,)]
            type Unit
            "#
        );
    }

    #[test]
    fn error_attr_open() {
        assert_item_error_snapshot!("# derive");
    }

    #[test]
    fn error_attr_unclosed() {
        assert_item_error_snapshot!("#[derive(Show) type X");
    }

    #[test]
    fn error_attr_arg_end() {
        assert_item_error_snapshot!("#[derive(Show Eq)] type X");
    }

    #[test]
    fn error_attr_arg() {
        assert_item_error_snapshot!("#[derive(Show, ])] type X");
    }

    #[test]
    fn error_attr_name() {
        assert_item_error_snapshot!("#[Derive] type X");
    }

    #[test]
    fn error_attr_name_reserved() {
        assert_item_error_snapshot!("#[test] fn f() {}");
    }

    #[test]
    fn error_attr_dangling() {
        assert_item_error_snapshot!("#[extern]");
    }
}
