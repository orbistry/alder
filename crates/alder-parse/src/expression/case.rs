//! Case expression parsing for Alder.
//!
//! Ported from Elm's `Parse/Expression.hs` (case_, chompBranch, chompCaseEnd).

use alder_region::{Located, Position};
use alder_source::{CaseArm, Expr};

use crate::Parser;
use crate::error::{self, Case};

impl<'a> Parser<'a> {
    /// Parse a case expression.
    ///
    /// Mirrors Elm's `case_`:
    /// ```haskell
    /// case_ start =
    ///   inContext E.Case (Keyword.case_ E.Start) $
    ///     do  Space.chompAndCheckIndent E.CaseSpace E.CaseIndentExpr
    ///         (expr, exprEnd) <- specialize E.CaseExpr expression
    ///         Space.checkIndent exprEnd E.CaseIndentOf
    ///         Keyword.of_ E.CaseOf
    ///         Space.chompAndCheckIndent E.CaseSpace E.CaseIndentPattern
    ///         withIndent $
    ///           do  (firstBranch, firstEnd) <- chompBranch
    ///               (branches, end) <- chompCaseEnd [firstBranch] firstEnd
    ///               return (A.at start end (Src.Case expr branches), end)
    /// ```
    pub(crate) fn case_(
        &mut self,
        start: Position,
    ) -> Result<(&'a Located<Expr<'a>>, Position), error::Expr<'a>> {
        self.in_context(
            |bump, e, row, col| error::Expr::Case(bump.alloc(e), row, col),
            |p| p.keyword_case(error::Expr::Start),
            |p| {
                // Chomp whitespace and check indent for scrutinee expression
                p.chomp_and_check_indent(Case::Space, Case::IndentExpr)?;

                // Parse the scrutinee expression
                let (scrutinee, scrutinee_end) = p.specialize(
                    |bump, e, row, col| Case::Expr(bump.alloc(e), row, col),
                    |p| p.expression(),
                )?;

                // Check indent for "of" keyword
                p.check_indent(scrutinee_end.line, scrutinee_end.column, Case::IndentOf)?;

                // Parse "of" keyword
                p.keyword_of(Case::Of)?;

                // Chomp whitespace and check indent for first pattern
                p.chomp_and_check_indent(Case::Space, Case::IndentPattern)?;

                // Parse branches with proper indentation
                p.with_indent(|p| {
                    // Parse first branch
                    let (first_arm, first_end) = p.chomp_case_branch()?;

                    // Parse remaining branches
                    let (arms, end) = p.chomp_case_end(vec![first_arm], first_end)?;

                    // Build the case expression
                    let arms_slice = p.alloc_slice_copy(&arms);
                    let case_expr = Expr::Case {
                        scrutinee,
                        arms: arms_slice,
                    };

                    Ok((p.add_end(start, case_expr), end))
                })
            },
        )
    }

    /// Parse a single case branch (pattern -> expression).
    ///
    /// Mirrors Elm's `chompBranch`:
    /// ```haskell
    /// chompBranch =
    ///   do  (pattern, patternEnd) <- specialize E.CasePattern Pattern.expression
    ///       Space.checkIndent patternEnd E.CaseIndentArrow
    ///       word2 0x2D 0x3E {-->-} E.CaseArrow
    ///       Space.chompAndCheckIndent E.CaseSpace E.CaseIndentBranch
    ///       (branchExpr, end) <- specialize E.CaseBranch expression
    ///       return ((pattern, branchExpr), end)
    /// ```
    fn chomp_case_branch(&mut self) -> Result<(&'a CaseArm<'a>, Position), Case<'a>> {
        // Parse the pattern
        let (pattern, pattern_end) = self.specialize(
            |bump, e, row, col| Case::Pattern(bump.alloc(e), row, col),
            |p| p.pattern_expr(),
        )?;

        // Check indent for arrow
        self.check_indent(pattern_end.line, pattern_end.column, Case::IndentArrow)?;

        // Parse the arrow ->
        self.word2(b'-', b'>', Case::Arrow)?;

        // Chomp whitespace and check indent for branch body
        self.chomp_and_check_indent(Case::Space, Case::IndentBranch)?;

        // Parse the branch expression
        let (body, end) = self.specialize(
            |bump, e, row, col| Case::Branch(bump.alloc(e), row, col),
            |p| p.expression(),
        )?;

        let arm = self.alloc(CaseArm { pattern, body });
        Ok((arm, end))
    }

    /// Parse remaining case branches.
    ///
    /// Mirrors Elm's `chompCaseEnd`:
    /// ```haskell
    /// chompCaseEnd branches end =
    ///   oneOfWithFallback
    ///     [ do  Space.checkAligned E.CasePatternAlignment
    ///           (branch, newEnd) <- chompBranch
    ///           chompCaseEnd (branch:branches) newEnd
    ///     ]
    ///     (reverse branches, end)
    /// ```
    fn chomp_case_end(
        &mut self,
        mut arms: Vec<&'a CaseArm<'a>>,
        end: Position,
    ) -> Result<(Vec<&'a CaseArm<'a>>, Position), Case<'a>> {
        // Clone for the fallback
        let arms_for_fallback = arms.clone();

        self.one_of_with_fallback(
            vec![Box::new(|p: &mut Parser<'a>| {
                // Check alignment for next pattern
                p.check_aligned(Case::PatternAlignment)?;

                // Parse the next branch
                let (arm, new_end) = p.chomp_case_branch()?;
                arms.push(arm);

                // Continue parsing more branches
                p.chomp_case_end(arms, new_end)
            })],
            (arms_for_fallback, end),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::expression::assert_expression_snapshot;

    #[test]
    fn case_simple() {
        assert_expression_snapshot!("case x of\n    Just y -> y\n    Nothing -> 0");
    }

    #[test]
    fn case_single_branch() {
        assert_expression_snapshot!("case x of\n    _ -> 1");
    }

    #[test]
    fn case_multiple_branches() {
        assert_expression_snapshot!(
            r#"
            case x of
                0 -> "zero"
                1 -> "one"
                _ -> "other"
        "#
        );
    }

    #[test]
    fn case_with_patterns() {
        assert_expression_snapshot!(
            r#"
            case list of
                [] -> 0
                [x] -> x
                x :: xs -> x
        "#
        );
    }

    #[test]
    fn case_nested() {
        assert_expression_snapshot!(
            r#"
            case x of
                Just y ->
                    case y of
                        Just z -> z
                        Nothing -> 0
                Nothing -> 0
        "#
        );
    }
}
