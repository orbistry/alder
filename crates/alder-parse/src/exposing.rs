//! Exposing list parsing for Alder.
//!
//! Ported from Elm's `Parse/Module.hs` (exposing, exposingHelp, chompExposed, privacy).
//!
//! Parses exposing lists like:
//! - `(..)` - expose everything
//! - `(foo, bar)` - expose specific values
//! - `(Foo, Bar(..))` - expose types, optionally with constructors
//! - `((+), (-))` - expose operators

use alder_region::Region;
use alder_source::{Exposed, Exposing, Privacy};

use crate::Parser;
use crate::error;

impl<'a> Parser<'a> {
    /// Parse an exposing list.
    ///
    /// Mirrors Elm's `exposing`:
    /// ```text
    /// exposing = '(' ( '..' | exposed { ',' exposed } ) ')'
    /// ```
    pub fn exposing(&mut self) -> Result<Exposing<'a>, error::Exposing> {
        // Opening paren
        self.word1(b'(', error::Exposing::Start)?;
        self.chomp_and_check_indent(error::Exposing::Space, error::Exposing::IndentValue)?;

        // Either ".." for open, or explicit list
        self.one_of(
            error::Exposing::Value,
            vec![
                // (..)
                Box::new(|p: &mut Parser<'a>| {
                    p.word2(b'.', b'.', error::Exposing::Value)?;
                    p.chomp_and_check_indent(error::Exposing::Space, error::Exposing::IndentEnd)?;
                    p.word1(b')', error::Exposing::End)?;
                    Ok(Exposing::Open)
                }),
                // Explicit list
                Box::new(|p: &mut Parser<'a>| {
                    let exposed = p.chomp_exposed()?;
                    p.chomp_and_check_indent(error::Exposing::Space, error::Exposing::IndentEnd)?;
                    p.exposing_help(vec![exposed])
                }),
            ],
        )
    }

    /// Parse remaining exposed items after the first.
    ///
    /// Mirrors Elm's `exposingHelp`.
    fn exposing_help(
        &mut self,
        mut rev_exposed: Vec<&'a Exposed<'a>>,
    ) -> Result<Exposing<'a>, error::Exposing> {
        loop {
            let rev_exposed_for_fallback = rev_exposed.clone();

            let result = self.one_of(
                error::Exposing::End,
                vec![
                    // More items: , exposed
                    Box::new(|p: &mut Parser<'a>| {
                        p.word1(b',', error::Exposing::End)?;
                        p.chomp_and_check_indent(
                            error::Exposing::Space,
                            error::Exposing::IndentValue,
                        )?;
                        let exposed = p.chomp_exposed()?;
                        p.chomp_and_check_indent(
                            error::Exposing::Space,
                            error::Exposing::IndentEnd,
                        )?;
                        rev_exposed.push(exposed);
                        Ok(ExposingHelpState::Continue)
                    }),
                    // End: )
                    Box::new(|p: &mut Parser<'a>| {
                        p.word1(b')', error::Exposing::End)?;
                        let exposed_slice = p.alloc_slice_copy(&rev_exposed_for_fallback);
                        Ok(ExposingHelpState::Done(Exposing::Explicit(exposed_slice)))
                    }),
                ],
            )?;

            match result {
                ExposingHelpState::Continue => continue,
                ExposingHelpState::Done(exposing) => return Ok(exposing),
            }
        }
    }

    /// Parse a single exposed item.
    ///
    /// Mirrors Elm's `chompExposed`:
    /// - lowercase name (value)
    /// - (operator)
    /// - Uppercase name with optional (..)
    fn chomp_exposed(&mut self) -> Result<&'a Exposed<'a>, error::Exposing> {
        let start = self.get_position();

        self.one_of(
            error::Exposing::Value,
            vec![
                // lowercase value
                Box::new(|p: &mut Parser<'a>| {
                    let name = p.lower_name(error::Exposing::Value)?;
                    let located = p.add_end(start, name);
                    Ok(p.alloc(Exposed::Lower(located)))
                }),
                // (operator)
                Box::new(|p: &mut Parser<'a>| {
                    p.word1(b'(', error::Exposing::Value)?;
                    let op = p.operator(error::Exposing::Operator, |bad_op, row, col| {
                        error::Exposing::OperatorReserved(bad_op, row, col)
                    })?;
                    p.word1(b')', error::Exposing::OperatorRightParen)?;
                    let end = p.get_position();
                    Ok(p.alloc(Exposed::Operator {
                        region: Region::new(start, end),
                        op,
                    }))
                }),
                // Uppercase type
                Box::new(|p: &mut Parser<'a>| {
                    let name = p.upper_name(error::Exposing::Value)?;
                    let located = p.add_end(start, name);
                    p.chomp_and_check_indent(error::Exposing::Space, error::Exposing::IndentEnd)?;
                    let privacy = p.privacy()?;
                    Ok(p.alloc(Exposed::Upper {
                        name: located,
                        privacy,
                    }))
                }),
            ],
        )
    }

    /// Parse optional (..) after a type name.
    ///
    /// Mirrors Elm's `privacy`.
    fn privacy(&mut self) -> Result<Privacy, error::Exposing> {
        self.one_of_with_fallback(
            vec![Box::new(|p: &mut Parser<'a>| {
                p.word1(b'(', error::Exposing::TypePrivacy)?;
                p.chomp_and_check_indent(error::Exposing::Space, error::Exposing::TypePrivacy)?;
                let start = p.get_position();
                p.word2(b'.', b'.', error::Exposing::TypePrivacy)?;
                let end = p.get_position();
                p.chomp_and_check_indent(error::Exposing::Space, error::Exposing::TypePrivacy)?;
                p.word1(b')', error::Exposing::TypePrivacy)?;
                Ok(Privacy::Public(Region::new(start, end)))
            })],
            Privacy::Private,
        )
    }
}

/// Internal state for exposing_help loop.
enum ExposingHelpState<'a> {
    Continue,
    Done(Exposing<'a>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::Bump;
    use indoc::indoc;

    macro_rules! assert_exposing_snapshot {
        ($input:expr) => {{
            let input = indoc!($input);
            let bump = Bump::new();
            let src = bump.alloc_str(input);
            let mut parser = Parser::new(&bump, src.as_bytes());
            let result = parser.exposing();
            match result {
                Ok(ref exposing) => {
                    insta::with_settings!({
                        description => format!("Code:\n\n{}", input),
                        omit_expression => true,
                    }, {
                        insta::assert_debug_snapshot!(exposing);
                    });
                }
                Err(e) => {
                    panic!("Expected successful parse, got error: {:?}", e);
                }
            }
        }};
    }

    #[test]
    fn exposing_open() {
        assert_exposing_snapshot!("(..)");
    }

    #[test]
    fn exposing_single_value() {
        assert_exposing_snapshot!("(foo)");
    }

    #[test]
    fn exposing_multiple_values() {
        assert_exposing_snapshot!("(foo, bar, baz)");
    }

    #[test]
    fn exposing_type_private() {
        assert_exposing_snapshot!("(Foo)");
    }

    #[test]
    fn exposing_type_public() {
        assert_exposing_snapshot!("(Foo(..))");
    }

    #[test]
    fn exposing_operator() {
        assert_exposing_snapshot!("((+))");
    }

    #[test]
    fn exposing_multiple_operators() {
        assert_exposing_snapshot!("((+), (-), (++))");
    }

    #[test]
    fn exposing_mixed() {
        assert_exposing_snapshot!("(foo, Bar, Baz(..), (+))");
    }

    #[test]
    fn exposing_with_spaces() {
        assert_exposing_snapshot!("( foo , bar )");
    }

    // Note: Multiline exposing lists are tested indirectly through module/import tests,
    // since they require proper indent context to be set by the caller.
}
