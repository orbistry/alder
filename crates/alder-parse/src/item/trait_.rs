//! `trait` declarations.
//!
//! See docs/parser-internals.md §5.11 and §10.38.
//!
//! Grammar (SPEC.md "Items"):
//!
//! ```ebnf
//! trait_decl = 'trait' upper_ident type_params [ where_clause ] '{' { trait_item } '}' ;
//! trait_item = 'type' upper_ident
//!            | 'fn' lower_ident '(' [ params ] ')' [ '->' type ] [ where_clause ] [ block ] ;
//! ```
//!
//! `type_params` is required (`trait Show {` → `Trait::Params(TypeParams::Open)`).
//! Body items follow the item-separation rule (§2.1 rule 3): the item after
//! an item must be `}` or start on a later line, else `Trait::SameLine`; a
//! `;` where an item should start is `Trait::Semicolon` (language.md's
//! one-line `trait Iterator[i] { type Item; fn next(it: i) -> Option[Item] }`
//! hits it first). Anything that is not `type`, `fn` or `}` is
//! `Trait::Item`, checked before the same-line rule so a stray token is
//! never reported as a second item; EOF inside the body is `Trait::Item`
//! too, exactly as `Block::End` covers an unclosed block (`Trait` has no
//! dedicated unclosed-body variant, so the M2 renderer special-cases EOF
//! for the message). `type Item = …` is
//! `Trait::AssocTypeHasBody` at the `=`; an `fn` item is `fn_decl` with an
//! optional body (`None` = required, `Some` = default).
//!
//! Conventions: `trait_decl` runs after the `trait` keyword and stops right
//! after the closing `}` without chomping; `item()` chomps.
// OWNER: item/trait_.rs (Wave 3)

use alder_region::Position;
use alder_source::{TraitDecl, TraitItem};
use bumpalo::collections::Vec as BumpVec;

use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// After `trait`. `type_params` is required (missing `[` → Trait::Params(TypeParams::Open)).
    /// Body items are line-break separated (Trait::SameLine); a `;` after an item → Trait::Semicolon.
    pub(crate) fn trait_decl(&mut self) -> Result<&'a TraitDecl<'a>, error::Trait<'a>> {
        self.chomp();
        let name = self.located_upper(error::Trait::Name)?;
        self.chomp();
        let params = self.specialize(
            |bump, e, row, col| error::Trait::Params(bump.alloc(e), row, col),
            |p| p.type_params(),
        )?;
        self.chomp();
        let where_clause = if self.peek_keyword(b"where") {
            self.advance_by(5);
            self.specialize(
                |bump, e, row, col| error::Trait::Where(bump.alloc(e), row, col),
                |p| p.where_clause(),
            )?
        } else {
            &[]
        };
        self.word1(b'{', error::Trait::Open)?;
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
                Some(b';') => return Err(error::Trait::Semicolon(row, col)),
                _ => {}
            }
            let word = self.peek_word();
            if word != "type" && word != "fn" {
                return Err(error::Trait::Item(row, col));
            }
            if last_end.is_some_and(|end| !self.newline_since(end)) {
                return Err(error::Trait::SameLine(row, col));
            }
            let (item, end) = if word == "type" {
                self.advance_by(4);
                self.chomp();
                let name = self.located_upper(error::Trait::AssocType)?;
                self.chomp();
                if self.peek() == Some(b'=') {
                    let (row, col) = self.position();
                    return Err(error::Trait::AssocTypeHasBody(row, col));
                }
                (TraitItem::AssocType(name), name.region.end)
            } else {
                let (decl, end) = self.specialize(
                    |bump, e, row, col| error::Trait::Fn(bump.alloc(e), row, col),
                    |p| {
                        p.advance_by(2); // `fn`
                        p.fn_decl_with_end()
                    },
                )?;
                self.chomp();
                (TraitItem::Fn(decl), end)
            };
            items.push(item);
            last_end = Some(end);
        }
        Ok(self.alloc(TraitDecl {
            name,
            params,
            where_clause,
            items: items.into_bump_slice(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::assert_item_snapshot;

    // Deviation from §7.1, following item/fn_.rs: the pair below drives
    // `trait_decl()` directly (the input starts at the `trait` keyword, which
    // the macro consumes) so the §7.2 tests run before `item()` lands. The
    // `pub` form goes through `item()` and stays ignored until item/mod.rs
    // lands. Wave 4 decides whether to keep or fold them; recorded for §10.
    //
    // Every trait goes through `type_params()` (item/type_alias.rs), so all
    // direct tests wait for that file; their snapshots were generated and
    // verified against the real `type_params()` from the item/type_alias.rs
    // branch (applied locally, not committed): `Open` at the cursor after the
    // caller's chomp, `Empty` at the `[`, `Var` / `End` at the offending
    // byte, stops after `]`.

    /// Snapshot test macro for a successful `trait_decl()` parse (input starts at `trait`).
    macro_rules! assert_trait_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            if let Err((row, col)) = parser.keyword(b"trait", |row, col| (row, col)) {
                panic!("input must start with `trait` ({row}:{col})\n\nSource:\n{code}");
            }
            let result = parser
                .trait_decl()
                .unwrap_or_else(|e| panic!("expected Ok, got Err: {e:#?}\n\nSource:\n{code}"));
            // `trait_decl()` stops at the `}`; `item()` chomps.
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

    /// Snapshot test macro for a `trait_decl()` parse error (input starts at `trait`).
    macro_rules! assert_trait_error_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            if let Err((row, col)) = parser.keyword(b"trait", |row, col| (row, col)) {
                panic!("input must start with `trait` ({row}:{col})\n\nSource:\n{code}");
            }
            let err = parser
                .trait_decl()
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

    /// language.md "Traits" (`pub` dropped: the direct macro starts at `trait`).
    #[test]
    fn trait_single_fn() {
        assert_trait_snapshot!(
            r#"
            trait Show[a] {
                fn show(value: a) -> String
            }
        "#
        );
    }

    /// language.md "Traits" with the missing `->` restored (§10.35).
    #[test]
    fn trait_hkt() {
        assert_trait_snapshot!(
            r#"
            trait Functor[f] {
                fn map(fa: f[a], g: fn(a) -> b) -> f[b]
            }
        "#
        );
    }

    #[test]
    fn trait_default_body() {
        assert_trait_snapshot!(
            r#"
            trait Show[a] {
                fn show(value: a) -> String { "?" }
            }
        "#
        );
    }

    #[test]
    fn trait_assoc_type() {
        assert_trait_snapshot!(
            r#"
            trait Container[c] {
                type Item
            }
        "#
        );
    }

    /// language.md's one-line `Iterator` example rewritten one item per line (§10.35).
    #[test]
    fn trait_assoc_type_and_fn() {
        assert_trait_snapshot!(
            r#"
            trait Iterator[i] {
                type Item
                fn next(it: i) -> Option[Item]
            }
        "#
        );
    }

    /// language.md "Traits": `trait Ord[a] where a: Eq`.
    #[test]
    fn trait_where() {
        assert_trait_snapshot!(
            r#"
            trait Ord[a] where a: Eq {
                fn compare(x: a, y: a) -> Ordering
            }
        "#
        );
    }

    #[test]
    fn trait_where_multiline_trailing_comma() {
        assert_trait_snapshot!(
            r#"
            trait Ord[a]
                where
                    a: Eq,
            {
                fn compare(x: a, y: a) -> Ordering
            }
        "#
        );
    }

    #[test]
    fn trait_multiple_items() {
        assert_trait_snapshot!(
            r#"
            trait Monoid[a] {
                fn empty() -> a
                fn append(x: a, y: a) -> a
                fn concat(xs: Array[a]) -> a { Array.fold(xs, empty(), append) }
            }
        "#
        );
    }

    #[test]
    fn trait_fn_where() {
        assert_trait_snapshot!(
            r#"
            trait Container[c] {
                type Item
                fn sum(xs: c) -> Number where c.Item == Number
            }
        "#
        );
    }

    #[test]
    fn trait_two_params() {
        assert_trait_snapshot!(
            r#"
            trait Convert[a, b] {
                fn convert(x: a) -> b
            }
        "#
        );
    }

    #[test]
    fn trait_empty() {
        assert_trait_snapshot!("trait Marker[a] {}");
    }

    /// language.md "Traits", as written.
    #[test]
    fn trait_pub() {
        assert_item_snapshot!(
            r#"
            pub trait Show[a] {
                fn show(value: a) -> String
            }
        "#
        );
    }

    #[test]
    fn error_no_name() {
        assert_trait_error_snapshot!("trait [a] {}");
    }

    #[test]
    fn error_no_params() {
        assert_trait_error_snapshot!("trait Show {");
    }

    #[test]
    fn error_where() {
        assert_trait_error_snapshot!("trait Ord[a] where a: 1 {}");
    }

    #[test]
    fn error_open() {
        assert_trait_error_snapshot!("trait Show[a] fn");
    }

    #[test]
    fn error_bad_item() {
        assert_trait_error_snapshot!("trait Show[a] { let x = 1 }");
    }

    #[test]
    fn error_unclosed() {
        assert_trait_error_snapshot!(
            r#"
            trait Show[a] {
                fn show(value: a) -> String
        "#
        );
    }

    /// EOF right after `{` is `Trait::Item`, like `Block::End` for a block.
    #[test]
    fn error_unclosed_empty_body() {
        assert_trait_error_snapshot!("trait Show[a] {");
    }

    #[test]
    fn error_assoc_type_name() {
        assert_trait_error_snapshot!("trait Iterator[i] { type item }");
    }

    #[test]
    fn error_assoc_type_has_body() {
        assert_trait_error_snapshot!("trait Iterator[i] { type Item = Number }");
    }

    #[test]
    fn error_fn() {
        assert_trait_error_snapshot!("trait Show[a] { fn (value: a) -> String }");
    }

    /// language.md's one-line `Iterator` example (§10.35, §10.38).
    #[test]
    fn error_semicolon_between_items() {
        assert_trait_error_snapshot!(
            "trait Iterator[i] { type Item; fn next(it: i) -> Option[Item] }"
        );
    }

    #[test]
    fn error_same_line_items() {
        assert_trait_error_snapshot!("trait Iterator[i] { type Item fn next(it: i) -> Item }");
    }

    #[test]
    fn error_same_line_after_body() {
        assert_trait_error_snapshot!(
            r#"
            trait Show[a] {
                fn show(value: a) -> String { "?" } fn other(value: a) -> String
            }
        "#
        );
    }
}
