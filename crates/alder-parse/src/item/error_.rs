//! `error` groups.
//!
//! See docs/parser-internals.md §5.11.
//!
//! Grammar (SPEC.md "Items", with §10.8's trailing commas):
//!
//! ```ebnf
//! error_decl  = 'error' upper_ident '{' [ tag_variant { ',' tag_variant } [ ',' ] ] '}' ;
//! tag_variant = tag [ '(' type { ',' type } [ ',' ] ')' ] ;
//! ```
//!
//! Tags are read by `tag_variant()` (type_.rs), the same scanner error rows
//! use, so `:expired(Timestamp)` lexes identically in `error AuthError { … }`
//! and in `[:expired(Timestamp) | r]`. A name that is not a `:tag` (`Foo`,
//! `expired`) is `ErrorDecl::Tag(TagVariant::Name)`.
//!
//! Conventions: `error_decl` runs after the `error` keyword and stops right
//! after the closing `}` without chomping; `item()` chomps.
// OWNER: item/error_.rs (Wave 3)

use alder_source::ErrorDecl;
use bumpalo::collections::Vec as BumpVec;

use crate::{Parser, error};

// Called by `item()` (item/mod.rs, Wave 3); the allow goes away with the
// Wave 4 sweep (docs/parser-internals.md §9 step 4.2).
#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `error`.
    pub(crate) fn error_decl(&mut self) -> Result<&'a ErrorDecl<'a>, error::ErrorDecl<'a>> {
        self.chomp();
        let name = self.located_upper(error::ErrorDecl::Name)?;
        self.chomp();
        self.word1(b'{', error::ErrorDecl::Open)?;
        self.chomp();
        let mut tags = BumpVec::new_in(self.bump);
        loop {
            if self.peek() == Some(b'}') {
                self.advance();
                break;
            }
            let tag = self.specialize(
                |bump, e, row, col| error::ErrorDecl::Tag(bump.alloc(e), row, col),
                |p| p.tag_variant(),
            )?;
            tags.push(tag);
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
                    return Err(error::ErrorDecl::End(row, col));
                }
            }
        }
        Ok(self.alloc(ErrorDecl {
            name,
            tags: tags.into_bump_slice(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::assert_item_snapshot;

    // Deviation from §7.1, following item/fn_.rs: the pair below drives
    // `error_decl()` directly (the input starts at the `error` keyword, which
    // the macro consumes) so the §7.2 tests run before `item()` lands. The
    // `pub` form goes through `item()` and stays ignored until item/mod.rs
    // lands. Wave 4 decides whether to keep or fold them; recorded for §10.

    /// Snapshot test macro for a successful `error_decl()` parse (input starts at `error`).
    macro_rules! assert_error_decl_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            if let Err((row, col)) = parser.keyword(b"error", |row, col| (row, col)) {
                panic!("input must start with `error` ({row}:{col})\n\nSource:\n{code}");
            }
            let result = parser
                .error_decl()
                .unwrap_or_else(|e| panic!("expected Ok, got Err: {e:#?}\n\nSource:\n{code}"));
            // `error_decl()` stops at the `}`; `item()` chomps.
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

    /// Snapshot test macro for an `error_decl()` parse error (input starts at `error`).
    macro_rules! assert_error_decl_error_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            if let Err((row, col)) = parser.keyword(b"error", |row, col| (row, col)) {
                panic!("input must start with `error` ({row}:{col})\n\nSource:\n{code}");
            }
            let err = parser
                .error_decl()
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
    fn error_group_simple() {
        assert_error_decl_snapshot!("error AuthError { :invalid_token, :expired }");
    }

    #[test]
    fn error_group_args() {
        assert_error_decl_snapshot!(
            "error AuthError { :expired(Timestamp), :bad(Number, String) }"
        );
    }

    /// language.md "Errors" (`pub` dropped: the direct macro starts at `error`).
    #[test]
    fn error_group_trailing_comma() {
        assert_error_decl_snapshot!(
            r#"
            error AuthError {
                :invalid_token,
                :expired(Timestamp),
            }
        "#
        );
    }

    #[test]
    fn error_group_empty() {
        assert_error_decl_snapshot!("error Never {}");
    }

    #[test]
    fn error_group_single_no_comma() {
        assert_error_decl_snapshot!("error Timeout { :timeout }");
    }

    /// language.md "Errors", as written.
    #[test]
    #[ignore = "waits for item/mod.rs"]
    fn error_group_pub() {
        assert_item_snapshot!(
            r#"
            pub error AuthError {
                :invalid_token,
                :expired(Timestamp),
            }
        "#
        );
    }

    #[test]
    fn error_no_name() {
        assert_error_decl_error_snapshot!("error { :a }");
    }

    #[test]
    fn error_open() {
        assert_error_decl_error_snapshot!("error E :a");
    }

    #[test]
    fn error_bad_tag() {
        assert_error_decl_error_snapshot!("error E { Foo }");
    }

    #[test]
    fn error_bad_tag_no_colon() {
        assert_error_decl_error_snapshot!("error E { expired }");
    }

    #[test]
    fn error_tag_arg() {
        assert_error_decl_error_snapshot!("error E { :a(1) }");
    }

    #[test]
    fn error_tag_arg_end() {
        assert_error_decl_error_snapshot!("error E { :a(Number Number) }");
    }

    #[test]
    fn error_unclosed() {
        assert_error_decl_error_snapshot!("error E { :a, :b");
    }

    #[test]
    fn error_end() {
        assert_error_decl_error_snapshot!("error E { :a :b }");
    }
}
