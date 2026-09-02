//! `type Name[params] = Type` aliases, opaque `type Name`, and `[a, b]` parameter lists.
//!
//! Grammar (SPEC.md):
//!
//! ```text
//! type_alias  = 'type' upper_ident [ type_params ] '=' type ;
//! extern_type = 'type' upper_ident ;                       (* requires #[extern] *)
//! type_params = '[' lower_ident { ',' lower_ident } ']' ;
//! ```
//!
//! A `type Name` with no `=` is `ItemKind::OpaqueType` (§10.26); the
//! `#[extern]` requirement is canonicalization's. The `[params]` and `=`
//! are looked ahead past whitespace with `save_state` / `restore_state`,
//! so an opaque type leaves the cursor at the end of its name. A
//! parameterised type needs a body (`OpaqueType` cannot carry parameters):
//! `type Foo[a]` is `TypeAlias::Body(Type::Start)` at the position where
//! `=` was expected. `type_params` reports `Empty` at the `[` of `[]`.
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/type_alias.rs (Wave 3)

use alder_source::{ItemKind, Name, TypeAlias};
use bumpalo::collections::Vec as BumpVec;

use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// After `type`: TypeAlias or OpaqueType.
    pub(crate) fn type_decl(&mut self) -> Result<ItemKind<'a>, error::TypeAlias<'a>> {
        self.chomp();
        let name = self.located_upper(error::TypeAlias::Name)?;

        let saved = self.save_state();
        self.chomp();
        let params = if self.peek() == Some(b'[') {
            let params = self.specialize(
                |bump, e, row, col| error::TypeAlias::Params(bump.alloc(e), row, col),
                |p| p.type_params(),
            )?;
            self.chomp();
            params
        } else {
            &[]
        };

        if self.peek() == Some(b'=') && !matches!(self.peek_at(1), Some(b'=' | b'>')) {
            self.advance();
            self.chomp();
            let typ = self.specialize(
                |bump, e, row, col| error::TypeAlias::Body(bump.alloc(e), row, col),
                |p| p.type_expr(),
            )?;
            return Ok(ItemKind::TypeAlias(self.alloc(TypeAlias {
                name,
                params,
                typ,
            })));
        }
        if params.is_empty() {
            self.restore_state(saved);
            return Ok(ItemKind::OpaqueType(name));
        }
        let (row, col) = self.position();
        Err(error::TypeAlias::Body(
            self.alloc(error::Type::Start(row, col)),
            row,
            col,
        ))
    }

    /// Expects `[` (else TypeParams::Open); `type`/`enum` peek for `[` first, `trait` calls unconditionally.
    pub(crate) fn type_params(&mut self) -> Result<&'a [Name<'a>], error::TypeParams> {
        let (open_row, open_col) = self.position();
        self.word1(b'[', error::TypeParams::Open)?;
        self.chomp();
        if self.peek() == Some(b']') {
            return Err(error::TypeParams::Empty(open_row, open_col));
        }
        let mut params = BumpVec::new_in(self.bump);
        loop {
            params.push(self.located_lower(error::TypeParams::Var)?);
            self.chomp();
            match self.peek() {
                Some(b',') => {
                    self.advance();
                    self.chomp();
                    if self.peek() == Some(b']') {
                        self.advance();
                        break;
                    }
                }
                Some(b']') => {
                    self.advance();
                    break;
                }
                _ => {
                    let (row, col) = self.position();
                    return Err(error::TypeParams::End(row, col));
                }
            }
        }
        Ok(params.into_bump_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{assert_item_error_snapshot, assert_item_snapshot};

    #[test]
    fn alias_simple() {
        assert_item_snapshot!("type Id = Number");
    }

    #[test]
    fn alias_params() {
        assert_item_snapshot!("type Pair[a, b] = (a, b)");
    }

    #[test]
    fn alias_record() {
        assert_item_snapshot!(
            r#"
            type User = {
                name: String,
                email: String,
            }
            "#
        );
    }

    #[test]
    fn alias_fn() {
        assert_item_snapshot!("type Handler = fn(Request) -> Task[Response]");
    }

    #[test]
    fn alias_params_trailing_comma() {
        assert_item_snapshot!("type Pair[a, b,] = (a, b)");
    }

    #[test]
    fn opaque_type() {
        assert_item_snapshot!("type Response");
    }

    #[test]
    fn opaque_type_with_attr() {
        assert_item_snapshot!(
            r#"
            #[extern]
            type Response
            "#
        );
    }

    #[test]
    fn error_no_name() {
        assert_item_error_snapshot!("type id = Number");
    }

    #[test]
    fn error_alias_no_body() {
        assert_item_error_snapshot!("type Id =");
    }

    #[test]
    fn error_alias_params_no_body() {
        assert_item_error_snapshot!("type Pair[a, b]");
    }

    #[test]
    fn error_alias_bad_body() {
        assert_item_error_snapshot!("type Id = 42");
    }

    #[test]
    fn error_params_unclosed() {
        assert_item_error_snapshot!("type Pair[a, b = (a, b)");
    }

    #[test]
    fn error_params_empty() {
        assert_item_error_snapshot!("type Pair[] = Number");
    }

    #[test]
    fn error_params_uppercase() {
        assert_item_error_snapshot!("type Pair[A] = A");
    }
}
