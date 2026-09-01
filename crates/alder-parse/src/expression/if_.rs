//! If expression parsing for Alder.
//!
//! Ported from Elm's `Parse/Expression.hs` (if_, chompIfEnd).
//!
//! Parses: `if cond then branch else branch`
//! Also handles: `if c1 then b1 else if c2 then b2 else b3`

use alder_region::{Located, Position, Region};
use alder_source::{Expr, IfBranch};
use bumpalo::collections::Vec as BumpVec;

use crate::Parser;
use crate::error::{self, If};

impl<'a> Parser<'a> {
    /// Parse an if expression: `if cond then branch else branch`.
    ///
    /// Mirrors Elm's `if_`:
    /// ```haskell
    /// if_ start =
    ///   inContext E.If (Keyword.if_ E.Start) $
    ///     chompIfEnd start []
    /// ```
    pub(crate) fn if_(
        &mut self,
        start: Position,
    ) -> Result<(&'a Located<Expr<'a>>, Position), error::Expr<'a>> {
        self.in_context(
            |bump, e, row, col| error::Expr::If(bump.alloc(e), row, col),
            |p| p.keyword_if(error::Expr::Start),
            |p| p.chomp_if_end(start, vec![]),
        )
    }

    /// Parse the body of an if expression after the initial `if` keyword.
    ///
    /// Mirrors Elm's `chompIfEnd`:
    /// ```haskell
    /// chompIfEnd start branches =
    ///   do  Space.chompAndCheckIndent E.IfSpace E.IfIndentCondition
    ///       (condition, condEnd) <- specialize E.IfCondition expression
    ///       Space.checkIndent condEnd E.IfIndentThen
    ///       Keyword.then_ E.IfThen
    ///       Space.chompAndCheckIndent E.IfSpace E.IfIndentThenBranch
    ///       (thenBranch, thenEnd) <- specialize E.IfThenBranch expression
    ///       Space.checkIndent thenEnd E.IfIndentElse
    ///       Keyword.else_ E.IfElse
    ///       Space.chompAndCheckIndent E.IfSpace E.IfIndentElseBranch
    ///       let newBranches = (condition, thenBranch) : branches
    ///       oneOf E.IfElseBranchStart
    ///         [ do  Keyword.if_ E.IfElseBranchStart
    ///               chompIfEnd start newBranches
    ///         , do  (elseBranch, elseEnd) <- specialize E.IfElseBranch expression
    ///               let ifExpr = Src.If (reverse newBranches) elseBranch
    ///               return (A.at start elseEnd ifExpr, elseEnd)
    ///         ]
    /// ```
    fn chomp_if_end(
        &mut self,
        start: Position,
        mut branches: Vec<&'a IfBranch<'a>>,
    ) -> Result<(&'a Located<Expr<'a>>, Position), If<'a>> {
        // Parse condition
        self.chomp_and_check_indent(If::Space, If::IndentCondition)?;
        let (condition, cond_end) = self.if_condition()?;

        // Parse `then`
        self.check_indent(cond_end.line, cond_end.column, If::IndentThen)?;
        self.keyword_then(If::Then)?;

        // Parse then branch
        self.chomp_and_check_indent(If::Space, If::IndentThenBranch)?;
        let (then_branch, then_end) = self.if_then_branch()?;

        // Parse `else`
        self.check_indent(then_end.line, then_end.column, If::IndentElse)?;
        self.keyword_else(If::Else)?;

        // Create the new branch
        let branch = self.bump.alloc(IfBranch {
            condition,
            then_branch,
        });
        branches.push(branch);

        // Parse else branch: either `else if ...` or final else expression
        self.chomp_and_check_indent(If::Space, If::IndentElseBranch)?;

        // Clone for second closure
        let branches_for_else = branches.clone();

        self.one_of(
            If::ElseBranchStart,
            vec![
                // `else if ...` - continue the chain
                Box::new(|p: &mut Parser<'a>| {
                    p.keyword_if(If::ElseBranchStart)?;
                    p.chomp_if_end(start, branches)
                }),
                // Final else expression
                Box::new(|p: &mut Parser<'a>| {
                    let (else_branch, else_end) = p.if_else_branch()?;

                    // Convert branches to bump slice
                    // Note: Elm reverses because it uses `:` (prepend), we use push (append)
                    // so our branches are already in correct order
                    let mut branch_vec: BumpVec<'a, &'a IfBranch<'a>> = BumpVec::new_in(p.bump);
                    for b in branches_for_else {
                        branch_vec.push(b);
                    }
                    let branches_slice = branch_vec.into_bump_slice();

                    let if_expr = Expr::If {
                        branches: branches_slice,
                        final_else: else_branch,
                    };

                    Ok((
                        p.alloc(Located::at(Region::new(start, else_end), if_expr)),
                        else_end,
                    ))
                }),
            ],
        )
    }

    /// Parse condition expression in an if.
    fn if_condition(&mut self) -> Result<(&'a Located<Expr<'a>>, Position), If<'a>> {
        self.specialize(
            |bump, e, r, c| If::Condition(bump.alloc(e), r, c),
            |p| p.expression(),
        )
    }

    /// Parse then branch expression in an if.
    fn if_then_branch(&mut self) -> Result<(&'a Located<Expr<'a>>, Position), If<'a>> {
        self.specialize(
            |bump, e, r, c| If::ThenBranch(bump.alloc(e), r, c),
            |p| p.expression(),
        )
    }

    /// Parse else branch expression in an if.
    fn if_else_branch(&mut self) -> Result<(&'a Located<Expr<'a>>, Position), If<'a>> {
        self.specialize(
            |bump, e, r, c| If::ElseBranch(bump.alloc(e), r, c),
            |p| p.expression(),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::expression::{assert_expression_snapshot, assert_indented_expression_snapshot};

    #[test]
    fn if_simple() {
        assert_expression_snapshot!("if True then 1 else 2");
    }

    #[test]
    fn if_with_vars() {
        assert_expression_snapshot!("if x then y else z");
    }

    #[test]
    fn if_else_if() {
        assert_expression_snapshot!("if a then 1 else if b then 2 else 3");
    }

    #[test]
    fn if_multiline() {
        assert_indented_expression_snapshot!(
            r#"
            if condition then
                trueBranch
            else
                falseBranch
        "#
        );
    }

    #[test]
    fn if_nested_condition() {
        assert_expression_snapshot!("if f x then 1 else 2");
    }

    #[test]
    fn if_nested_branches() {
        assert_expression_snapshot!("if a then f x else g y");
    }

    #[test]
    fn if_else_if_multiline() {
        assert_indented_expression_snapshot!(
            r#"
            if a then
                1
            else if b then
                2
            else
                3
        "#
        );
    }
}
