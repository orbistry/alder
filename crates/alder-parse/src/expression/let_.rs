//! Let expression parsing for Alder.
//!
//! Ported from Elm's `Parse/Expression.hs` (let_, chompLetDefs, definition, destructure).

use alder_region::{Located, Position};
use alder_source::{Def, Expr};
use bumpalo::collections::Vec as BumpVec;

use crate::Parser;
use crate::error::{self, Def as DefErr, Destruct, Let};

impl<'a> Parser<'a> {
    /// Parse a let expression.
    ///
    /// Mirrors Elm's `let_`:
    /// ```haskell
    /// let_ start =
    ///   inContext E.Let (Keyword.let_ E.Start) $
    ///     do  (defs, defsEnd) <-
    ///           withBacksetIndent 3 $
    ///             do  Space.chompAndCheckIndent E.LetSpace E.LetIndentDef
    ///                 withIndent $
    ///                   do  (def, end) <- chompLetDef
    ///                       chompLetDefs [def] end
    ///         Space.checkIndent defsEnd E.LetIndentIn
    ///         Keyword.in_ E.LetIn
    ///         Space.chompAndCheckIndent E.LetSpace E.LetIndentBody
    ///         (body, end) <- specialize E.LetBody expression
    ///         return (A.at start end (Src.Let defs body), end)
    /// ```
    pub(crate) fn let_(
        &mut self,
        start: Position,
    ) -> Result<(&'a Located<Expr<'a>>, Position), error::Expr<'a>> {
        self.in_context(
            |bump, e, row, col| error::Expr::Let(bump.alloc(e), row, col),
            |p| p.keyword_let(error::Expr::Start),
            |p| {
                // Parse definitions with backset indent (3 for "let")
                let (defs, defs_end) = p.with_backset_indent(3, |p| {
                    p.chomp_and_check_indent(Let::Space, Let::IndentDef)?;
                    p.with_indent(|p| {
                        let (first_def, first_end) = p.chomp_let_def()?;
                        p.chomp_let_defs(vec![first_def], first_end)
                    })
                })?;

                // Check indent for "in" keyword
                p.check_indent(defs_end.line, defs_end.column, Let::IndentIn)?;

                // Parse "in" keyword
                p.keyword_in(Let::In)?;

                // Chomp whitespace and check indent for body
                p.chomp_and_check_indent(Let::Space, Let::IndentBody)?;

                // Parse body expression
                let (body, end) = p.specialize(
                    |bump, e, row, col| Let::Body(bump.alloc(e), row, col),
                    |p| p.expression(),
                )?;

                // Build the let expression
                let defs_slice = p.alloc_slice_copy(&defs);
                let let_expr = Expr::Let {
                    defs: defs_slice,
                    body,
                };

                Ok((p.add_end(start, let_expr), end))
            },
        )
    }

    /// Parse remaining let definitions.
    ///
    /// Mirrors Elm's `chompLetDefs`:
    /// ```haskell
    /// chompLetDefs revDefs end =
    ///   oneOfWithFallback
    ///     [ do  Space.checkAligned E.LetDefAlignment
    ///           (def, newEnd) <- chompLetDef
    ///           chompLetDefs (def:revDefs) newEnd
    ///     ]
    ///     (reverse revDefs, end)
    /// ```
    fn chomp_let_defs(
        &mut self,
        mut defs: Vec<&'a Located<Def<'a>>>,
        end: Position,
    ) -> Result<(Vec<&'a Located<Def<'a>>>, Position), Let<'a>> {
        let defs_for_fallback = defs.clone();

        self.one_of_with_fallback(
            vec![Box::new(|p: &mut Parser<'a>| {
                // Check alignment for next definition
                p.check_aligned(Let::DefAlignment)?;

                // Parse the next definition
                let (def, new_end) = p.chomp_let_def()?;
                defs.push(def);

                // Continue parsing more definitions
                p.chomp_let_defs(defs, new_end)
            })],
            (defs_for_fallback, end),
        )
    }

    /// Parse a single let definition (value or destructure).
    ///
    /// Mirrors Elm's `chompLetDef`:
    /// ```haskell
    /// chompLetDef =
    ///   oneOf E.LetDefName
    ///     [ definition
    ///     , destructure
    ///     ]
    /// ```
    fn chomp_let_def(&mut self) -> Result<(&'a Located<Def<'a>>, Position), Let<'a>> {
        self.one_of(
            Let::DefName,
            vec![
                Box::new(|p: &mut Parser<'a>| p.definition()),
                Box::new(|p| p.destructure()),
            ],
        )
    }

    /// Parse a value definition: `name args = body` or `name : Type \n name args = body`.
    ///
    /// Mirrors Elm's `definition`.
    fn definition(&mut self) -> Result<(&'a Located<Def<'a>>, Position), Let<'a>> {
        let start = self.get_position();
        let name = self.lower_name(Let::DefName)?;
        let name_located = self.add_end(start, name);

        self.specialize(
            |bump, e, row, col| Let::Def(name, bump.alloc(e), row, col),
            |p| {
                p.chomp_and_check_indent(DefErr::Space, DefErr::IndentEquals)?;

                p.one_of(
                    DefErr::Equals,
                    vec![
                        // Type annotation: name : Type \n name args = body
                        Box::new(|p: &mut Parser<'a>| {
                            p.word1(b':', DefErr::Equals)?;
                            p.chomp_and_check_indent(DefErr::Space, DefErr::IndentType)?;

                            let (type_ann, _) = p.specialize(
                                |bump, e, row, col| DefErr::Type(bump.alloc(e), row, col),
                                |p| p.type_expr(),
                            )?;

                            // type_expr already chomps trailing whitespace
                            p.check_aligned(DefErr::Alignment)?;
                            let def_name = p.chomp_matching_name(name)?;
                            p.chomp_and_check_indent(DefErr::Space, DefErr::IndentEquals)?;
                            p.chomp_def_args_and_body(start, def_name, Some(type_ann))
                        }),
                        // No type annotation: name args = body
                        Box::new(|p: &mut Parser<'a>| {
                            p.chomp_def_args_and_body(start, name_located, None)
                        }),
                    ],
                )
            },
        )
    }

    /// Parse function arguments and body.
    ///
    /// Mirrors Elm's `chompDefArgsAndBody`.
    fn chomp_def_args_and_body(
        &mut self,
        start: Position,
        name: &'a Located<&'a str>,
        type_ann: Option<&'a Located<alder_source::Type<'a>>>,
    ) -> Result<(&'a Located<Def<'a>>, Position), DefErr<'a>> {
        let mut args: BumpVec<'a, &'a Located<alder_source::Pattern<'a>>> =
            BumpVec::new_in(self.bump);

        loop {
            let args_for_body = args.clone();

            let result = self.one_of(
                DefErr::Equals,
                vec![
                    // Parse an argument pattern
                    Box::new(|p: &mut Parser<'a>| {
                        let arg = p.specialize(
                            |bump, e, row, col| DefErr::Arg(bump.alloc(e), row, col),
                            |p| p.pattern_term(),
                        )?;
                        p.chomp_and_check_indent(DefErr::Space, DefErr::IndentEquals)?;
                        args.push(arg);
                        Ok(DefinitionState::MoreArgs)
                    }),
                    // Parse the body
                    Box::new(|p: &mut Parser<'a>| {
                        p.word1(b'=', DefErr::Equals)?;
                        p.chomp_and_check_indent(DefErr::Space, DefErr::IndentBody)?;
                        let (body, end) = p.specialize(
                            |bump, e, row, col| DefErr::Body(bump.alloc(e), row, col),
                            |p| p.expression(),
                        )?;

                        let args_slice = args_for_body.into_bump_slice();
                        let def = Def::Define {
                            name,
                            args: args_slice,
                            body,
                            annotation: type_ann,
                        };
                        Ok(DefinitionState::Done(p.add_end(start, def), end))
                    }),
                ],
            )?;

            match result {
                DefinitionState::MoreArgs => continue,
                DefinitionState::Done(def, end) => return Ok((def, end)),
            }
        }
    }

    /// Check that the name matches the expected name (for type-annotated definitions).
    fn chomp_matching_name(&mut self, expected: &str) -> Result<&'a Located<&'a str>, DefErr<'a>> {
        let start = self.get_position();
        let name = self.lower_name(DefErr::NameRepeat)?;
        let name_located = self.add_end(start, name);

        if name == expected {
            Ok(name_located)
        } else {
            let (row, col) = self.position();
            Err(DefErr::NameMatch(name, row, col))
        }
    }

    /// Parse a destructuring definition: `pattern = expr`.
    ///
    /// Mirrors Elm's `destructure`.
    fn destructure(&mut self) -> Result<(&'a Located<Def<'a>>, Position), Let<'a>> {
        self.specialize(
            |bump, e, row, col| Let::Destruct(bump.alloc(e), row, col),
            |p| {
                let start = p.get_position();
                let pattern = p.specialize(
                    |bump, e, row, col| Destruct::Pattern(bump.alloc(e), row, col),
                    |p| p.pattern_term(),
                )?;

                p.chomp_and_check_indent(Destruct::Space, Destruct::IndentEquals)?;
                p.word1(b'=', Destruct::Equals)?;
                p.chomp_and_check_indent(Destruct::Space, Destruct::IndentBody)?;

                let (expr, end) = p.specialize(
                    |bump, e, row, col| Destruct::Body(bump.alloc(e), row, col),
                    |p| p.expression(),
                )?;

                let def = Def::Destruct {
                    pattern,
                    body: expr,
                };
                Ok((p.add_end(start, def), end))
            },
        )
    }
}

/// State for parsing definition arguments.
enum DefinitionState<'a> {
    MoreArgs,
    Done(&'a Located<Def<'a>>, Position),
}

#[cfg(test)]
mod tests {
    use crate::expression::{assert_expression_snapshot, assert_indented_expression_snapshot};

    #[test]
    fn let_simple() {
        assert_expression_snapshot!("let x = 1 in x");
    }

    #[test]
    fn let_multiline() {
        assert_indented_expression_snapshot!(
            r#"
            let
                x = 1
            in
                x
        "#
        );
    }

    #[test]
    fn let_multiple_defs() {
        assert_indented_expression_snapshot!(
            r#"
            let
                x = 1
                y = 2
            in
                x
        "#
        );
    }

    #[test]
    fn let_with_function() {
        assert_indented_expression_snapshot!(
            r#"
            let
                f x = x
            in
                f 1
        "#
        );
    }

    #[test]
    fn let_with_type_annotation() {
        assert_indented_expression_snapshot!(
            r#"
            let
                f : Int -> Int
                f x = x
            in
                f 1
        "#
        );
    }

    #[test]
    fn let_destructure() {
        assert_indented_expression_snapshot!(
            r#"
            let
                (a, b) = pair
            in
                a
        "#
        );
    }

    #[test]
    fn let_nested() {
        assert_indented_expression_snapshot!(
            r#"
            let
                x =
                    let
                        y = 1
                    in
                        y
            in
                x
        "#
        );
    }
}
