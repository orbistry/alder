//! `enum` declarations.
//!
//! See docs/parser-internals.md §5.11 and §10.39.
//!
//! Grammar (SPEC.md "Items", with §10.8's trailing commas and §10.39's
//! extension-free record payloads):
//!
//! ```ebnf
//! enum_decl      = 'enum' upper_ident [ type_params ] '{' [ variant { ',' variant } [ ',' ] ] '}' ;
//! variant        = upper_ident [ '(' type { ',' type } [ ',' ] ')' | variant_record ] ;
//! variant_record = '{' [ field_type { ',' field_type } [ ',' ] ] '}' ;
//! ```
//!
//! A payload opener (`(` or `{`) attaches to its variant only when it starts
//! on the variant's line (§2.1 rule 1, as `tag_variant` and type arguments
//! do); on a later line it is reported as `Enum::End`. Record payloads reuse
//! `field_types()`, so `Rect { r | width: Number }` parses as a record type
//! and is then refused as `Enum::VariantRecordExt` at the extension name.
//! `Circle()` is `Enum::VariantArg(Type::Start)`: a tuple payload has at
//! least one type.
//!
//! Conventions: `enum_decl` runs after the `enum` keyword and stops right
//! after the closing `}` without chomping; `item()` chomps.
// OWNER: item/enum_.rs (Wave 3)

use alder_source::{EnumDecl, Variant, VariantPayload};
use bumpalo::collections::Vec as BumpVec;

use crate::{Parser, error};

// Called by `item()` (item/mod.rs, Wave 3); the allow goes away with the
// Wave 4 sweep (docs/parser-internals.md §9 step 4.2).
#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `enum`. Record payloads reuse `field_types()`; a `Some(ext)` result is Enum::VariantRecordExt.
    pub(crate) fn enum_decl(&mut self) -> Result<&'a EnumDecl<'a>, error::Enum<'a>> {
        self.chomp();
        let name = self.located_upper(error::Enum::Name)?;
        self.chomp();
        let params = if self.peek() == Some(b'[') {
            self.specialize(
                |bump, e, row, col| error::Enum::Params(bump.alloc(e), row, col),
                |p| p.type_params(),
            )?
        } else {
            &[]
        };
        self.chomp();
        self.word1(b'{', error::Enum::Open)?;
        self.chomp();
        let mut variants = BumpVec::new_in(self.bump);
        loop {
            if self.peek() == Some(b'}') {
                self.advance();
                break;
            }
            variants.push(self.variant()?);
            self.chomp();
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
                    return Err(error::Enum::End(row, col));
                }
            }
        }
        Ok(self.alloc(EnumDecl {
            name,
            params,
            variants: variants.into_bump_slice(),
        }))
    }

    /// `Name`, `Name(T, …)` or `Name { field: T, … }`. Stops after the
    /// payload's closer (or the name); trailing whitespace before a
    /// payload-less `,` / `}` is left for the caller to chomp.
    fn variant(&mut self) -> Result<Variant<'a>, error::Enum<'a>> {
        let name = self.located_upper(error::Enum::Variant)?;
        self.chomp();
        let payload = match self.peek() {
            Some(b'(') if !self.newline_since(name.region.end) => {
                self.advance();
                self.chomp();
                let mut args = BumpVec::new_in(self.bump);
                loop {
                    let arg = self.specialize(
                        |bump, e, row, col| error::Enum::VariantArg(bump.alloc(e), row, col),
                        |p| p.type_expr(),
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
                            return Err(error::Enum::VariantArgEnd(row, col));
                        }
                    }
                }
                VariantPayload::Tuple(args.into_bump_slice())
            }
            Some(b'{') if !self.newline_since(name.region.end) => {
                let (fields, ext) = self.specialize(
                    |bump, e, row, col| error::Enum::VariantRecord(bump.alloc(e), row, col),
                    |p| {
                        p.advance(); // `{`
                        p.field_types()
                    },
                )?;
                if let Some(ext) = ext {
                    return Err(error::Enum::VariantRecordExt(
                        ext.region.start.line,
                        ext.region.start.column,
                    ));
                }
                VariantPayload::Record(fields)
            }
            _ => VariantPayload::Unit,
        };
        Ok(Variant { name, payload })
    }
}

#[cfg(test)]
mod tests {
    use super::super::assert_item_snapshot;

    // Deviation from §7.1, following item/fn_.rs: the pair below drives
    // `enum_decl()` directly (the input starts at the `enum` keyword, which
    // the macro consumes) so the §7.2 tests run before `item()` lands. The
    // `pub` form goes through `item()` and stays ignored until item/mod.rs
    // lands. Wave 4 decides whether to keep or fold them; recorded for §10.

    /// Snapshot test macro for a successful `enum_decl()` parse (input starts at `enum`).
    macro_rules! assert_enum_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            if let Err((row, col)) = parser.keyword(b"enum", |row, col| (row, col)) {
                panic!("input must start with `enum` ({row}:{col})\n\nSource:\n{code}");
            }
            let result = parser
                .enum_decl()
                .unwrap_or_else(|e| panic!("expected Ok, got Err: {e:#?}\n\nSource:\n{code}"));
            // `enum_decl()` stops at the `}`; `item()` chomps.
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

    /// Snapshot test macro for an `enum_decl()` parse error (input starts at `enum`).
    macro_rules! assert_enum_error_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            if let Err((row, col)) = parser.keyword(b"enum", |row, col| (row, col)) {
                panic!("input must start with `enum` ({row}:{col})\n\nSource:\n{code}");
            }
            let err = parser
                .enum_decl()
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

    #[test]
    fn enum_unit_variants() {
        assert_enum_snapshot!("enum Color { Red, Green, Blue }");
    }

    #[test]
    fn enum_tuple_variant() {
        assert_enum_snapshot!("enum Shape { Circle(Number) }");
    }

    #[test]
    fn enum_tuple_variant_many_args() {
        assert_enum_snapshot!("enum Pair { Both(Number, String) }");
    }

    #[test]
    fn enum_tuple_variant_trailing_comma() {
        assert_enum_snapshot!("enum Pair { Both(Number, String,) }");
    }

    #[test]
    fn enum_record_variant() {
        assert_enum_snapshot!("enum Shape { Rect { width: Number, height: Number } }");
    }

    #[test]
    fn enum_record_variant_empty() {
        assert_enum_snapshot!("enum Shape { Empty {} }");
    }

    /// language.md "Enums" (`pub` dropped: the direct macro starts at `enum`).
    #[test]
    fn enum_mixed() {
        assert_enum_snapshot!(
            r#"
            enum Shape {
                Circle(Number),
                Rect { width: Number, height: Number },
            }
        "#
        );
    }

    /// language.md "Enums" (`pub` dropped: the direct macro starts at `enum`).
    #[test]
    #[ignore = "waits for item/type_alias.rs"]
    fn enum_params() {
        assert_enum_snapshot!(
            r#"
            enum Option[a] {
                Some(a),
                None,
            }
        "#
        );
    }

    #[test]
    #[ignore = "waits for item/type_alias.rs"]
    fn enum_params_two() {
        assert_enum_snapshot!("enum Result[a, e] { Ok(a), Err(e) }");
    }

    #[test]
    fn enum_trailing_comma() {
        assert_enum_snapshot!("enum Color { Red, Green, }");
    }

    #[test]
    fn enum_empty() {
        assert_enum_snapshot!("enum Never {}");
    }

    #[test]
    fn enum_multiline_no_trailing_comma() {
        assert_enum_snapshot!(
            r#"
            enum Color {
                Red,
                Green
            }
        "#
        );
    }

    #[test]
    #[ignore = "waits for item/mod.rs"]
    fn enum_pub() {
        assert_item_snapshot!(
            r#"
            pub enum Shape {
                Circle(Number),
                Rect { width: Number, height: Number },
            }
        "#
        );
    }

    #[test]
    fn error_no_name() {
        assert_enum_error_snapshot!("enum { A }");
    }

    #[test]
    fn error_open() {
        assert_enum_error_snapshot!("enum Color Red");
    }

    #[test]
    fn error_variant_lowercase() {
        assert_enum_error_snapshot!("enum Color { red }");
    }

    #[test]
    fn error_unclosed() {
        assert_enum_error_snapshot!("enum Color { Red, Green");
    }

    #[test]
    fn error_end() {
        assert_enum_error_snapshot!("enum Color { Red Green }");
    }

    #[test]
    fn error_variant_arg() {
        assert_enum_error_snapshot!("enum Shape { Circle(1) }");
    }

    #[test]
    fn error_variant_arg_empty() {
        assert_enum_error_snapshot!("enum Shape { Circle() }");
    }

    #[test]
    fn error_variant_arg_end() {
        assert_enum_error_snapshot!("enum Shape { Circle(Number Number) }");
    }

    #[test]
    fn error_variant_record() {
        assert_enum_error_snapshot!("enum Shape { Rect { width } }");
    }

    #[test]
    fn error_variant_record_extension() {
        assert_enum_error_snapshot!("enum Shape { Rect { r | width: Number } }");
    }

    #[test]
    fn error_payload_on_next_line() {
        assert_enum_error_snapshot!(
            r#"
            enum Shape {
                Circle
                (Number),
            }
        "#
        );
    }
}
