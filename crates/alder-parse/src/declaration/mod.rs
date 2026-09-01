//! Declaration parsing for Alder.
//!
//! Ported from Elm's `Parse/Declaration.hs`.
//! Handles value definitions, type annotations, type aliases, custom types, and infix declarations.

mod infix;
mod type_alias;
mod union;
mod value;

use alder_region::{Located, Position};
use alder_source::{Alias, Comment, Union, Value};

use crate::Parser;
use crate::error::{self, Decl as DeclErr};

/// A parsed declaration with optional doc comment.
#[derive(Debug)]
pub enum Decl<'a> {
    Value(Option<&'a Comment<'a>>, &'a Located<Value<'a>>),
    Union(Option<&'a Comment<'a>>, &'a Located<Union<'a>>),
    Alias(Option<&'a Comment<'a>>, &'a Located<Alias<'a>>),
}

impl<'a> Parser<'a> {
    /// Parse a single declaration.
    ///
    /// Mirrors Elm's `declaration`:
    /// ```haskell
    /// declaration :: Space.Parser E.Decl Decl
    /// declaration =
    ///   do  maybeDocs <- chompDocComment
    ///       start <- getPosition
    ///       oneOf E.DeclStart
    ///         [ typeDecl maybeDocs start
    ///         , portDecl maybeDocs  -- skipped in Alder
    ///         , valueDecl maybeDocs start
    ///         ]
    /// ```
    pub fn declaration(&mut self) -> Result<(Decl<'a>, Position), error::Decl<'a>> {
        let maybe_docs = self.chomp_doc_comment()?;

        let start = self.get_position();

        self.one_of(
            DeclErr::Start,
            vec![
                // type alias or type (union)
                Box::new(|p: &mut Parser<'a>| p.type_decl(maybe_docs, start)),
                // value definition
                Box::new(|p| p.value_decl(maybe_docs, start)),
            ],
        )
    }

    /// Parse an optional doc comment `{-| ... -}`.
    ///
    /// Mirrors Elm's `chompDocComment`:
    /// ```haskell
    /// chompDocComment =
    ///   oneOfWithFallback
    ///     [ do  docComment <- Space.docComment E.DeclStart E.DeclSpace
    ///           Space.chomp E.DeclSpace
    ///           Space.checkFreshLine E.DeclFreshLineAfterDocComment
    ///           return (Just docComment)
    ///     ]
    ///     Nothing
    /// ```
    fn chomp_doc_comment(&mut self) -> Result<Option<&'a Comment<'a>>, error::Decl<'a>> {
        self.one_of_with_fallback(
            vec![Box::new(|p: &mut Parser<'a>| {
                let doc = p.doc_comment(DeclErr::Start, |space, row, col| {
                    DeclErr::Space(space, row, col)
                })?;
                p.chomp(DeclErr::Space)?;
                p.check_fresh_line(DeclErr::FreshLineAfterDocComment)?;
                Ok(Some(doc))
            })],
            None,
        )
    }

    /// Parse a type declaration (alias or union).
    ///
    /// Mirrors Elm's `typeDecl`:
    /// ```haskell
    /// typeDecl maybeDocs start =
    ///   inContext E.DeclType (Keyword.type_ E.DeclStart) $
    ///     do  Space.chompAndCheckIndent E.DT_Space E.DT_IndentName
    ///         oneOf E.DT_Name
    ///           [ inContext E.DT_Alias (Keyword.alias_ E.DT_Name) $ ...
    ///           , specialize E.DT_Union $ ...
    ///           ]
    /// ```
    fn type_decl(
        &mut self,
        maybe_docs: Option<&'a Comment<'a>>,
        start: Position,
    ) -> Result<(Decl<'a>, Position), error::Decl<'a>> {
        self.in_context(
            |bump, e, row, col| error::Decl::Type(bump.alloc(e), row, col),
            |p| p.keyword_type(error::Decl::Start),
            |p| {
                p.chomp_and_check_indent(error::DeclType::Space, error::DeclType::IndentName)?;

                p.one_of(
                    error::DeclType::Name,
                    vec![
                        // type alias
                        Box::new(|p: &mut Parser<'a>| {
                            p.in_context(
                                |bump, e, row, col| error::DeclType::Alias(bump.alloc(e), row, col),
                                |p| p.keyword_alias(error::DeclType::Name),
                                |p| {
                                    let (alias, end) = p.type_alias_body(start)?;
                                    Ok((Decl::Alias(maybe_docs, alias), end))
                                },
                            )
                        }),
                        // type union (custom type)
                        Box::new(|p| {
                            p.specialize(
                                |bump, e, row, col| error::DeclType::Union(bump.alloc(e), row, col),
                                |p| {
                                    let (union, end) = p.union_body(start)?;
                                    Ok((Decl::Union(maybe_docs, union), end))
                                },
                            )
                        }),
                    ],
                )
            },
        )
    }
}

/// Macro for testing successful declaration parsing.
#[cfg(test)]
macro_rules! assert_decl_snapshot {
    ($src:expr) => {{
        let bump = bumpalo::Bump::new();
        let src = indoc::indoc!($src);
        let src_in_arena = bump.alloc_str(src);
        let mut parser = crate::Parser::new(&bump, src_in_arena.as_bytes());
        match parser.declaration() {
            Ok((decl, _end)) => {
                insta::with_settings!({
                    description => src,
                    omit_expression => true,
                }, {
                    insta::assert_debug_snapshot!(decl);
                });
            }
            Err(e) => panic!("Expected Ok, got Err: {:?}\n\nSource:\n{}", e, src),
        }
    }};
}

#[cfg(test)]
pub(crate) use assert_decl_snapshot;
