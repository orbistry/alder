//! Custom type (union) parsing for Alder.
//!
//! Ported from Elm's `Parse/Declaration.hs` (chompCustomNameToEquals, chompVariants)
//! and `Parse/Type.hs` (variant).

use alder_region::{Located, Position};
use alder_source::{Ctor, Union};
use bumpalo::collections::Vec as BumpVec;

use crate::Parser;
use crate::error::CustomType;

impl<'a> Parser<'a> {
    /// Parse the body of a custom type declaration (after "type").
    ///
    /// Parses: Name a b = Ctor1 Type | Ctor2 Type
    ///
    /// Mirrors Elm's custom type parsing in `typeDecl`.
    pub(super) fn union_body(
        &mut self,
        start: Position,
    ) -> Result<(&'a Located<Union<'a>>, Position), CustomType<'a>> {
        let (name, args) = self.chomp_custom_name_to_equals()?;

        // Parse first variant
        let (first_variant, first_end) = self.variant()?;

        // Parse remaining variants
        let (variants, end) = self.chomp_variants(vec![first_variant], first_end)?;

        let ctors = self.alloc_slice_copy(&variants);
        let union = Union {
            name,
            arguments: args,
            ctors,
        };
        let located_union = self.add_end(start, union);

        Ok((located_union, end))
    }

    /// Parse custom type name and type parameters until `=`.
    ///
    /// Mirrors Elm's `chompCustomNameToEquals`.
    fn chomp_custom_name_to_equals(
        &mut self,
    ) -> Result<(&'a Located<&'a str>, &'a [&'a Located<&'a str>]), CustomType<'a>> {
        let name_start = self.get_position();
        let name_str = self.upper_name(CustomType::Name)?;
        let name = self.add_end(name_start, name_str);

        self.chomp_and_check_indent(CustomType::Space, CustomType::IndentEquals)?;
        self.chomp_custom_name_to_equals_help(name)
    }

    /// Helper to collect type parameters for a custom type.
    ///
    /// Mirrors Elm's `chompCustomNameToEqualsHelp`.
    fn chomp_custom_name_to_equals_help(
        &mut self,
        name: &'a Located<&'a str>,
    ) -> Result<(&'a Located<&'a str>, &'a [&'a Located<&'a str>]), CustomType<'a>> {
        let mut args: BumpVec<'a, &'a Located<&'a str>> = BumpVec::new_in(self.bump);

        loop {
            let args_clone = args.clone();

            let result = self.one_of(
                CustomType::Equals,
                vec![
                    // Parse a type parameter
                    Box::new(|p: &mut Parser<'a>| {
                        let arg_start = p.get_position();
                        let arg_str = p.lower_name(CustomType::Equals)?;
                        let arg = p.add_end(arg_start, arg_str);
                        p.chomp_and_check_indent(CustomType::Space, CustomType::IndentEquals)?;
                        args.push(arg);
                        Ok(CustomNameState::MoreArgs)
                    }),
                    // Found equals - done with params
                    Box::new(|p: &mut Parser<'a>| {
                        p.word1(b'=', CustomType::Equals)?;
                        p.chomp_and_check_indent(CustomType::Space, CustomType::IndentAfterEquals)?;
                        Ok(CustomNameState::Done(args_clone.into_bump_slice()))
                    }),
                ],
            )?;

            match result {
                CustomNameState::MoreArgs => continue,
                CustomNameState::Done(args_slice) => return Ok((name, args_slice)),
            }
        }
    }

    /// Parse a single variant: CtorName Type1 Type2 ...
    ///
    /// Mirrors Elm's `Type.variant`.
    fn variant(&mut self) -> Result<(&'a Ctor<'a>, Position), CustomType<'a>> {
        let name_start = self.get_position();
        let name_str = self.upper_name(CustomType::Variant)?;
        let name = self.add_end(name_start, name_str);
        let name_end = self.get_position();

        self.chomp(CustomType::Space)?;

        // Parse constructor arguments (type terms)
        let (args, end) = self.specialize(
            |bump, e, row, col| CustomType::VariantArg(bump.alloc(e), row, col),
            |p| p.chomp_variant_args(name_end),
        )?;

        let ctor = self.alloc(Ctor {
            name,
            arguments: args,
        });
        Ok((ctor, end))
    }

    /// Parse variant constructor arguments.
    fn chomp_variant_args(
        &mut self,
        mut end: Position,
    ) -> Result<(&'a [&'a Located<alder_source::Type<'a>>], Position), crate::error::Type<'a>> {
        let mut args: BumpVec<'a, &'a Located<alder_source::Type<'a>>> = BumpVec::new_in(self.bump);

        loop {
            let args_clone = args.clone();

            let result: Result<VariantArgState, crate::error::Type<'a>> = self
                .one_of_with_fallback(
                    vec![Box::new(|p: &mut Parser<'a>| {
                        p.check_indent(end.line, end.column, crate::error::Type::IndentStart)?;
                        let arg = p.type_term()?;
                        let new_end = p.get_position();
                        p.chomp(crate::error::Type::Space)?;
                        args.push(arg);
                        Ok(VariantArgState::MoreArgs(new_end))
                    })],
                    VariantArgState::Done(args_clone.into_bump_slice(), end),
                );

            match result? {
                VariantArgState::MoreArgs(new_end) => {
                    end = new_end;
                    continue;
                }
                VariantArgState::Done(args_slice, final_end) => return Ok((args_slice, final_end)),
            }
        }
    }

    /// Parse remaining variants after the first one.
    ///
    /// Mirrors Elm's `chompVariants`.
    fn chomp_variants(
        &mut self,
        mut variants: Vec<&'a Ctor<'a>>,
        end: Position,
    ) -> Result<(Vec<&'a Ctor<'a>>, Position), CustomType<'a>> {
        let variants_for_fallback = variants.clone();

        self.one_of_with_fallback(
            vec![Box::new(|p: &mut Parser<'a>| {
                p.check_indent(end.line, end.column, CustomType::IndentBar)?;
                p.word1(b'|', CustomType::Bar)?;
                p.chomp_and_check_indent(CustomType::Space, CustomType::IndentAfterBar)?;

                let (variant, new_end) = p.variant()?;
                variants.push(variant);

                p.chomp_variants(variants, new_end)
            })],
            (variants_for_fallback, end),
        )
    }
}

/// State for parsing custom type name and parameters.
enum CustomNameState<'a> {
    MoreArgs,
    Done(&'a [&'a Located<&'a str>]),
}

/// State for parsing variant arguments.
enum VariantArgState<'a> {
    MoreArgs(Position),
    Done(&'a [&'a Located<alder_source::Type<'a>>], Position),
}

#[cfg(test)]
mod tests {
    use super::super::assert_decl_snapshot;

    #[test]
    fn union_simple() {
        assert_decl_snapshot!("type Bool = True | False");
    }

    #[test]
    fn union_with_params() {
        assert_decl_snapshot!("type Maybe a = Just a | Nothing");
    }

    #[test]
    fn union_multiple_args() {
        assert_decl_snapshot!("type Result e a = Ok a | Err e");
    }

    #[test]
    fn union_multiline() {
        assert_decl_snapshot!(
            r#"
            type Msg
                = Increment
                | Decrement
                | Reset
        "#
        );
    }

    #[test]
    fn union_with_doc_comment() {
        assert_decl_snapshot!(
            r#"
            {-| Represents optional values -}
            type Maybe a = Just a | Nothing
        "#
        );
    }
}
