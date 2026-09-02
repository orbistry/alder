//! `schema` declarations.
//!
//! See docs/parser-internals.md §5.11 and §10.28.
//!
//! Grammar (SPEC.md "Items"):
//!
//! ```ebnf
//! schema_decl = 'schema' upper_ident [ 'from' lower_ident ] '{' { schema_item } '}' ;
//! schema_item = 'pick' lower_ident { ',' lower_ident } [ ',' ]
//!             | lower_ident ':' ( rule { ',' rule } | type [ ',' rule { ',' rule } ] ) [ ',' ] ;
//! rule        = lower_ident [ '(' [ expression { ',' expression } [ ',' ] ')' ] ] ;
//! ```
//!
//! Items are line-separated only by convention. After a field's `:`, a
//! non-reserved lowercase word starts the rule list; anything else
//! (`String`, `Array[String]`, `fn(a) -> b`, `(a, b)`) is a type, which may
//! be followed by `,` and rules or stand alone (§10.28, extended: SPEC
//! requires at least one rule after a type; the standalone form is
//! accepted here because `confirm: String` is the natural way to declare
//! an unconstrained field). `pick` is contextual: `pick: …` is a field
//! named `pick`. A trailing comma is accepted after pick names and after
//! rules; the list ends when `}`, the next `name :` or the next `pick`
//! item follows the comma. Rules are parsed by `modifier()`
//! (item/table.rs).
//!
//! Consequences of the §10.28 type-vs-rule rule, for the Wave 4 SPEC
//! amendment: a schema field type can never start with a lowercase
//! identifier (`a: t` is the rule `t`; `a: r[x], min(1)` is the rule `r`
//! followed by `Schema::End` at `[`), so SPEC's `[ type ',' ]` must
//! exclude type variables and their applications there. And after a
//! finished item, a lowercase word that is neither `pick …` nor `name :`
//! is `Schema::End` at the word (a rule or pick name missing its `,`),
//! not a field missing its `:`.
//!
//! `schema_decl` stops right after the closing `}` without chomping, so
//! `item()` computes the item region before its own chomp.
// OWNER: item/schema.rs (Wave 3)

use alder_source::{SchemaDecl, SchemaItem};
use bumpalo::collections::Vec as BumpVec;

use crate::keyword::is_reserved;
use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// After `schema`.
    // Called by `item()` (item/mod.rs, Wave 3); the allow goes away with the
    // Wave 4 sweep (docs/parser-internals.md §9 step 4.2).
    #[allow(unused)]
    pub(crate) fn schema_decl(&mut self) -> Result<&'a SchemaDecl<'a>, error::Schema<'a>> {
        self.chomp();
        let name = self.located_upper(error::Schema::Name)?;
        self.chomp();
        let from = if self.peek_keyword(b"from") {
            self.advance_by(4);
            self.chomp();
            let table = self.located_lower(error::Schema::From)?;
            self.chomp();
            Some(table)
        } else {
            None
        };
        self.word1(b'{', error::Schema::Open)?;
        self.chomp();
        let mut items = BumpVec::new_in(self.bump);
        loop {
            match self.peek() {
                Some(b'}') => {
                    self.advance();
                    break;
                }
                Some(b) if b.is_ascii_lowercase() => {
                    let item = if self.peek_keyword(b"pick") && !self.peek_ident_colon() {
                        self.pick()?
                    } else if items.is_empty() || self.peek_ident_colon() {
                        self.field()?
                    } else {
                        // A word after a finished item that starts neither
                        // `pick …` nor `name :` is a rule or pick name that
                        // lost its `,` (`min(3)\n max(10)`, `String min(1)`,
                        // `pick email name`): report it at the word, not as
                        // a missing `:` further on.
                        let (row, col) = self.position();
                        return Err(error::Schema::End(row, col));
                    };
                    items.push(item);
                }
                _ => {
                    let (row, col) = self.position();
                    return Err(if items.is_empty() {
                        error::Schema::Item(row, col)
                    } else {
                        error::Schema::End(row, col)
                    });
                }
            }
        }
        Ok(self.alloc(SchemaDecl {
            name,
            from,
            items: items.into_bump_slice(),
        }))
    }

    /// At `pick`. Chomps trailing whitespace.
    fn pick(&mut self) -> Result<SchemaItem<'a>, error::Schema<'a>> {
        self.advance_by(4);
        self.chomp();
        let mut names = BumpVec::new_in(self.bump);
        names.push(self.located_lower(error::Schema::PickName)?);
        self.chomp();
        while self.peek() == Some(b',') {
            self.advance();
            self.chomp();
            if self.at_item_boundary() {
                break;
            }
            names.push(self.located_lower(error::Schema::PickName)?);
            self.chomp();
        }
        Ok(SchemaItem::Pick(names.into_bump_slice()))
    }

    /// `name ':' [ type [','] ] { rule ',' }`. Chomps trailing whitespace.
    fn field(&mut self) -> Result<SchemaItem<'a>, error::Schema<'a>> {
        let name = self.located_lower(error::Schema::Item)?;
        self.chomp();
        self.word1(b':', error::Schema::Colon)?;
        self.chomp();
        let mut expect_rule = true;
        let typ = if self.peek_lower() && !is_reserved(self.peek_word()) {
            None
        } else {
            let typ = self.specialize(
                |bump, e, row, col| error::Schema::Type(bump.alloc(e), row, col),
                |p| p.type_expr(),
            )?;
            // `type_expr()` chomps.
            expect_rule = self.eat_rule_comma();
            Some(typ)
        };
        let mut rules = BumpVec::new_in(self.bump);
        while expect_rule {
            if !self.peek_lower() || is_reserved(self.peek_word()) {
                let (row, col) = self.position();
                return Err(error::Schema::Rule(row, col));
            }
            rules.push(self.modifier(error::Schema::RuleArg, error::Schema::RuleArgEnd)?);
            // `modifier()` chomps.
            expect_rule = self.eat_rule_comma();
        }
        Ok(SchemaItem::Field {
            name,
            typ,
            rules: rules.into_bump_slice(),
        })
    }

    /// Consume a `,` (and whitespace) if present. Returns whether another
    /// rule must follow: `false` when there was no comma or the comma was a
    /// trailing one (followed by `}` or the next `name :`).
    fn eat_rule_comma(&mut self) -> bool {
        if self.peek() != Some(b',') {
            return false;
        }
        self.advance();
        self.chomp();
        !self.at_item_boundary()
    }

    /// After a trailing comma: is the cursor on `}`, on the next item's
    /// `name :`, or on the next `pick` item? Does not consume.
    fn at_item_boundary(&mut self) -> bool {
        self.peek() == Some(b'}')
            || self.peek_ident_colon()
            || (self.peek_keyword(b"pick") && !self.peek_ident_colon())
    }
}

#[cfg(test)]
mod tests {
    use super::super::assert_item_snapshot;

    // Deviation from §7.1 (one `assert_<thing>` pair per module): like
    // item/fn_.rs, the two macros below drive `schema_decl()` directly
    // because the §7.2 tests go through `item()`, which stays a stub until
    // item/mod.rs lands. The input is a complete `schema …` declaration;
    // the macro consumes the keyword and hands the rest to `schema_decl()`.
    // Private to this `mod tests`. Wave 4 decides whether to keep or fold
    // them; recorded for §10.

    /// Snapshot test macro for a successful `schema_decl()` parse (input starts at `schema`).
    macro_rules! assert_schema_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            parser
                .keyword(b"schema", |row, col| format!("input must start with `schema` ({row}:{col})"))
                .unwrap();
            let result = parser
                .schema_decl()
                .unwrap_or_else(|e| panic!("expected Ok, got Err: {e:#?}\n\nSource:\n{code}"));
            // `schema_decl()` stops at the `}`; `item()` chomps.
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

    /// Snapshot test macro for a `schema_decl()` parse error (input starts at `schema`).
    macro_rules! assert_schema_error_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            parser
                .keyword(b"schema", |row, col| format!("input must start with `schema` ({row}:{col})"))
                .unwrap();
            let err = parser
                .schema_decl()
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

    // ---- schema_decl() directly --------------------------------------------

    #[test]
    fn schema_simple() {
        assert_schema_snapshot!(
            r#"
            schema Login {
                email: String, min(1)
            }
        "#
        );
    }

    #[test]
    fn schema_empty() {
        assert_schema_snapshot!("schema Empty {}");
    }

    #[test]
    fn schema_from() {
        assert_schema_snapshot!(
            r#"
            schema SignUp from users {
                pick email
            }
        "#
        );
    }

    #[test]
    fn schema_pick() {
        assert_schema_snapshot!(
            r#"
            schema SignUp from users {
                pick email, name
            }
        "#
        );
    }

    #[test]
    fn schema_pick_trailing_comma() {
        assert_schema_snapshot!(
            r#"
            schema SignUp from users {
                pick email, name,
                password: String
            }
        "#
        );
    }

    #[test]
    fn schema_pick_twice() {
        assert_schema_snapshot!(
            r#"
            schema SignUp from users {
                pick email
                pick name
            }
        "#
        );
    }

    #[test]
    fn schema_pick_trailing_comma_then_pick() {
        assert_schema_snapshot!(
            r#"
            schema S from users {
                pick a,
                pick b
            }
        "#
        );
    }

    #[test]
    fn schema_rules_trailing_comma_then_pick() {
        assert_schema_snapshot!(
            r#"
            schema S from users {
                name: min(3),
                pick email
            }
        "#
        );
    }

    #[test]
    fn schema_type_var_is_rule() {
        assert_schema_snapshot!("schema S { a: t }");
    }

    #[test]
    fn schema_pick_as_field_name() {
        assert_schema_snapshot!("schema S { pick: String }");
    }

    #[test]
    fn schema_typed_rules() {
        assert_schema_snapshot!(
            r#"
            schema SignUp {
                password: String, min(12), max(64)
            }
        "#
        );
    }

    #[test]
    fn schema_untyped_rules() {
        assert_schema_snapshot!(
            r#"
            schema SignUp from users {
                name: min(3), max(40)
            }
        "#
        );
    }

    #[test]
    fn schema_untyped_bare_rule() {
        assert_schema_snapshot!("schema S from users { name: required }");
    }

    #[test]
    fn schema_typed_no_rules() {
        assert_schema_snapshot!("schema S { confirm: String }");
    }

    #[test]
    fn schema_type_applied() {
        assert_schema_snapshot!("schema S { tags: Array[String], max(5) }");
    }

    #[test]
    fn schema_type_fn() {
        assert_schema_snapshot!("schema S { check: fn(String) -> Bool, required }");
    }

    #[test]
    fn schema_rules_trailing_comma() {
        assert_schema_snapshot!(
            r#"
            schema S {
                name: min(3),
                age: Number,
            }
        "#
        );
    }

    #[test]
    fn schema_rule_args_multiple() {
        assert_schema_snapshot!("schema S { age: Number, between(18, 120) }");
    }

    #[test]
    fn schema_rule_custom_fn() {
        assert_schema_snapshot!(
            "schema S { confirm: String, equals(password), check(fn(c) c != \"\") }"
        );
    }

    #[test]
    fn schema_fields_same_line() {
        assert_schema_snapshot!("schema S { a: min(1) b: max(2) }");
    }

    #[test]
    fn schema_with_comments() {
        assert_schema_snapshot!(
            r#"
            schema S from users {
                // columns
                pick email // the login
                name: min(3)
            }
        "#
        );
    }

    #[test]
    fn docs_web_sign_up() {
        assert_schema_snapshot!(
            r#"
            schema SignUp from users {
                pick email, name
                name: min(3)
                password: String, min(12)
                confirm: String, equals(password)
            }
        "#
        );
    }

    #[test]
    fn error_no_name() {
        assert_schema_error_snapshot!("schema { }");
    }

    #[test]
    fn error_name_lowercase() {
        assert_schema_error_snapshot!("schema signUp { }");
    }

    #[test]
    fn error_from_no_table() {
        assert_schema_error_snapshot!("schema SignUp from { }");
    }

    #[test]
    fn error_from_reserved_table() {
        assert_schema_error_snapshot!("schema SignUp from type { }");
    }

    #[test]
    fn error_open() {
        assert_schema_error_snapshot!("schema SignUp ( )");
    }

    #[test]
    fn error_bad_item() {
        assert_schema_error_snapshot!("schema S { 5 }");
    }

    #[test]
    fn error_item_reserved() {
        assert_schema_error_snapshot!("schema S { type: String }");
    }

    #[test]
    fn error_unclosed_empty() {
        assert_schema_error_snapshot!("schema S {");
    }

    #[test]
    fn error_pick_name() {
        assert_schema_error_snapshot!("schema S from users { pick 5 }");
    }

    #[test]
    fn error_pick_name_after_comma() {
        assert_schema_error_snapshot!("schema S from users { pick a, 5 }");
    }

    #[test]
    fn error_missing_colon() {
        assert_schema_error_snapshot!("schema S { name min(3) }");
    }

    #[test]
    fn error_type() {
        assert_schema_error_snapshot!("schema S { name: 5 }");
    }

    #[test]
    fn error_rule_after_comma() {
        assert_schema_error_snapshot!("schema S { name: String, 5 }");
    }

    #[test]
    fn error_rule_reserved() {
        assert_schema_error_snapshot!("schema S { name: min(3), fn }");
    }

    #[test]
    fn error_rule_missing_comma_next_line() {
        assert_schema_error_snapshot!(
            r#"
            schema S {
                name: min(3)
                    max(10)
            }
        "#
        );
    }

    #[test]
    fn error_rules_missing_comma() {
        assert_schema_error_snapshot!("schema S { a: min(3) max(5) }");
    }

    #[test]
    fn error_type_then_rule_missing_comma() {
        assert_schema_error_snapshot!("schema S { a: String min(1) }");
    }

    #[test]
    fn error_pick_names_missing_comma() {
        assert_schema_error_snapshot!("schema S from users { pick email name }");
    }

    #[test]
    fn error_type_var_applied() {
        assert_schema_error_snapshot!("schema S { a: r[x], min(1) }");
    }

    #[test]
    fn error_rule_arg() {
        assert_schema_error_snapshot!("schema S { name: min( }");
    }

    #[test]
    fn error_rule_arg_end() {
        assert_schema_error_snapshot!("schema S { name: min(3 4) }");
    }

    #[test]
    fn error_rule_arg_unclosed() {
        assert_schema_error_snapshot!("schema S { name: min(3");
    }

    #[test]
    fn error_end() {
        assert_schema_error_snapshot!(
            r#"
            schema S from users {
                pick a
                5
            }
        "#
        );
    }

    #[test]
    fn error_unclosed() {
        assert_schema_error_snapshot!("schema S { name: min(3)");
    }

    // ---- through `item()` (§7.2 names) -------------------------------------

    #[test]
    fn schema_pub() {
        assert_item_snapshot!(
            r#"
            pub schema SignUp from users {
                pick email, name
            }
        "#
        );
    }
}
