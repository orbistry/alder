//! `match` expressions and the arm head shared with `@match`.
//!
//! See docs/parser-internals.md §5.13:
//!
//! ```ebnf
//! match_expr = 'match' expression '{' { match_arm } '}' ;
//! match_arm  = pattern { '|' pattern } [ 'if' expression ] '=>' ( block | expression ) [ ',' ] ;
//! ```
//!
//! The scrutinee is parsed under `no_record_ctor` (§2.3) so
//! `match shape { … }` opens the arms rather than a record constructor;
//! the arms run with constructors allowed again. A body starting with `{`
//! is always a block (§2.2). The comma after an arm is optional, as the
//! grammar and `Match::End` ("expected `,`, a pattern, or `}`") say: an
//! arm may follow the previous body directly, usually on the next line.
//! Guards are not heads: a `=>` always separates them from any `{`, so
//! they inherit the surrounding constructor setting.
// OWNER: expression/match_.rs (Wave 2)

use alder_region::{Located, Position};
use alder_source::{Expr, MatchArm, Pattern};
use bumpalo::collections::Vec as BumpVec;

use crate::{Col, Parser, Row, error};

// `primary()` (expression/mod.rs) is the only caller; the allow goes away
// with the Wave 4 sweep (docs/parser-internals.md §9 step 4.2).
#[allow(unused)]
impl<'a> Parser<'a> {
    /// After `match`.
    pub(crate) fn match_(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let (row, col) = (start.line, start.column);
        let (scrutinee, arms) = self
            .match_body()
            .map_err(|e| error::Expr::Match(self.alloc(e), row, col))?;
        Ok(self.add_end(start, Expr::Match { scrutinee, arms }))
    }

    /// Scrutinee and arms; consumes through the closing `}`.
    #[allow(clippy::type_complexity)]
    fn match_body(
        &mut self,
    ) -> Result<(&'a Located<Expr<'a>>, &'a [MatchArm<'a>]), error::Match<'a>> {
        self.chomp();
        let scrutinee = self.specialize(
            |bump, e, row, col| error::Match::Scrutinee(bump.alloc(e), row, col),
            |p| p.with_record_ctor(false, |p| p.expression()),
        )?;
        // `expression()` chomped; an Elm-style `of` lands on `Match::Open`
        // here, where the renderer can spot it.
        self.word1(b'{', error::Match::Open)?;
        self.chomp();
        let arms = self.with_record_ctor(true, |p| p.match_arms())?;
        Ok((scrutinee, arms))
    }

    /// Arms until the closing `}` (consumed).
    fn match_arms(&mut self) -> Result<&'a [MatchArm<'a>], error::Match<'a>> {
        let mut arms = BumpVec::new_in(self.bump);
        // After `{` or `,` an arm (or `}`) is required; after a comma-less
        // body anything that is not a pattern start is `Match::End`.
        let mut expect_arm = true;
        loop {
            if self.peek() == Some(b'}') {
                self.advance();
                return Ok(arms.into_bump_slice());
            }
            let (row, col) = self.position();
            match self.match_arm() {
                Ok(arm) => arms.push(arm),
                Err(error::Arm::Pattern(error::Pattern::Start(..), r, c))
                    if !expect_arm && (r, c) == (row, col) =>
                {
                    return Err(error::Match::End(r, c));
                }
                Err(e) => return Err(error::Match::Arm(self.alloc(e), row, col)),
            }
            // The body (`expression()` or `block()`) chomped.
            expect_arm = self.peek() == Some(b',');
            if expect_arm {
                self.advance();
                self.chomp();
            }
        }
    }

    /// One arm: head, then `{ block }` or an expression.
    fn match_arm(&mut self) -> Result<MatchArm<'a>, error::Arm<'a>> {
        let (patterns, guard) =
            self.arm_head(error::Arm::Pattern, error::Arm::Guard, error::Arm::Arrow)?;
        let body = if self.peek() == Some(b'{') {
            let block = self.specialize(
                |bump, e, row, col| error::Arm::Block(bump.alloc(e), row, col),
                |p| p.block(),
            )?;
            self.alloc(Located::at(block.region, Expr::Block(block)))
        } else {
            self.specialize(
                |bump, e, row, col| error::Arm::Body(bump.alloc(e), row, col),
                |p| p.expression(),
            )?
        };
        Ok(MatchArm {
            patterns,
            guard,
            body,
        })
    }

    /// `p | q [if guard] =>` — shared with @match (errors mapped by the caller).
    /// Chomps after the `=>`, leaving the cursor on the body.
    // Required, not a stub allow: the §5.13 return type trips clippy's
    // `type_complexity` under `-D warnings`. Step 4.2 strips `allow(unused)`
    // only; this one stays.
    #[allow(clippy::type_complexity)]
    pub(crate) fn arm_head<E>(
        &mut self,
        to_pattern: impl FnOnce(&'a error::Pattern<'a>, Row, Col) -> E,
        to_guard: impl FnOnce(&'a error::Expr<'a>, Row, Col) -> E,
        to_arrow: impl FnOnce(Row, Col) -> E,
    ) -> Result<
        (
            &'a [&'a Located<Pattern<'a>>],
            Option<&'a Located<Expr<'a>>>,
        ),
        E,
    > {
        // `pattern_alternatives()` chomps after each pattern.
        let patterns = self.specialize(
            |bump, e, row, col| to_pattern(bump.alloc(e), row, col),
            |p| p.pattern_alternatives(),
        )?;
        let guard = if self.peek_keyword(b"if") {
            self.advance_by(2);
            self.chomp();
            Some(self.specialize(
                |bump, e, row, col| to_guard(bump.alloc(e), row, col),
                |p| p.expression(),
            )?)
        } else {
            None
        };
        self.word2(b'=', b'>', to_arrow)?;
        self.chomp();
        Ok((patterns, guard))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{assert_expression_error_snapshot, assert_expression_snapshot};

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn match_simple() {
        assert_expression_snapshot!(
            r#"
            match x {
                1 => "one",
            }
        "#
        );
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn match_multiple_arms() {
        assert_expression_snapshot!(
            r#"
            match x {
                1 => "one",
                2 => "two",
                _ => "many",
            }
        "#
        );
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn match_trailing_comma() {
        assert_expression_snapshot!("match x { 1 => a, }");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn match_no_trailing_comma() {
        assert_expression_snapshot!("match x { 1 => a, 2 => b }");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn match_newline_separated_arms() {
        assert_expression_snapshot!(
            r#"
            match x {
                1 => "one"
                2 => "two"
            }
        "#
        );
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn match_empty() {
        assert_expression_snapshot!("match x {}");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn match_alternatives() {
        assert_expression_snapshot!("match x { 1 | 2 | 3 => small, _ => big }");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn match_guard() {
        assert_expression_snapshot!("match n { n if n > 0 => pos, _ => other }");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn match_alternatives_with_guard() {
        assert_expression_snapshot!("match x { 1 | 2 if ok => a, _ => b }");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn match_block_body() {
        assert_expression_snapshot!(
            r#"
            match x {
                Some(v) => {
                    let y = v * 2
                    y
                }
                None => 0,
            }
        "#
        );
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn match_block_single_name_is_block() {
        assert_expression_snapshot!("match x { _ => { y } }");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn match_ctor_args() {
        assert_expression_snapshot!(
            r#"
            match u.nickname {
                Some(n) => n,
                None => u.name,
            }
        "#
        );
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn match_tag_patterns() {
        assert_expression_snapshot!(
            r#"
            match load(id) {
                Ok(p) => render(p),
                Err(:not_found(id)) => notFound(id),
                Err(:timeout) => retry(),
                Err(_) => fail(),
            }
        "#
        );
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn match_wildcard() {
        assert_expression_snapshot!("match x { _ => 0 }");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn match_pin() {
        assert_expression_snapshot!(
            r#"
            match input {
                ^expected => "matched the existing value",
                other => `got ${other}`,
            }
        "#
        );
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn match_scrutinee_path_no_record_ctor() {
        assert_expression_snapshot!("match Shape::Empty { Shape::Empty => 1 }");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn match_record_ctor_in_arm() {
        assert_expression_snapshot!("match s { _ => Shape::Rect { width: 1 } }");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn error_missing_arrow_thin_arrow() {
        assert_expression_error_snapshot!("match x { 1 -> a }");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn error_of_keyword() {
        assert_expression_error_snapshot!("match x of { 1 => a }");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn error_missing_body() {
        assert_expression_error_snapshot!("match x { 1 => }");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn error_unclosed() {
        assert_expression_error_snapshot!("match x { 1 => a");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn error_bad_pattern() {
        assert_expression_error_snapshot!("match x { +1 => a }");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn error_guard() {
        assert_expression_error_snapshot!("match x { n if => a }");
    }

    #[test]
    #[ignore = "waits for expression/mod.rs"]
    fn error_end_after_body() {
        assert_expression_error_snapshot!("match x { 1 => a ) }");
    }
}
