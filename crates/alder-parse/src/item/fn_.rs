//! `fn` declarations, parameter lists and `where` clauses.
//!
//! See docs/parser-internals.md §5.11.
//!
//! Grammar (SPEC.md "Items"):
//!
//! ```ebnf
//! params       = param { ',' param } [ ',' ] ;
//! param        = [ 'mut' ] pattern [ ':' type ] ;
//! where_clause = 'where' constraint { ',' constraint } [ ',' ] ;
//! constraint   = lower_ident ':' bound { '+' bound } | lower_ident '.' upper_ident '==' type ;
//! bound        = path ;
//! ```
//!
//! Conventions: `params()` consumes through the `)` and does not chomp
//! after it. `where_clause()` stops after the last constraint (or its
//! trailing comma) with trailing whitespace chomped, because its callers
//! must look past whitespace for the body anyway. A `where` followed by
//! something that cannot start a constraint yields an empty clause
//! (§5.11: "may be empty"); after a `,` the clause continues only when a
//! non-reserved lowercase word follows, so a trailing comma before the
//! next item (`where a: Show,` then `fn …` on the next line) is fine.
// OWNER: item/fn_.rs (Wave 3; `params` / `where_clause` may land in Wave 2, see §9 step 2.4)

use alder_region::Region;
use alder_source::{Constraint, FnDecl, Param};
use bumpalo::collections::Vec as BumpVec;

use crate::keyword::is_reserved;
use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `fn`; body optional.
    pub(crate) fn fn_decl(&mut self) -> Result<&'a FnDecl<'a>, error::Fn<'a>> {
        todo!()
    }
}

impl<'a> Parser<'a> {
    /// At `(`; shared by lambda/component. Consumes through the `)`.
    pub(crate) fn params(&mut self) -> Result<&'a [Param<'a>], error::Params<'a>> {
        self.word1(b'(', error::Params::Open)?;
        self.chomp();
        let mut params = BumpVec::new_in(self.bump);
        loop {
            if self.peek() == Some(b')') {
                self.advance();
                break;
            }
            params.push(self.param()?);
            // `pattern()` and `type_expr()` chomp, so the cursor is on the
            // next token.
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
                    return Err(error::Params::End(row, col));
                }
            }
        }
        Ok(params.into_bump_slice())
    }

    /// `[mut] pattern [: type]`.
    fn param(&mut self) -> Result<Param<'a>, error::Params<'a>> {
        let mutable = if self.peek_keyword(b"mut") {
            let start = self.get_position();
            self.advance_by(3);
            let region = Region::new(start, self.get_position());
            self.chomp();
            Some(region)
        } else {
            None
        };
        let pattern = self.specialize(
            |bump, e, row, col| error::Params::Pattern(bump.alloc(e), row, col),
            |p| p.pattern(),
        )?;
        let annotation = if self.peek() == Some(b':') {
            self.advance();
            self.chomp();
            Some(self.specialize(
                |bump, e, row, col| error::Params::Type(bump.alloc(e), row, col),
                |p| p.type_expr(),
            )?)
        } else {
            None
        };
        Ok(Param {
            mutable,
            pattern,
            annotation,
        })
    }
}

// Called by `fn_decl`, `trait_decl` and `impl_decl` (Wave 3); the allow goes
// away with the Wave 4 sweep (docs/parser-internals.md §9 step 4.2).
#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `where`; may be empty.
    pub(crate) fn where_clause(&mut self) -> Result<&'a [Constraint<'a>], error::Where<'a>> {
        self.chomp();
        let mut constraints = BumpVec::new_in(self.bump);
        // The first constraint only needs a lowercase letter: a reserved word
        // there (`where type: Show`) is reported as `Where::Var`.
        let mut expect = self.peek_lower();
        while expect {
            constraints.push(self.constraint()?);
            self.chomp();
            if self.peek() != Some(b',') {
                break;
            }
            self.advance();
            self.chomp();
            expect = self.peek_lower() && !is_reserved(self.peek_word());
        }
        Ok(constraints.into_bump_slice())
    }

    /// `a: Show + Eq` or `i.Item == Number`.
    fn constraint(&mut self) -> Result<Constraint<'a>, error::Where<'a>> {
        let var = self.located_lower(error::Where::Var)?;
        self.chomp();
        match self.peek() {
            Some(b':') => {
                self.advance();
                self.chomp();
                let mut bounds = BumpVec::new_in(self.bump);
                bounds.push(self.path(error::Where::Bound, error::Where::Bound)?);
                self.chomp();
                while self.peek() == Some(b'+') {
                    self.advance();
                    self.chomp();
                    bounds.push(self.path(error::Where::Bound, error::Where::Bound)?);
                    self.chomp();
                }
                Ok(Constraint::Bound {
                    var,
                    bounds: bounds.into_bump_slice(),
                })
            }
            Some(b'.') => {
                self.advance();
                let assoc = self.located_upper(error::Where::AssocName)?;
                self.chomp();
                self.word2(b'=', b'=', error::Where::AssocEq)?;
                self.chomp();
                let typ = self.specialize(
                    |bump, e, row, col| error::Where::Type(bump.alloc(e), row, col),
                    |p| p.type_expr(),
                )?;
                Ok(Constraint::AssocEq { var, assoc, typ })
            }
            _ => {
                let (row, col) = self.position();
                Err(error::Where::Colon(row, col))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{assert_item_error_snapshot, assert_item_snapshot};

    /// Snapshot test macro for a successful `params()` parse (input starts at `(`).
    macro_rules! assert_params_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            let result = parser
                .params()
                .unwrap_or_else(|e| panic!("expected Ok, got Err: {e:#?}\n\nSource:\n{code}"));
            // `params()` stops at the `)`; its callers chomp.
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

    /// Snapshot test macro for a `params()` parse error (input starts at `(`).
    macro_rules! assert_params_error_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            let err = parser
                .params()
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

    /// Snapshot test macro for a successful `where_clause()` parse (input
    /// starts right after `where`).
    macro_rules! assert_where_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            let result = parser
                .where_clause()
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

    /// Snapshot test macro for a `where_clause()` parse error (input starts
    /// right after `where`).
    macro_rules! assert_where_error_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            let err = parser
                .where_clause()
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

    // ---- params() directly -------------------------------------------------

    #[test]
    fn params_empty() {
        assert_params_snapshot!("()");
    }

    #[test]
    fn params_single() {
        assert_params_snapshot!("(x)");
    }

    #[test]
    fn params_multiple() {
        assert_params_snapshot!("(a, b, c)");
    }

    #[test]
    fn params_typed() {
        assert_params_snapshot!("(a: Number, b: String)");
    }

    #[test]
    fn params_mut() {
        assert_params_snapshot!("(mut count: Number)");
    }

    #[test]
    fn params_pattern() {
        assert_params_snapshot!("((a, b), { x, y })");
    }

    #[test]
    fn params_fn_type() {
        assert_params_snapshot!("(xs: Array[a], g: fn(a) -> b)");
    }

    #[test]
    fn params_trailing_comma() {
        assert_params_snapshot!("(a, b,)");
    }

    #[test]
    fn params_multiline() {
        assert_params_snapshot!(
            r#"
            (
                a: Number,
                b: String,
            )
        "#
        );
    }

    #[test]
    fn error_params_open() {
        assert_params_error_snapshot!("x)");
    }

    #[test]
    fn error_params_pattern() {
        assert_params_error_snapshot!("(+)");
    }

    #[test]
    fn error_params_reserved() {
        assert_params_error_snapshot!("(type)");
    }

    #[test]
    fn error_params_type() {
        assert_params_error_snapshot!("(a: )");
    }

    #[test]
    fn error_params_end() {
        assert_params_error_snapshot!("(a b)");
    }

    #[test]
    fn error_params_unclosed_direct() {
        assert_params_error_snapshot!("(a, b");
    }

    // ---- where_clause() directly -------------------------------------------

    #[test]
    fn where_single() {
        assert_where_snapshot!(" a: Show");
    }

    #[test]
    fn where_multi() {
        assert_where_snapshot!(" a: Show, k: Hash");
    }

    #[test]
    fn where_plus() {
        assert_where_snapshot!(" k: Eq + Hash");
    }

    #[test]
    fn where_qualified_bound() {
        assert_where_snapshot!(" a: Std::Show");
    }

    #[test]
    fn where_assoc() {
        assert_where_snapshot!(" i.Item == Number");
    }

    #[test]
    fn where_multiline_trailing_comma() {
        assert_where_snapshot!(
            r#"

                t: Traversable,
                f: Applicative,
        "#
        );
    }

    #[test]
    fn where_empty() {
        assert_where_snapshot!("");
    }

    #[test]
    fn error_where_var_reserved() {
        assert_where_error_snapshot!(" type: Show");
    }

    #[test]
    fn error_where_colon() {
        assert_where_error_snapshot!(" a Show");
    }

    #[test]
    fn error_where_bound_direct() {
        assert_where_error_snapshot!(" a: 1");
    }

    #[test]
    fn error_where_bound_after_plus() {
        assert_where_error_snapshot!(" a: Show +");
    }

    #[test]
    fn error_where_assoc_name() {
        assert_where_error_snapshot!(" i.item == Number");
    }

    #[test]
    fn error_where_assoc_eq() {
        assert_where_error_snapshot!(" i.Item = Number");
    }

    #[test]
    fn error_where_assoc_type() {
        assert_where_error_snapshot!(" i.Item == )");
    }

    // ---- through `item()` (§7.2 names) -------------------------------------

    #[test]
    #[ignore = "waits for item/mod.rs"]
    fn fn_params() {
        assert_item_snapshot!("fn add(a, b) { a + b }");
    }

    #[test]
    #[ignore = "waits for item/mod.rs"]
    fn fn_typed_params() {
        assert_item_snapshot!("fn add(a: Number, b: Number) -> Number { a + b }");
    }

    #[test]
    #[ignore = "waits for item/mod.rs"]
    fn fn_mut_param() {
        assert_item_snapshot!("fn bump(mut n: Number) { n += 1 }");
    }

    #[test]
    #[ignore = "waits for item/mod.rs"]
    fn fn_pattern_param() {
        assert_item_snapshot!("fn swap((a, b)) { (b, a) }");
    }

    #[test]
    #[ignore = "waits for item/mod.rs"]
    fn fn_trailing_comma_params() {
        assert_item_snapshot!(
            r#"
            fn add(
                a: Number,
                b: Number,
            ) -> Number {
                a + b
            }
        "#
        );
    }

    #[test]
    #[ignore = "waits for item/mod.rs"]
    fn fn_where_single() {
        assert_item_snapshot!(
            r#"
            fn describe(xs: Array[a]) -> String where a: Show {
                xs |> Array.map(show) |> String.join(", ")
            }
        "#
        );
    }

    #[test]
    #[ignore = "waits for item/mod.rs"]
    fn fn_where_multi() {
        assert_item_snapshot!(
            r#"
            fn show2(a: a, b: b) -> String where a: Show, b: Show {
                show(a)
            }
        "#
        );
    }

    #[test]
    #[ignore = "waits for item/mod.rs"]
    fn fn_where_plus() {
        assert_item_snapshot!(
            r#"
            fn lookup(cache: Cache[k, v], key: k) -> Option[v]
                where k: Eq + Hash
        "#
        );
    }

    #[test]
    #[ignore = "waits for item/mod.rs"]
    fn fn_where_assoc() {
        assert_item_snapshot!(
            r#"
            fn sum(it: i) -> Number where i: Iterator, i.Item == Number {
                0
            }
        "#
        );
    }

    #[test]
    #[ignore = "waits for item/mod.rs"]
    fn fn_where_multiline_trailing_comma() {
        assert_item_snapshot!(
            r#"
            fn traverse(xs: t[f[a]], g: fn(a) -> f[b]) -> f[t[b]]
                where
                    t: Traversable,
                    f: Applicative,
        "#
        );
    }

    #[test]
    #[ignore = "waits for item/mod.rs"]
    fn error_params_unclosed() {
        assert_item_error_snapshot!("fn add(a, b { a }");
    }

    #[test]
    #[ignore = "waits for item/mod.rs"]
    fn error_where_bad_bound() {
        assert_item_error_snapshot!("fn f(x: a) where a: 1 { x }");
    }
}
