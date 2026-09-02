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
//!
//! `fn_decl` (after `fn`) reads `name ( params ) [-> type] [where …]` and
//! then a body only when a `{` follows; otherwise the declaration is
//! bodiless (`body: None`, §10.26: extern functions and trait signatures —
//! the `#[extern]` requirement is canonicalization's). The body may start
//! on a later line: nothing else that follows a bodiless declaration in a
//! module, trait or impl body starts with `{`. The cursor is left where the
//! last sub-parser left it (past trailing whitespace after a body or a
//! return type, right after the `)` otherwise). `fn_decl_with_end` also
//! returns the position right after the declaration's last byte, which
//! `trait_decl` / `impl_decl` need for the item-separation rule (§2.1 rule
//! 3) because `block()` and `type_expr()` chomp past it.
// OWNER: item/fn_.rs (Wave 3; `params` / `where_clause` may land in Wave 2, see §9 step 2.4)

use alder_region::{Position, Region};
use alder_source::{Constraint, FnDecl, Param};
use bumpalo::collections::Vec as BumpVec;

use crate::keyword::is_reserved;
use crate::{Parser, error};

// Called by `item()` (item/mod.rs, Wave 3); the allow goes away with the
// Wave 4 sweep (docs/parser-internals.md §9 step 4.2).
#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `fn`; body optional.
    pub(crate) fn fn_decl(&mut self) -> Result<&'a FnDecl<'a>, error::Fn<'a>> {
        self.fn_decl_with_end().map(|(decl, _)| decl)
    }
}

impl<'a> Parser<'a> {
    /// After `fn`. Like `fn_decl`, plus the position right after the
    /// declaration's last byte (the body's `}`, the last `where`
    /// constraint, the return type, or the params' `)`), computed before any
    /// trailing whitespace was chomped.
    pub(super) fn fn_decl_with_end(&mut self) -> Result<(&'a FnDecl<'a>, Position), error::Fn<'a>> {
        self.chomp();
        let name = self.located_lower(error::Fn::Name)?;
        self.chomp();
        let params = self.specialize(
            |bump, e, row, col| error::Fn::Params(bump.alloc(e), row, col),
            |p| p.params(),
        )?;
        // `params()` stops right after the `)`.
        let mut end = self.get_position();
        self.chomp();
        let ret = if self.peek() == Some(b'-') && self.peek_at(1) == Some(b'>') {
            self.advance_by(2);
            self.chomp();
            let typ = self.specialize(
                |bump, e, row, col| error::Fn::Ret(bump.alloc(e), row, col),
                |p| p.type_expr(),
            )?;
            end = typ.region.end;
            Some(typ)
        } else {
            None
        };
        let where_clause = if self.peek_keyword(b"where") {
            self.advance_by(5);
            end = self.get_position();
            let constraints = self.specialize(
                |bump, e, row, col| error::Fn::Where(bump.alloc(e), row, col),
                |p| p.where_clause(),
            )?;
            if let Some(last) = constraints.last() {
                end = constraint_end(last);
            }
            constraints
        } else {
            &[]
        };
        let body = if self.peek() == Some(b'{') {
            let block = self.specialize(
                |bump, e, row, col| error::Fn::Body(bump.alloc(e), row, col),
                |p| p.block(),
            )?;
            end = block.region.end;
            Some(block)
        } else {
            None
        };
        let decl = self.alloc(FnDecl {
            name,
            params,
            ret,
            where_clause,
            body,
        });
        Ok((decl, end))
    }
}

/// The position right after a `where` constraint's last byte. A trailing
/// comma consumed by `where_clause()` is not included; it sits on the
/// constraint's line in any layout the formatter emits.
fn constraint_end(constraint: &Constraint<'_>) -> Position {
    match constraint {
        Constraint::Bound { var, bounds } => bounds
            .last()
            .map_or(var.region.end, |bound| bound.region().end),
        Constraint::AssocEq { typ, .. } => typ.region.end,
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
    use super::super::assert_item_snapshot;

    // Deviation from §7.1 (one `assert_<thing>` pair per module, defined at
    // module level): the macros below drive `fn_decl()`, `params()` and
    // `where_clause()` directly. They exist because the §7.2-named tests go
    // through `item()` otherwise and would stay ignored until item/mod.rs
    // lands; the `params` / `where` pairs also pin positions the item-level
    // tests cannot isolate (`Params::End`, `Where::AssocEq`, …). They are
    // private to this `mod tests` (not re-exported). Only the `pub` and
    // attribute forms still go through `item()`. Wave 4 decides whether to
    // keep or fold them; recorded for §10.

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

    /// Snapshot test macro for a successful `fn_decl()` parse (input starts
    /// at `fn`, which the macro consumes).
    macro_rules! assert_fn_decl_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            if let Err((row, col)) = parser.keyword(b"fn", |row, col| (row, col)) {
                panic!("input must start with `fn` ({row}:{col})\n\nSource:\n{code}");
            }
            let result = parser
                .fn_decl()
                .unwrap_or_else(|e| panic!("expected Ok, got Err: {e:#?}\n\nSource:\n{code}"));
            // A bodiless `fn` stops at the `)`; `item()` chomps.
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

    /// Snapshot test macro for a `fn_decl()` parse error (input starts at
    /// `fn`, which the macro consumes).
    macro_rules! assert_fn_decl_error_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            if let Err((row, col)) = parser.keyword(b"fn", |row, col| (row, col)) {
                panic!("input must start with `fn` ({row}:{col})\n\nSource:\n{code}");
            }
            let err = parser
                .fn_decl()
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

    // ---- fn_decl() directly (§7.2 names) -----------------------------------

    #[test]
    fn fn_no_params() {
        assert_fn_decl_snapshot!("fn main() { run() }");
    }

    #[test]
    fn fn_params() {
        assert_fn_decl_snapshot!("fn add(a, b) { a + b }");
    }

    #[test]
    fn fn_typed_params() {
        assert_fn_decl_snapshot!("fn add(a: Number, b: Number) -> Number { a + b }");
    }

    #[test]
    fn fn_ret() {
        assert_fn_decl_snapshot!("fn name() -> String { \"x\" }");
    }

    #[test]
    fn fn_mut_param() {
        assert_fn_decl_snapshot!("fn bump(mut n: Number) { n += 1 }");
    }

    #[test]
    fn fn_pattern_param() {
        assert_fn_decl_snapshot!("fn swap((a, b)) { (b, a) }");
    }

    #[test]
    fn fn_trailing_comma_params() {
        assert_fn_decl_snapshot!(
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
    fn fn_multiline_body() {
        assert_fn_decl_snapshot!(
            r#"
            fn classify(n: Number) -> String {
                let sign = if n < 0 { "neg" } else { "pos" }
                sign
            }
        "#
        );
    }

    /// language.md "Traits".
    #[test]
    fn fn_where_single() {
        assert_fn_decl_snapshot!(
            r#"
            fn describe(xs: Array[a]) -> String where a: Show {
                xs |> Array.map(show) |> String.join(", ")
            }
        "#
        );
    }

    #[test]
    fn fn_where_multi() {
        assert_fn_decl_snapshot!(
            r#"
            fn show2(a: a, b: b) -> String where a: Show, b: Show {
                show(a)
            }
        "#
        );
    }

    /// language.md "Type application and variables" (bodiless).
    #[test]
    fn fn_where_plus() {
        assert_fn_decl_snapshot!(
            r#"
            fn lookup(cache: Cache[k, v], key: k) -> Option[v]
                where k: Eq + Hash
        "#
        );
    }

    #[test]
    fn fn_where_assoc() {
        assert_fn_decl_snapshot!(
            r#"
            fn sum(it: i) -> Number where i: Iterator, i.Item == Number {
                0
            }
        "#
        );
    }

    /// language.md "Type application and variables" (bodiless).
    #[test]
    fn fn_where_multiline_trailing_comma() {
        assert_fn_decl_snapshot!(
            r#"
            fn traverse(xs: t[f[a]], g: fn(a) -> f[b]) -> f[t[b]]
                where
                    t: Traversable,
                    f: Applicative,
        "#
        );
    }

    #[test]
    fn fn_where_then_body_on_next_line() {
        assert_fn_decl_snapshot!(
            r#"
            fn describe(xs: Array[a]) -> String
                where a: Show
            {
                show(xs)
            }
        "#
        );
    }

    /// language.md "JavaScript interop" (the attribute form is `fn_bodiless_with_extern_attr`).
    #[test]
    fn fn_bodiless() {
        assert_fn_decl_snapshot!("fn randomUUID() -> String");
    }

    #[test]
    fn fn_bodiless_no_ret() {
        assert_fn_decl_snapshot!("fn next(it: i)");
    }

    #[test]
    fn fn_body_on_next_line() {
        assert_fn_decl_snapshot!(
            r#"
            fn main()
            {
                run()
            }
        "#
        );
    }

    #[test]
    fn error_no_name() {
        assert_fn_decl_error_snapshot!("fn (a) { a }");
    }

    #[test]
    fn error_name_reserved() {
        assert_fn_decl_error_snapshot!("fn for() { 1 }");
    }

    #[test]
    fn error_params_missing() {
        assert_fn_decl_error_snapshot!("fn add { 1 }");
    }

    #[test]
    fn error_params_unclosed() {
        assert_fn_decl_error_snapshot!("fn add(a, b { a }");
    }

    #[test]
    fn error_ret() {
        assert_fn_decl_error_snapshot!("fn add() -> { 1 }");
    }

    #[test]
    fn error_where_bad_bound() {
        assert_fn_decl_error_snapshot!("fn f(x: a) where a: 1 { x }");
    }

    #[test]
    fn error_body() {
        assert_fn_decl_error_snapshot!("fn f() { let }");
    }

    #[test]
    fn error_body_unclosed() {
        assert_fn_decl_error_snapshot!("fn f() { 1");
    }

    // ---- through `item()` ---------------------------------------------------

    /// language.md "Functions".
    #[test]
    #[ignore = "waits for item/mod.rs"]
    fn fn_pub() {
        assert_item_snapshot!(
            r#"
            pub fn add(a: Number, b: Number) -> Number {
                a + b
            }
        "#
        );
    }

    /// language.md "JavaScript interop".
    #[test]
    #[ignore = "waits for item/mod.rs"]
    fn fn_bodiless_with_extern_attr() {
        assert_item_snapshot!(
            r#"
            #[extern("node:crypto", "randomUUID")]
            fn randomUUID() -> String
        "#
        );
    }
}
