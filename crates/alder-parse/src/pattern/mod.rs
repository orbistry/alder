//! Pattern parsing for Alder.
//!
//! Ported from Elm's `Parse/Pattern.hs`.
//!
//! Provides:
//! - `pattern_term` - atomic patterns (wildcard, var, ctor, literal, record, tuple, list)
//! - `pattern_expr` - full patterns including cons (::), as-patterns, and ctor with args

mod list;
mod record;
mod term;
mod tuple;

use alder_region::{Located, Position, Region};
use alder_source::Pattern;
use bumpalo::collections::Vec as BumpVec;

use crate::Parser;
use crate::error;

impl<'a> Parser<'a> {
    /// Parse an atomic pattern (no cons, as, or ctor args).
    ///
    /// Mirrors Elm's `Pattern.term`:
    /// ```haskell
    /// term =
    ///   do  start <- getPosition
    ///       oneOf E.PStart
    ///         [ record start
    ///         , tuple start
    ///         , list start
    ///         , termHelp start
    ///         ]
    /// ```
    pub fn pattern_term(&mut self) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        let start = self.get_position();

        self.one_of(
            error::Pattern::Start,
            vec![
                Box::new(|p: &mut Parser<'a>| p.pattern_record(start)),
                Box::new(|p| p.pattern_tuple(start)),
                Box::new(|p| p.pattern_list(start)),
                Box::new(|p| p.pattern_term_help(start)),
            ],
        )
    }

    /// Parse a full pattern expression including cons (::), as-patterns, and ctor with args.
    ///
    /// Returns `(pattern, end)` where `end` is the position at the end of the pattern
    /// (before any trailing whitespace was chomped). This is important for indent checking.
    ///
    /// Mirrors Elm's `Pattern.expression`:
    /// ```haskell
    /// expression :: Space.Parser E.Pattern Src.Pattern
    /// expression =
    ///   do  start <- getPosition
    ///       ePart <- exprPart
    ///       exprHelp start [] ePart
    /// ```
    pub fn pattern_expr(
        &mut self,
    ) -> Result<(&'a Located<Pattern<'a>>, Position), error::Pattern<'a>> {
        let start = self.get_position();
        let (first_pattern, first_end) = self.pattern_expr_part()?;
        self.pattern_expr_help(start, first_pattern, first_end)
    }

    /// Parse a pattern expression part (term or ctor with args).
    ///
    /// Mirrors Elm's `exprPart`.
    fn pattern_expr_part(
        &mut self,
    ) -> Result<(&'a Located<Pattern<'a>>, Position), error::Pattern<'a>> {
        let start = self.get_position();

        // Check if it starts with uppercase (constructor that might have args)
        if matches!(self.peek(), Some(b) if b.is_ascii_uppercase()) {
            let ctor_start = self.pos;
            self.advance();
            self.chomp_inner_chars();

            // Check for qualification
            let (region, module, name) = if self.is_dot_upper() || self.is_dot_lower() {
                self.pattern_ctor_qualified_parts(start, ctor_start)?
            } else {
                let name = self.slice_from(ctor_start);
                let end = self.get_position();
                (Region::new(start, end), None, name)
            };

            // Now try to parse arguments
            self.pattern_ctor_with_args(start, region, module, name)
        } else {
            // Regular term - chomp whitespace but return the end position
            // from BEFORE chomping (extracted from pattern's region)
            let pattern = self.pattern_term()?;
            let end = pattern.region.end;
            self.chomp(error::Pattern::Space)?;
            Ok((pattern, end))
        }
    }

    /// Parse qualified constructor parts, returning (region, module, name).
    fn pattern_ctor_qualified_parts(
        &mut self,
        start: Position,
        ctor_start: usize,
    ) -> Result<(Region, Option<&'a str>, &'a str), error::Pattern<'a>> {
        let (row, col) = self.position();

        // Keep chomping Module.Module... until we hit the final name
        loop {
            if self.is_dot_upper() {
                self.advance(); // consume dot
                self.advance(); // consume first uppercase char
                self.chomp_inner_chars();
            } else if self.is_dot_lower() {
                // Qualified lowercase - error for patterns
                return Err(error::Pattern::Start(row, col));
            } else {
                break;
            }
        }

        let full = self.slice_from(ctor_start);
        let end = self.get_position();
        let region = Region::new(start, end);

        if let Some(last_dot) = full.rfind('.') {
            let module = &full[..last_dot];
            let name = &full[last_dot + 1..];
            Ok((region, Some(module), name))
        } else {
            Ok((region, None, full))
        }
    }

    /// Parse constructor with potential arguments.
    fn pattern_ctor_with_args(
        &mut self,
        start: Position,
        region: Region,
        module: Option<&'a str>,
        name: &'a str,
    ) -> Result<(&'a Located<Pattern<'a>>, Position), error::Pattern<'a>> {
        let mut end = self.get_position();
        self.chomp(error::Pattern::Space)?;

        let mut args: BumpVec<'a, &'a Located<Pattern<'a>>> = BumpVec::new_in(self.bump);

        // Try to parse arguments (terms only, not full expressions)
        loop {
            let arg_result = self.one_of_with_fallback(
                vec![Box::new(|p: &mut Parser<'a>| {
                    // Check indent before trying to parse arg
                    let (check_row, check_col) = p.position();
                    p.check_indent(check_row, check_col, error::Pattern::IndentStart)?;

                    let arg = p.pattern_term()?;
                    Ok(Some(arg))
                })],
                None,
            )?;

            match arg_result {
                Some(arg) => {
                    args.push(arg);
                    end = self.get_position();
                    self.chomp(error::Pattern::Space)?;
                }
                None => break,
            }
        }

        let args_slice = args.into_bump_slice();
        let pattern = match module {
            Some(module) => Pattern::CtorQual {
                region,
                module,
                name,
                args: args_slice,
            },
            None => Pattern::Ctor {
                region,
                name,
                args: args_slice,
            },
        };

        Ok((self.add_end(start, pattern), end))
    }

    /// Parse the rest of a pattern expression (cons and as).
    ///
    /// Returns `(pattern, end)` where `end` is the position at the end of the pattern.
    ///
    /// Mirrors Elm's `exprHelp`.
    fn pattern_expr_help(
        &mut self,
        start: Position,
        pattern: &'a Located<Pattern<'a>>,
        end: Position,
    ) -> Result<(&'a Located<Pattern<'a>>, Position), error::Pattern<'a>> {
        let mut patterns: BumpVec<'a, &'a Located<Pattern<'a>>> = BumpVec::new_in(self.bump);
        let mut current = pattern;
        let mut current_end = end;

        loop {
            // Check indent at the end position (before chomp), not current parser position.
            // If indent check fails, we're done - return what we have.
            if self
                .check_indent(
                    current_end.line,
                    current_end.column,
                    error::Pattern::IndentStart,
                )
                .is_err()
            {
                let result = self.build_cons_chain(&mut patterns, current);
                return Ok((result, current_end));
            }

            // Try to parse :: or as
            let result = self.one_of_with_fallback(
                vec![
                    // Cons: `::`
                    Box::new(|p: &mut Parser<'a>| {
                        p.word2(0x3A, 0x3A, error::Pattern::Start)?; // ::
                        p.chomp_and_check_indent(error::Pattern::Space, error::Pattern::IndentStart)?;

                        let (next_pattern, next_end) = p.pattern_expr_part()?;
                        Ok(ConsOrAs::Cons(next_pattern, next_end))
                    }),
                    // As: `as name`
                    Box::new(|p: &mut Parser<'a>| {
                        // Check for "as" keyword
                        let (row, col) = p.position();
                        if !p.remaining().starts_with(b"as")
                            || matches!(p.peek_at(2), Some(b) if b.is_ascii_alphanumeric() || b == b'_')
                        {
                            return Err(error::Pattern::Start(row, col));
                        }
                        p.advance_by(2);

                        p.chomp_and_check_indent(error::Pattern::Space, error::Pattern::IndentAlias)?;

                        let name_start = p.get_position();
                        let name = p.lower_name(error::Pattern::Alias)?;
                        let name_end = p.get_position();
                        p.chomp(error::Pattern::Space)?;

                        let alias = p.add_end(name_start, name);
                        Ok(ConsOrAs::As(alias, name_end))
                    }),
                ],
                ConsOrAs::Done,
            )?;

            match result {
                ConsOrAs::Cons(next_pattern, next_end) => {
                    patterns.push(current);
                    current = next_pattern;
                    current_end = next_end;
                }
                ConsOrAs::As(alias, alias_end) => {
                    // Build up cons chain first
                    let base = self.build_cons_chain(&mut patterns, current);
                    // Then wrap in alias
                    return Ok((
                        self.add_end(
                            start,
                            Pattern::Alias {
                                pattern: base,
                                name: alias,
                            },
                        ),
                        alias_end,
                    ));
                }
                ConsOrAs::Done => {
                    // Build up cons chain
                    let result = self.build_cons_chain(&mut patterns, current);
                    return Ok((result, current_end));
                }
            }
        }
    }

    /// Build a cons chain from accumulated patterns.
    fn build_cons_chain(
        &self,
        patterns: &mut BumpVec<'a, &'a Located<Pattern<'a>>>,
        tail: &'a Located<Pattern<'a>>,
    ) -> &'a Located<Pattern<'a>> {
        if patterns.is_empty() {
            tail
        } else {
            // Build right-to-left: a :: b :: c => Cons(a, Cons(b, c))
            let mut result = tail;
            while let Some(head) = patterns.pop() {
                let region = Region::new(
                    Position::new(head.region.start.line, head.region.start.column),
                    Position::new(result.region.end.line, result.region.end.column),
                );
                result = self.alloc(Located::at(region, Pattern::Cons { head, tail: result }));
            }
            result
        }
    }
}

/// Helper enum for pattern expression parsing.
enum ConsOrAs<'a> {
    Cons(&'a Located<Pattern<'a>>, Position),
    As(&'a Located<&'a str>, Position),
    Done,
}

/// Snapshot test macro for successful pattern parsing.
#[cfg(test)]
macro_rules! assert_pattern_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let src = bump.alloc_str(indoc::indoc!($code));
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let (result, _end) = parser.pattern_expr().expect("expected successful parse");

        insta::with_settings!({
            description => format!("Code:\n\n{}", indoc::indoc!($code)),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }};
}

/// Snapshot test macro for pattern parse errors.
#[cfg(test)]
macro_rules! assert_pattern_error_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let src = bump.alloc_str(indoc::indoc!($code));
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let result = parser.pattern_expr().expect_err("expected parse error");

        insta::with_settings!({
            description => format!("Code:\n\n{}", indoc::indoc!($code)),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }};
}

/// Snapshot test macro for multiline patterns, laid out as they would
/// appear indented inside a definition (see `test_support::indent_fragment`).
#[cfg(test)]
macro_rules! assert_indented_pattern_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let fragment = indoc::indoc!($code);
        let indented = $crate::test_support::indent_fragment(fragment);
        let src = bump.alloc_str(&indented);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        parser
            .chomp(|_, _, _| "space error")
            .expect("expected leading indent");
        let (result, _end) = parser.pattern_expr().expect("expected successful parse");

        insta::with_settings!({
            description => format!("Code (indented inside a def):\n\n{}", indented),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }};
}

#[cfg(test)]
pub(crate) use assert_indented_pattern_snapshot;
#[cfg(test)]
pub(crate) use assert_pattern_error_snapshot;
#[cfg(test)]
pub(crate) use assert_pattern_snapshot;

#[cfg(test)]
mod tests {
    // Cons patterns
    #[test]
    fn cons_simple() {
        assert_pattern_snapshot!("head :: tail");
    }

    #[test]
    fn cons_multiple() {
        assert_pattern_snapshot!("a :: b :: c");
    }

    #[test]
    fn cons_with_empty_list() {
        assert_pattern_snapshot!("head :: []");
    }

    // As patterns
    #[test]
    fn as_simple() {
        assert_pattern_snapshot!("x as foo");
    }

    #[test]
    fn as_with_tuple() {
        assert_pattern_snapshot!("(a, b) as pair");
    }

    #[test]
    fn as_with_cons() {
        assert_pattern_snapshot!("head :: tail as list");
    }

    // Constructor with args
    #[test]
    fn ctor_with_one_arg() {
        assert_pattern_snapshot!("Just x");
    }

    #[test]
    fn ctor_with_multiple_args() {
        assert_pattern_snapshot!("Node left value right");
    }

    #[test]
    fn ctor_qualified_with_args() {
        assert_pattern_snapshot!("Maybe.Just x");
    }

    #[test]
    fn ctor_nested() {
        assert_pattern_snapshot!("Just (Just x)");
    }

    // Complex combinations
    #[test]
    fn complex_pattern() {
        assert_pattern_snapshot!("Just x :: rest as list");
    }
}
