//! `test "name" { }` declarations and `tests { }` blocks.
//!
//! Grammar (SPEC.md):
//!
//! ```text
//! test_decl   = 'test' string block ;
//! tests_block = 'tests' '{' { item } '}' ;
//! ```
//!
//! The test name is a plain string literal; its `Located` region includes
//! the quotes. A `tests` body is an item list with the module's separation
//! rule (`items_until_close`, §10.38): imports inside it are reachable only
//! through `ItemKind::Tests`.
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/test.rs (Wave 3)

use alder_region::Located;
use alder_source::{Item, TestDecl};

use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// After `test`.
    pub(crate) fn test_decl(&mut self) -> Result<&'a TestDecl<'a>, error::Test<'a>> {
        self.chomp();
        let start = self.get_position();
        let text = self.string_literal(error::Test::Name, error::Test::NameString)?;
        let name = self.located(start, text);
        self.chomp();
        let body = self.specialize(
            |bump, e, row, col| error::Test::Body(bump.alloc(e), row, col),
            |p| p.block(),
        )?;
        Ok(self.alloc(TestDecl { name, body }))
    }

    /// After `tests`.
    pub(crate) fn tests_block(&mut self) -> Result<&'a [&'a Located<Item<'a>>], error::Tests<'a>> {
        self.chomp();
        self.word1(b'{', error::Tests::Open)?;
        self.chomp();
        self.items_until_close()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{assert_item_error_snapshot, assert_item_snapshot};

    #[test]
    fn test_simple() {
        assert_item_snapshot!(
            r#"
            test "adds numbers" {
                assert add(1, 2) == 3
            }
            "#
        );
    }

    #[test]
    fn test_provide() {
        assert_item_snapshot!(
            r#"
            test "finds a user" {
                provide Db = fakeDb() {
                    assert find(1).await == Ok(ada)
                }
            }
            "#
        );
    }

    #[test]
    fn test_empty_body() {
        assert_item_snapshot!(r#"test "todo" {}"#);
    }

    #[test]
    fn tests_block_empty() {
        assert_item_snapshot!("tests {}");
    }

    #[test]
    fn tests_block_import_and_tests() {
        assert_item_snapshot!(
            r#"
            tests {
                import @alder/test.{ fakeDb }

                test "adds numbers" {
                    assert add(1, 2) == 3
                }

                test "finds a user" {
                    provide Db = fakeDb() {
                        assert find(1).await == Ok(ada)
                    }
                }
            }
            "#
        );
    }

    #[test]
    fn tests_block_helper_let() {
        assert_item_snapshot!(
            r#"
            tests {
                let ada = { name: "Ada" }
                test "has a name" {
                    assert ada.name == "Ada"
                }
            }
            "#
        );
    }

    #[test]
    fn error_test_no_name() {
        assert_item_error_snapshot!("test { assert true }");
    }

    #[test]
    fn error_test_name_bad_string() {
        assert_item_error_snapshot!(r#"test "unterminated"#);
    }

    #[test]
    fn error_test_no_body() {
        assert_item_error_snapshot!(r#"test "adds numbers""#);
    }

    #[test]
    fn error_test_body() {
        assert_item_error_snapshot!(
            r#"
            test "adds numbers" {
                assert
            }
            "#
        );
    }

    #[test]
    fn error_tests_open() {
        assert_item_error_snapshot!("tests [");
    }

    #[test]
    fn error_tests_bad_item() {
        assert_item_error_snapshot!(
            r#"
            tests {
                42
            }
            "#
        );
    }

    #[test]
    fn error_tests_item() {
        assert_item_error_snapshot!(
            r#"
            tests {
                test 42 {}
            }
            "#
        );
    }

    #[test]
    fn error_tests_same_line() {
        assert_item_error_snapshot!(r#"tests { test "a" {} test "b" {} }"#);
    }

    #[test]
    fn error_tests_semicolon() {
        assert_item_error_snapshot!(r#"tests { test "a" {}; }"#);
    }

    #[test]
    fn error_tests_unclosed() {
        assert_item_error_snapshot!(
            r#"
            tests {
                test "a" {}
            "#
        );
    }
}
