//! `impl` declarations.
//!
//! See docs/parser-internals.md §5.11 and §10.38.
//!
//! Grammar (SPEC.md "Items", with §10.8's trailing commas):
//!
//! ```ebnf
//! impl_decl = 'impl' path '[' type { ',' type } [ ',' ] ']' [ where_clause ] '{' { impl_item } '}' ;
//! impl_item = 'type' upper_ident '=' type | fn_decl ;
//! ```
//!
//! The trait is a `path` (`Show`, `Std::Show`); a dangling `::` is reported
//! as `Impl::Trait` too, since `Impl` has no member variant. The `[` is
//! required (`impl Show {` → `Impl::Open`), and `[]` is
//! `Impl::Arg(Type::Start)`. Body items follow the item-separation rule
//! (§2.1 rule 3) exactly as in `trait_decl`: `Impl::SameLine`,
//! `Impl::Semicolon`, and `Impl::Item` for anything but `type`, `fn` or `}`.
//! `type Item` without `=` is `Impl::AssocEquals`.
//!
//! Conventions: `impl_decl` runs after the `impl` keyword and stops right
//! after the closing `}` without chomping; `item()` chomps.
// OWNER: item/impl_.rs (Wave 3)

use alder_region::Position;
use alder_source::{ImplDecl, ImplItem};
use bumpalo::collections::Vec as BumpVec;

use crate::{Parser, error};

// Called by `item()` (item/mod.rs, Wave 3); the allow goes away with the
// Wave 4 sweep (docs/parser-internals.md §9 step 4.2).
#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `impl`. Body items are line-break separated (Impl::SameLine); a `;` after an item → Impl::Semicolon.
    pub(crate) fn impl_decl(&mut self) -> Result<&'a ImplDecl<'a>, error::Impl<'a>> {
        self.chomp();
        let trait_ = self.path(error::Impl::Trait, error::Impl::Trait)?;
        self.chomp();
        self.word1(b'[', error::Impl::Open)?;
        self.chomp();
        let mut args = BumpVec::new_in(self.bump);
        loop {
            let arg = self.specialize(
                |bump, e, row, col| error::Impl::Arg(bump.alloc(e), row, col),
                |p| p.type_expr(),
            )?;
            args.push(arg);
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
                    return Err(error::Impl::ArgEnd(row, col));
                }
            }
        }
        self.chomp();
        let where_clause = if self.peek_keyword(b"where") {
            self.advance_by(5);
            self.specialize(
                |bump, e, row, col| error::Impl::Where(bump.alloc(e), row, col),
                |p| p.where_clause(),
            )?
        } else {
            &[]
        };
        self.word1(b'{', error::Impl::BodyOpen)?;
        self.chomp();
        let mut items = BumpVec::new_in(self.bump);
        // End of the previous item, for the item-separation rule.
        let mut last_end: Option<Position> = None;
        loop {
            let (row, col) = self.position();
            match self.peek() {
                Some(b'}') => {
                    self.advance();
                    break;
                }
                Some(b';') => return Err(error::Impl::Semicolon(row, col)),
                _ => {}
            }
            let word = self.peek_word();
            if word != "type" && word != "fn" {
                return Err(error::Impl::Item(row, col));
            }
            if last_end.is_some_and(|end| !self.newline_since(end)) {
                return Err(error::Impl::SameLine(row, col));
            }
            let (item, end) = if word == "type" {
                self.advance_by(4);
                self.chomp();
                let name = self.located_upper(error::Impl::AssocType)?;
                self.chomp();
                if self.peek() != Some(b'=') || matches!(self.peek_at(1), Some(b'=' | b'>')) {
                    let (row, col) = self.position();
                    return Err(error::Impl::AssocEquals(row, col));
                }
                self.advance();
                self.chomp();
                let typ = self.specialize(
                    |bump, e, row, col| error::Impl::AssocBody(bump.alloc(e), row, col),
                    |p| p.type_expr(),
                )?;
                (ImplItem::AssocType { name, typ }, typ.region.end)
            } else {
                let (decl, end) = self.specialize(
                    |bump, e, row, col| error::Impl::Fn(bump.alloc(e), row, col),
                    |p| {
                        p.advance_by(2); // `fn`
                        p.fn_decl_with_end()
                    },
                )?;
                self.chomp();
                (ImplItem::Fn(decl), end)
            };
            items.push(item);
            last_end = Some(end);
        }
        Ok(self.alloc(ImplDecl {
            trait_,
            args: args.into_bump_slice(),
            where_clause,
            items: items.into_bump_slice(),
        }))
    }
}

#[cfg(test)]
mod tests {
    // Deviation from §7.1, following item/fn_.rs: the pair below drives
    // `impl_decl()` directly (the input starts at the `impl` keyword, which
    // the macro consumes) so the §7.2 tests run before `item()` lands. Wave 4
    // decides whether to keep or fold them; recorded for §10.

    /// Snapshot test macro for a successful `impl_decl()` parse (input starts at `impl`).
    macro_rules! assert_impl_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            if let Err((row, col)) = parser.keyword(b"impl", |row, col| (row, col)) {
                panic!("input must start with `impl` ({row}:{col})\n\nSource:\n{code}");
            }
            let result = parser
                .impl_decl()
                .unwrap_or_else(|e| panic!("expected Ok, got Err: {e:#?}\n\nSource:\n{code}"));
            // `impl_decl()` stops at the `}`; `item()` chomps.
            parser.chomp();
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

    /// Snapshot test macro for an `impl_decl()` parse error (input starts at `impl`).
    macro_rules! assert_impl_error_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            if let Err((row, col)) = parser.keyword(b"impl", |row, col| (row, col)) {
                panic!("input must start with `impl` ({row}:{col})\n\nSource:\n{code}");
            }
            let err = parser
                .impl_decl()
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

    /// language.md "Traits".
    #[test]
    fn impl_simple() {
        assert_impl_snapshot!(
            r#"
            impl Show[User] {
                fn show(user: User) -> String { user.name }
            }
        "#
        );
    }

    /// language.md "Traits".
    #[test]
    fn impl_hkt() {
        assert_impl_snapshot!(
            r#"
            impl Functor[Option] {
                fn map(fa: Option[a], g: fn(a) -> b) -> Option[b] {
                    match fa {
                        Some(x) => Some(g(x)),
                        None => None,
                    }
                }
            }
        "#
        );
    }

    #[test]
    fn impl_assoc_type() {
        assert_impl_snapshot!(
            r#"
            impl Iterator[Counter] {
                type Item = Number
                fn next(it: Counter) -> Option[Number] { None }
            }
        "#
        );
    }

    /// language.md "Traits": `impl Show[Cache[k, v]] where k: Show, v: Show`.
    #[test]
    fn impl_where() {
        assert_impl_snapshot!(
            r#"
            impl Show[Cache[k, v]] where k: Show, v: Show {
                fn show(cache: Cache[k, v]) -> String { "cache" }
            }
        "#
        );
    }

    #[test]
    fn impl_multiple_fns() {
        assert_impl_snapshot!(
            r#"
            impl Monoid[String] {
                fn empty() -> String { "" }
                fn append(x: String, y: String) -> String { x }
            }
        "#
        );
    }

    #[test]
    fn impl_multiple_args() {
        assert_impl_snapshot!(
            r#"
            impl Convert[Number, String] {
                fn convert(n: Number) -> String { show(n) }
            }
        "#
        );
    }

    #[test]
    fn impl_args_trailing_comma() {
        assert_impl_snapshot!("impl Convert[Number, String,] {}");
    }

    #[test]
    fn impl_qualified_trait() {
        assert_impl_snapshot!("impl Std::Show[User] {}");
    }

    #[test]
    fn impl_empty() {
        assert_impl_snapshot!("impl Marker[User] {}");
    }

    #[test]
    fn impl_bodiless_fn() {
        assert_impl_snapshot!(
            r#"
            impl Show[User] {
                fn show(user: User) -> String
            }
        "#
        );
    }

    #[test]
    fn error_trait() {
        assert_impl_error_snapshot!("impl show[User] {}");
    }

    #[test]
    fn error_trait_dangling_colons() {
        assert_impl_error_snapshot!("impl Show::[User] {}");
    }

    #[test]
    fn error_no_args() {
        assert_impl_error_snapshot!("impl Show {");
    }

    #[test]
    fn error_arg() {
        assert_impl_error_snapshot!("impl Show[1] {}");
    }

    #[test]
    fn error_args_empty() {
        assert_impl_error_snapshot!("impl Show[] {}");
    }

    #[test]
    fn error_arg_end() {
        assert_impl_error_snapshot!("impl Show[User User] {}");
    }

    #[test]
    fn error_where() {
        assert_impl_error_snapshot!("impl Show[User] where a: 1 {}");
    }

    #[test]
    fn error_body_open() {
        assert_impl_error_snapshot!("impl Show[User] fn");
    }

    #[test]
    fn error_bad_item() {
        assert_impl_error_snapshot!("impl Show[User] { let x = 1 }");
    }

    #[test]
    fn error_unclosed() {
        assert_impl_error_snapshot!(
            r#"
            impl Show[User] {
                fn show(user: User) -> String { user.name }
        "#
        );
    }

    #[test]
    fn error_assoc_type_name() {
        assert_impl_error_snapshot!("impl Iterator[Foo] { type item = Number }");
    }

    #[test]
    fn error_assoc_no_type() {
        assert_impl_error_snapshot!("impl Iterator[Foo] { type Item }");
    }

    #[test]
    fn error_assoc_double_equals() {
        assert_impl_error_snapshot!("impl Iterator[Foo] { type Item == Number }");
    }

    #[test]
    fn error_assoc_body() {
        assert_impl_error_snapshot!("impl Iterator[Foo] { type Item = 1 }");
    }

    #[test]
    fn error_fn() {
        assert_impl_error_snapshot!("impl Show[User] { fn show(user: User) -> String { let } }");
    }

    #[test]
    fn error_semicolon_between_items() {
        assert_impl_error_snapshot!(
            "impl Iterator[Foo] { type Item = Number; fn next(it: Foo) -> Option[Number] { None } }"
        );
    }

    #[test]
    fn error_same_line_items() {
        assert_impl_error_snapshot!(
            r#"
            impl Show[User] {
                fn show(user: User) -> String { user.name } fn other() {}
            }
        "#
        );
    }

    #[test]
    fn error_same_line_after_assoc_type() {
        assert_impl_error_snapshot!("impl Iterator[Foo] { type Item = Number fn next() {} }");
    }
}
