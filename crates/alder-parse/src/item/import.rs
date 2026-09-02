//! `import` items and module paths (`@author/package/seg`, `~/seg`).
//!
//! Grammar (SPEC.md, language.md "Modules"):
//!
//! ```text
//! import       = 'import' module_path [ 'as' lower_ident | '.' import_names ] ;
//! import_names = '{' import_name { ',' import_name } [ ',' ] '}' | '*' ;
//! import_name  = ( lower_ident | upper_ident ) [ 'as' ( lower_ident | upper_ident ) ] ;
//! reexport     = 'import' module_path '.' import_names ;      (* after 'pub' *)
//! module_path  = '@' lower_ident '/' lower_ident { '/' lower_ident } | '~' { '/' lower_ident } ;
//! ```
//!
//! A module path is one token: no whitespace between its parts, and every
//! part is a `raw_lower` (§2.4), so `@alder/test` and `~/db/users` parse.
//! The tail is looked ahead past whitespace with `save_state` /
//! `restore_state`, so a bare import leaves the cursor at the end of its
//! path. What a bare import binds is validated here (§10.37): `import ~`
//! has no segment (`Import::RootOnly`, at the `~`) and `import @alder/test`
//! would bind a reserved word (`Import::ReservedBinding(Test)`, at the
//! segment). `pub import` needs `.{ … }` or `.*` (§10.25,
//! `Import::PubNeedsNames` at the position where the tail was expected).
//! `ImportTail::All` carries the region of `.*`.
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/import.rs (Wave 3)

use alder_region::{Located, Position, Region};
use alder_source::{Import, ImportName, ImportTail, ModulePath, ModuleRoot, Name};
use bumpalo::collections::Vec as BumpVec;

use crate::keyword::Keyword;
use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// After `import`. The bare tail (`ImportTail::Module`) is validated here: no
    /// segments (`import ~`) → Import::RootOnly; reserved last segment
    /// (`import @alder/test`) → Import::ReservedBinding(kw).
    pub(crate) fn import(&mut self, is_pub: bool) -> Result<&'a Import<'a>, error::Import<'a>> {
        self.chomp();
        let path = self.specialize(
            |bump, e, row, col| error::Import::Path(bump.alloc(e), row, col),
            |p| p.module_path(),
        )?;

        // Look ahead past whitespace for a tail; a bare import consumes nothing
        // more so the cursor stays at the path's end.
        let saved = self.save_state();
        self.chomp();
        let tail_start = self.position();
        let tail = if self.peek() == Some(b'.') {
            self.advance();
            self.import_tail()?
        } else if self.peek_keyword(b"as") {
            self.advance_by(2);
            self.chomp();
            ImportTail::Alias(self.located_lower(error::Import::Alias)?)
        } else {
            self.restore_state(saved);
            ImportTail::Module
        };

        if is_pub && !matches!(tail, ImportTail::Names(_) | ImportTail::All(_)) {
            let (row, col) = tail_start;
            return Err(error::Import::PubNeedsNames(row, col));
        }
        if let ImportTail::Module = tail {
            let bound = match (path.value.root, path.value.segments.last()) {
                (_, Some(segment)) => *segment,
                (ModuleRoot::Package { package, .. }, None) => package,
                (ModuleRoot::Local(_), None) => {
                    let start = path.region.start;
                    return Err(error::Import::RootOnly(start.line, start.column));
                }
            };
            if let Some(kw) = Keyword::from_word(bound.value) {
                let start = bound.region.start;
                return Err(error::Import::ReservedBinding(kw, start.line, start.column));
            }
        }
        Ok(self.alloc(Import { path, tail }))
    }

    /// After `.`: `{ names }` or `*`.
    fn import_tail(&mut self) -> Result<ImportTail<'a>, error::Import<'a>> {
        match self.peek() {
            Some(b'*') => {
                // The `.` was just consumed: it sits one byte before the cursor.
                let (row, col) = self.position();
                let dot = Position::new(row, col - 1);
                self.advance();
                Ok(ImportTail::All(Region::new(dot, self.get_position())))
            }
            Some(b'{') => {
                self.advance();
                self.chomp();
                Ok(ImportTail::Names(self.import_names()?))
            }
            _ => {
                let (row, col) = self.position();
                Err(error::Import::Tail(row, col))
            }
        }
    }

    /// After `{`: `name [as alias]` list through `}`.
    fn import_names(&mut self) -> Result<&'a [ImportName<'a>], error::Import<'a>> {
        let mut names = BumpVec::new_in(self.bump);
        loop {
            if self.peek() == Some(b'}') {
                self.advance();
                break;
            }
            let name = self.import_name(error::Import::Name)?;
            self.chomp();
            let alias = if self.peek_keyword(b"as") {
                self.advance_by(2);
                self.chomp();
                let alias = self.import_name(error::Import::NameAlias)?;
                self.chomp();
                Some(alias)
            } else {
                None
            };
            names.push(ImportName { name, alias });
            match self.peek() {
                Some(b',') => {
                    self.advance();
                    self.chomp();
                }
                Some(b'}') => {
                    self.advance();
                    break;
                }
                _ => {
                    let (row, col) = self.position();
                    return Err(error::Import::NamesEnd(row, col));
                }
            }
        }
        Ok(names.into_bump_slice())
    }

    /// `lower_ident | upper_ident` — a value or a type / enum name.
    fn import_name(
        &mut self,
        to_error: impl FnOnce(crate::Row, crate::Col) -> error::Import<'a>,
    ) -> Result<Name<'a>, error::Import<'a>> {
        if self.peek_upper() {
            self.located_upper(to_error)
        } else {
            self.located_lower(to_error)
        }
    }

    /// `@author/package { '/' seg }` | `~ { '/' seg }`; author, package and segments via `raw_lower` (§2.4).
    pub(crate) fn module_path(&mut self) -> Result<Located<ModulePath<'a>>, error::ModulePath> {
        let start = self.get_position();
        let root = match self.peek() {
            Some(b'@') => {
                self.advance();
                let author = self.raw_lower(error::ModulePath::Author)?;
                self.word1(b'/', error::ModulePath::Slash)?;
                let package = self.raw_lower(error::ModulePath::Package)?;
                ModuleRoot::Package { author, package }
            }
            Some(b'~') => {
                self.advance();
                ModuleRoot::Local(Region::new(start, self.get_position()))
            }
            _ => {
                let (row, col) = self.position();
                return Err(error::ModulePath::Start(row, col));
            }
        };
        let mut segments = BumpVec::new_in(self.bump);
        while self.peek() == Some(b'/') {
            self.advance();
            segments.push(self.raw_lower(error::ModulePath::Segment)?);
        }
        Ok(self.located(
            start,
            ModulePath {
                root,
                segments: segments.into_bump_slice(),
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{assert_item_error_snapshot, assert_item_snapshot};

    #[test]
    fn package_root() {
        assert_item_snapshot!("import @alder/http");
    }

    #[test]
    fn package_nested() {
        assert_item_snapshot!("import @alder/http/client");
    }

    #[test]
    fn package_alias() {
        assert_item_snapshot!("import @alder/http as h");
    }

    #[test]
    fn package_names() {
        assert_item_snapshot!("import @alder/http.{ get, Request }");
    }

    #[test]
    fn package_names_alias() {
        assert_item_snapshot!("import @alder/http.{ get as fetch, Request as Req }");
    }

    #[test]
    fn package_all() {
        assert_item_snapshot!("import @alder/http.*");
    }

    #[test]
    fn package_reserved_segment_names() {
        assert_item_snapshot!("import @alder/test.{ fakeDb }");
    }

    #[test]
    fn package_reserved_segment_alias() {
        assert_item_snapshot!("import @alder/test as t");
    }

    #[test]
    fn package_reserved_segment_all() {
        assert_item_snapshot!("import @alder/test.*");
    }

    #[test]
    fn local_root() {
        assert_item_snapshot!("import ~/db");
    }

    #[test]
    fn local_nested() {
        assert_item_snapshot!("import ~/db/users");
    }

    #[test]
    fn local_names() {
        assert_item_snapshot!("import ~/db/users.{ find }");
    }

    #[test]
    fn local_root_only_names() {
        assert_item_snapshot!("import ~.{ config }");
    }

    #[test]
    fn local_root_only_all() {
        assert_item_snapshot!("import ~.*");
    }

    #[test]
    fn local_root_only_alias() {
        assert_item_snapshot!("import ~ as root");
    }

    #[test]
    fn pub_reexport_names() {
        assert_item_snapshot!("pub import ~/leaf.{ someFunc }");
    }

    #[test]
    fn pub_reexport_all() {
        assert_item_snapshot!("pub import ~/leaf.*");
    }

    #[test]
    fn trailing_comma() {
        assert_item_snapshot!(
            r#"
            import @alder/http.{
                get,
                Request,
            }
            "#
        );
    }

    #[test]
    fn names_empty() {
        assert_item_snapshot!("import @alder/http.{}");
    }

    #[test]
    fn error_bad_root() {
        assert_item_error_snapshot!("import http");
    }

    #[test]
    fn error_missing_slash() {
        assert_item_error_snapshot!("import @alder");
    }

    #[test]
    fn error_missing_author() {
        assert_item_error_snapshot!("import @/http");
    }

    #[test]
    fn error_missing_package() {
        assert_item_error_snapshot!("import @alder/");
    }

    #[test]
    fn error_bad_segment() {
        assert_item_error_snapshot!("import ~/db/Users");
    }

    #[test]
    fn error_tail() {
        assert_item_error_snapshot!("import @alder/http.get");
    }

    #[test]
    fn error_alias_uppercase() {
        assert_item_error_snapshot!("import @alder/http as H");
    }

    #[test]
    fn error_name() {
        assert_item_error_snapshot!("import @alder/http.{ 1 }");
    }

    #[test]
    fn error_names_alias_no_name() {
        assert_item_error_snapshot!("import @alder/http.{ x as }");
    }

    #[test]
    fn error_names_end() {
        assert_item_error_snapshot!("import @alder/http.{ get Request }");
    }

    #[test]
    fn error_names_unclosed() {
        assert_item_error_snapshot!("import @alder/http.{ get,");
    }

    #[test]
    fn error_pub_needs_names() {
        assert_item_error_snapshot!("pub import @alder/http");
    }

    #[test]
    fn error_pub_needs_names_alias() {
        assert_item_error_snapshot!("pub import @alder/http as h");
    }

    #[test]
    fn error_reserved_binding() {
        assert_item_error_snapshot!("import @alder/test");
    }

    #[test]
    fn error_reserved_binding_segment() {
        assert_item_error_snapshot!("import ~/db/type");
    }

    #[test]
    fn error_root_only() {
        assert_item_error_snapshot!("import ~");
    }
}
