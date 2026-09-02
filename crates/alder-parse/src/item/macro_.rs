//! `macro` declarations (raw bodies until M5) and `comptime` blocks.
//!
//! Grammar (SPEC.md):
//!
//! ```text
//! macro_decl     = 'macro' lower_ident '(' [ lower_ident { ',' lower_ident } ] ')' block ;
//! comptime_block = 'comptime' block ;
//! ```
//!
//! A macro body is raw balanced text (`raw_balanced`, §6.5, §10.29): the
//! `Located<&str>` covers the interior between the braces, so `quote { }` /
//! `unquote(x)` never reach the expression parser in M1. The parameter list
//! is required; a missing `(` (`macro now {}`) is `Macro::ParamsOpen` at
//! the position where the `(` was expected.
//! `Macro::Body` carries `raw_balanced`'s problems (`Open` when
//! there is no `{`, `Endless`, `Unbalanced`, `String`) at the positions
//! §5.9 gives them. A `comptime` body is an ordinary block.
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/macro_.rs (Wave 3)

use alder_region::Located;
use alder_source::{Block, MacroDecl};
use bumpalo::collections::Vec as BumpVec;

use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// After `macro`.
    pub(crate) fn macro_decl(&mut self) -> Result<&'a MacroDecl<'a>, error::Macro> {
        self.chomp();
        let name = self.located_lower(error::Macro::Name)?;
        self.chomp();
        self.word1(b'(', error::Macro::ParamsOpen)?;
        self.chomp();
        let mut params = BumpVec::new_in(self.bump);
        loop {
            if self.peek() == Some(b')') {
                self.advance();
                break;
            }
            params.push(self.located_lower(error::Macro::Param)?);
            self.chomp();
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
                    return Err(error::Macro::ParamEnd(row, col));
                }
            }
        }
        self.chomp();
        let body = self.raw_balanced(b'{', b'}', error::Macro::Body)?;
        Ok(self.alloc(MacroDecl {
            name,
            params: params.into_bump_slice(),
            body,
        }))
    }

    /// After `comptime`.
    pub(crate) fn comptime_block(&mut self) -> Result<&'a Located<Block<'a>>, error::Block<'a>> {
        self.chomp();
        self.block()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{assert_item_error_snapshot, assert_item_snapshot};

    #[test]
    fn macro_no_params() {
        assert_item_snapshot!("macro now() { quote { Date.now() } }");
    }

    #[test]
    fn macro_params() {
        assert_item_snapshot!("macro swap(a, b) { quote { (unquote(b), unquote(a)) } }");
    }

    #[test]
    fn macro_params_trailing_comma() {
        assert_item_snapshot!("macro pair(a, b,) { a }");
    }

    #[test]
    fn macro_empty_body() {
        assert_item_snapshot!("macro nothing() {}");
    }

    #[test]
    fn macro_nested_braces() {
        assert_item_snapshot!(
            r#"
            macro when(cond, body) {
                quote {
                    if unquote(cond) { unquote(body) } else { () }
                }
            }
            "#
        );
    }

    #[test]
    fn macro_quote_body_raw() {
        assert_item_snapshot!(
            r#"
            macro assert_eq(left, right) {
                quote {
                    let l = unquote(left)
                    let r = unquote(right)
                    if l != r { Test.fail(unquote(stringify(left)), l, r) }
                }
            }
            "#
        );
    }

    #[test]
    fn macro_body_string_hides_brace() {
        assert_item_snapshot!(r#"macro close() { "}" }"#);
    }

    #[test]
    fn macro_pub() {
        assert_item_snapshot!("pub macro id(x) { x }");
    }

    #[test]
    fn comptime_block() {
        assert_item_snapshot!(
            r#"
            comptime {
                let routes = Fs.readDir("routes")
                routes
            }
            "#
        );
    }

    #[test]
    fn error_name() {
        assert_item_error_snapshot!("macro Now() {}");
    }

    #[test]
    fn error_no_params() {
        assert_item_error_snapshot!("macro now {}");
    }

    #[test]
    fn error_param() {
        assert_item_error_snapshot!("macro f(A) {}");
    }

    #[test]
    fn error_param_end() {
        assert_item_error_snapshot!("macro f(a b) {}");
    }

    #[test]
    fn error_no_body() {
        assert_item_error_snapshot!("macro f(a)");
    }

    #[test]
    fn error_unbalanced() {
        assert_item_error_snapshot!("macro f(a) { (a] }");
    }

    #[test]
    fn error_endless() {
        assert_item_error_snapshot!("macro f(a) { quote { a }");
    }

    #[test]
    fn error_comptime_no_block() {
        assert_item_error_snapshot!("comptime 42");
    }
}
