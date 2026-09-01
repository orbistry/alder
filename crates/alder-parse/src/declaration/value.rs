//! Value definition parsing for Alder.
//!
//! Ported from Elm's `Parse/Declaration.hs` (valueDecl, chompDefArgsAndBody).

use alder_region::{Located, Position};
use alder_source::{Comment, Value};
use bumpalo::collections::Vec as BumpVec;

use super::Decl;
use crate::Parser;
use crate::error::{self, DeclDef};

impl<'a> Parser<'a> {
    /// Parse a value definition.
    ///
    /// Mirrors Elm's `valueDecl`:
    /// ```haskell
    /// valueDecl maybeDocs start =
    ///   do  name <- Var.lower E.DeclStart
    ///       end <- getPosition
    ///       specialize (E.DeclDef name) $
    ///         do  Space.chompAndCheckIndent E.DeclDefSpace E.DeclDefIndentEquals
    ///             oneOf E.DeclDefEquals
    ///               [ -- type annotation: name : Type \n name args = body
    ///               , -- no annotation: name args = body
    ///               ]
    /// ```
    pub(super) fn value_decl(
        &mut self,
        maybe_docs: Option<&'a Comment<'a>>,
        start: Position,
    ) -> Result<(Decl<'a>, Position), error::Decl<'a>> {
        let name = self.lower_name(error::Decl::Start)?;
        let end = self.get_position();

        self.specialize(
            |bump, e, row, col| error::Decl::Def(name, bump.alloc(e), row, col),
            |p| {
                p.chomp_and_check_indent(DeclDef::Space, DeclDef::IndentEquals)?;

                p.one_of(
                    DeclDef::Equals,
                    vec![
                        // Type annotation: name : Type \n name args = body
                        Box::new(|p: &mut Parser<'a>| {
                            p.word1(b':', DeclDef::Equals)?;
                            p.chomp_and_check_indent(DeclDef::Space, DeclDef::IndentType)?;

                            let (type_ann, _) = p.specialize(
                                |bump, e, row, col| DeclDef::Type(bump.alloc(e), row, col),
                                |p| p.type_expr(),
                            )?;

                            // Must be on a fresh line for the definition
                            p.check_fresh_line(DeclDef::NameRepeat)?;

                            let def_name = p.chomp_matching_name_decl(name)?;
                            p.chomp_and_check_indent(DeclDef::Space, DeclDef::IndentEquals)?;
                            p.chomp_value_args_and_body(maybe_docs, start, def_name, Some(type_ann))
                        }),
                        // No type annotation: name args = body
                        Box::new(|p: &mut Parser<'a>| {
                            let name_located =
                                p.alloc(Located::at(alder_region::Region::new(start, end), name));
                            p.chomp_value_args_and_body(maybe_docs, start, name_located, None)
                        }),
                    ],
                )
            },
        )
    }

    /// Parse function arguments and body for a value definition.
    ///
    /// Mirrors Elm's `chompDefArgsAndBody`.
    fn chomp_value_args_and_body(
        &mut self,
        maybe_docs: Option<&'a Comment<'a>>,
        start: Position,
        name: &'a Located<&'a str>,
        type_ann: Option<&'a Located<alder_source::Type<'a>>>,
    ) -> Result<(Decl<'a>, Position), DeclDef<'a>> {
        let mut args: BumpVec<'a, &'a Located<alder_source::Pattern<'a>>> =
            BumpVec::new_in(self.bump);

        loop {
            let args_for_body = args.clone();

            let result = self.one_of(
                DeclDef::Equals,
                vec![
                    // Parse an argument pattern
                    Box::new(|p: &mut Parser<'a>| {
                        let arg = p.specialize(
                            |bump, e, row, col| DeclDef::Arg(bump.alloc(e), row, col),
                            |p| p.pattern_term(),
                        )?;
                        p.chomp_and_check_indent(DeclDef::Space, DeclDef::IndentEquals)?;
                        args.push(arg);
                        Ok(ValueDeclState::MoreArgs)
                    }),
                    // Parse the body
                    Box::new(|p: &mut Parser<'a>| {
                        p.word1(b'=', DeclDef::Equals)?;
                        p.chomp_and_check_indent(DeclDef::Space, DeclDef::IndentBody)?;
                        let (body, end) = p.specialize(
                            |bump, e, row, col| DeclDef::Body(bump.alloc(e), row, col),
                            |p| p.expression(),
                        )?;

                        let args_slice = args_for_body.into_bump_slice();
                        let value = Value {
                            name,
                            arguments: args_slice,
                            body,
                            annotation: type_ann,
                        };
                        let located_value = p.add_end(start, value);
                        Ok(ValueDeclState::Done(
                            Decl::Value(maybe_docs, located_value),
                            end,
                        ))
                    }),
                ],
            )?;

            match result {
                ValueDeclState::MoreArgs => continue,
                ValueDeclState::Done(decl, end) => return Ok((decl, end)),
            }
        }
    }

    /// Check that the name matches the expected name (for type-annotated definitions).
    fn chomp_matching_name_decl(
        &mut self,
        expected: &str,
    ) -> Result<&'a Located<&'a str>, DeclDef<'a>> {
        let start = self.get_position();
        let name = self.lower_name(DeclDef::NameRepeat)?;
        let name_located = self.add_end(start, name);

        if name == expected {
            Ok(name_located)
        } else {
            let (row, col) = self.position();
            Err(DeclDef::NameMatch(name, row, col))
        }
    }
}

/// State for parsing value definition arguments.
enum ValueDeclState<'a> {
    MoreArgs,
    Done(Decl<'a>, Position),
}

#[cfg(test)]
mod tests {
    use super::super::assert_decl_snapshot;

    #[test]
    fn value_simple() {
        assert_decl_snapshot!("foo = 1");
    }

    #[test]
    fn value_with_args() {
        assert_decl_snapshot!("add x y = x");
    }

    #[test]
    fn value_with_type_annotation() {
        assert_decl_snapshot!(
            r#"
            add : Int -> Int -> Int
            add x y = x
        "#
        );
    }

    #[test]
    fn value_multiline_body() {
        assert_decl_snapshot!(
            r#"
            greet name =
                name
        "#
        );
    }

    #[test]
    fn value_with_doc_comment() {
        assert_decl_snapshot!(
            r#"
            {-| A simple greeting function -}
            greet name = name
        "#
        );
    }

    #[test]
    fn value_with_doc_comment_multiline() {
        assert_decl_snapshot!(
            r#"
            {-| Adds two numbers together.

            This is a longer description.
            -}
            add : Int -> Int -> Int
            add x y = x
        "#
        );
    }
}
