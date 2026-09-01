//! Type alias parsing for Alder.
//!
//! Ported from Elm's `Parse/Declaration.hs` (chompAliasNameToEquals).

use alder_region::{Located, Position};
use alder_source::Alias;
use bumpalo::collections::Vec as BumpVec;

use crate::Parser;
use crate::error::TypeAlias;

impl<'a> Parser<'a> {
    /// Parse the body of a type alias declaration (after "type alias").
    ///
    /// Parses: Name a b = Type
    ///
    /// Mirrors Elm's type alias parsing in `typeDecl`.
    pub(super) fn type_alias_body(
        &mut self,
        start: Position,
    ) -> Result<(&'a Located<Alias<'a>>, Position), TypeAlias<'a>> {
        self.chomp_and_check_indent(TypeAlias::Space, TypeAlias::IndentEquals)?;
        let (name, args) = self.chomp_alias_name_to_equals()?;

        let (typ, end) = self.specialize(
            |bump, e, row, col| TypeAlias::Body(bump.alloc(e), row, col),
            |p| p.type_expr(),
        )?;

        let alias = Alias {
            name,
            arguments: args,
            typ,
        };
        let located_alias = self.add_end(start, alias);

        Ok((located_alias, end))
    }

    /// Parse alias name and type parameters until `=`.
    ///
    /// Mirrors Elm's `chompAliasNameToEquals`.
    fn chomp_alias_name_to_equals(
        &mut self,
    ) -> Result<(&'a Located<&'a str>, &'a [&'a Located<&'a str>]), TypeAlias<'a>> {
        let name_start = self.get_position();
        let name_str = self.upper_name(TypeAlias::Name)?;
        let name = self.add_end(name_start, name_str);

        self.chomp_and_check_indent(TypeAlias::Space, TypeAlias::IndentEquals)?;
        self.chomp_alias_name_to_equals_help(name)
    }

    /// Helper to collect type parameters for a type alias.
    ///
    /// Mirrors Elm's `chompAliasNameToEqualsHelp`.
    fn chomp_alias_name_to_equals_help(
        &mut self,
        name: &'a Located<&'a str>,
    ) -> Result<(&'a Located<&'a str>, &'a [&'a Located<&'a str>]), TypeAlias<'a>> {
        let mut args: BumpVec<'a, &'a Located<&'a str>> = BumpVec::new_in(self.bump);

        loop {
            let args_clone = args.clone();

            let result = self.one_of(
                TypeAlias::Equals,
                vec![
                    // Parse a type parameter
                    Box::new(|p: &mut Parser<'a>| {
                        let arg_start = p.get_position();
                        let arg_str = p.lower_name(TypeAlias::Equals)?;
                        let arg = p.add_end(arg_start, arg_str);
                        p.chomp_and_check_indent(TypeAlias::Space, TypeAlias::IndentEquals)?;
                        args.push(arg);
                        Ok(AliasNameState::MoreArgs)
                    }),
                    // Found equals - done with params
                    Box::new(|p: &mut Parser<'a>| {
                        p.word1(b'=', TypeAlias::Equals)?;
                        p.chomp_and_check_indent(TypeAlias::Space, TypeAlias::IndentBody)?;
                        Ok(AliasNameState::Done(args_clone.into_bump_slice()))
                    }),
                ],
            )?;

            match result {
                AliasNameState::MoreArgs => continue,
                AliasNameState::Done(args_slice) => return Ok((name, args_slice)),
            }
        }
    }
}

/// State for parsing type alias name and parameters.
enum AliasNameState<'a> {
    MoreArgs,
    Done(&'a [&'a Located<&'a str>]),
}

#[cfg(test)]
mod tests {
    use super::super::assert_decl_snapshot;

    #[test]
    fn type_alias_simple() {
        assert_decl_snapshot!("type alias Name = String");
    }

    #[test]
    fn type_alias_with_params() {
        assert_decl_snapshot!("type alias Result e a = Result e a");
    }

    #[test]
    fn type_alias_record() {
        assert_decl_snapshot!("type alias Model = { count : Int }");
    }

    #[test]
    fn type_alias_with_doc_comment() {
        assert_decl_snapshot!(
            r#"
            {-| The application model -}
            type alias Model = { count : Int }
        "#
        );
    }
}
