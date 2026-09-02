//! `type Name[params] = Type` aliases, opaque `type Name`, and `[a, b]` parameter lists.
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/type_alias.rs (Wave 3)

use alder_source::{ItemKind, Name};

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `type`: TypeAlias or OpaqueType.
    pub(crate) fn type_decl(&mut self) -> Result<ItemKind<'a>, error::TypeAlias<'a>> {
        todo!()
    }

    /// Expects `[` (else TypeParams::Open); `type`/`enum` peek for `[` first, `trait` calls unconditionally.
    pub(crate) fn type_params(&mut self) -> Result<&'a [Name<'a>], error::TypeParams> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
