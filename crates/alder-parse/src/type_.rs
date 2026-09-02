//! Type expressions.
//!
//! See docs/parser-internals.md §5.15.
//!
//! Grammar (SPEC.md "Types", with §10.14's HKT application):
//!
//! ```ebnf
//! type        = fn_type | type_term ;
//! fn_type     = 'fn' '(' [ type { ',' type } [ ',' ] ] ')' '->' type ;
//! type_term   = path [ type_args ]
//!             | lower_ident [ type_args ]
//!             | '(' ')' | '(' type ')' | '(' type ',' type { ',' type } [ ',' ] ')'
//!             | '{' [ lower_ident '|' ] [ field_type { ',' field_type } [ ',' ] ] '}'
//!             | '[' [ tag_variant { '|' tag_variant } ] [ '|' lower_ident ] ']' ;
//! type_args   = '[' type { ',' type } [ ',' ] ']' ;        (* adjacent to the name *)
//! field_type  = lower_ident [ '?' ] ':' type ;
//! tag_variant = tag [ '(' type { ',' type } [ ',' ] ')' ] ; (* '(' adjacent to the tag *)
//! ```
//!
//! Conventions: `type_expr` chomps trailing whitespace; `type_term`,
//! `type_args`, `tag_variant` and `field_types` stop right after their last
//! byte (the closing bracket, or the name when there is none). A
//! parenthesized type `(T)` is returned as `T` itself, so its region
//! excludes the parentheses (as in Elm).
// OWNER: type_.rs (Wave 1)

use alder_region::{Located, Position, Region};
use alder_source::{FieldType, Name, TagVariant, Type};
use bumpalo::collections::Vec as BumpVec;

use crate::Parser;
use crate::error::{self, TArgs, TErrorRow, TFn, TRecord, TTuple};
use crate::keyword::Keyword;

impl<'a> Parser<'a> {
    /// `fn` type or term. Chomps trailing whitespace.
    pub fn type_expr(&mut self) -> Result<&'a Located<Type<'a>>, error::Type<'a>> {
        let start = self.get_position();
        let typ = if self.peek_keyword(b"fn") {
            self.specialize(
                |bump, e, row, col| error::Type::Fn(bump.alloc(e), row, col),
                |p| p.type_fn(start),
            )?
        } else {
            self.type_term()?
        };
        self.chomp();
        Ok(typ)
    }

    /// path[args] | var[args] | ( ) | tuple | record | error row.
    ///
    /// Dispatches on the first byte and reports `Start` itself. Does not chomp.
    pub(crate) fn type_term(&mut self) -> Result<&'a Located<Type<'a>>, error::Type<'a>> {
        let start = self.get_position();
        match self.peek() {
            Some(b) if b.is_ascii_uppercase() => {
                let path = self.path(error::Type::Start, error::Type::PathMember)?;
                let args = self.type_args_opt()?;
                Ok(self.add_end(start, Type::Named { path, args }))
            }
            Some(b) if b.is_ascii_lowercase() => {
                if let Some(kw) = Keyword::from_word(self.peek_word()) {
                    let (row, col) = self.position();
                    return Err(error::Type::Reserved(kw, row, col));
                }
                let name = self.lower_name(error::Type::Start)?;
                let args = self.type_args_opt()?;
                Ok(self.add_end(start, Type::Var { name, args }))
            }
            Some(b'(') => self.specialize(
                |bump, e, row, col| error::Type::Tuple(bump.alloc(e), row, col),
                |p| p.type_tuple(start),
            ),
            Some(b'{') => self.specialize(
                |bump, e, row, col| error::Type::Record(bump.alloc(e), row, col),
                |p| {
                    p.advance(); // `{`
                    let (fields, ext) = p.field_types()?;
                    Ok(p.add_end(start, Type::Record { fields, ext }))
                },
            ),
            Some(b'[') => self.specialize(
                |bump, e, row, col| error::Type::ErrorRow(bump.alloc(e), row, col),
                |p| p.type_error_row(start),
            ),
            _ => {
                let (row, col) = self.position();
                Err(error::Type::Start(row, col))
            }
        }
    }

    /// Type arguments when a `[` immediately follows the name; empty otherwise.
    fn type_args_opt(&mut self) -> Result<&'a [&'a Located<Type<'a>>], error::Type<'a>> {
        if self.peek() == Some(b'[') {
            self.specialize(
                |bump, e, row, col| error::Type::Args(bump.alloc(e), row, col),
                |p| p.type_args(),
            )
        } else {
            Ok(&[])
        }
    }

    /// At `[`: `[T, U]`. Consumes through the `]`; does not chomp.
    pub(crate) fn type_args(&mut self) -> Result<&'a [&'a Located<Type<'a>>], TArgs<'a>> {
        self.advance(); // `[`
        self.chomp();
        if self.peek() == Some(b']') {
            let (row, col) = self.position();
            return Err(TArgs::Empty(row, col));
        }
        let mut args = BumpVec::new_in(self.bump);
        loop {
            let arg = self.specialize(
                |bump, e, row, col| TArgs::Type(bump.alloc(e), row, col),
                |p| p.type_expr(),
            )?;
            args.push(arg);
            match self.peek() {
                Some(b',') => {
                    self.advance();
                    self.chomp();
                    if self.peek() == Some(b']') {
                        self.advance();
                        break;
                    }
                }
                Some(b']') => {
                    self.advance();
                    break;
                }
                _ => {
                    let (row, col) = self.position();
                    return Err(TArgs::End(row, col));
                }
            }
        }
        Ok(args.into_bump_slice())
    }

    /// `:tag[(T, …)]` — shared by error rows and `error` groups.
    ///
    /// The `(` must be adjacent to the tag. Stops after the tag or its `)`;
    /// does not chomp.
    pub(crate) fn tag_variant(&mut self) -> Result<TagVariant<'a>, error::TagVariant<'a>> {
        let name = self.tag_name(error::TagVariant::Name, error::TagVariant::Name)?;
        let mut args = BumpVec::new_in(self.bump);
        if self.peek() == Some(b'(') {
            self.advance();
            self.chomp();
            loop {
                let arg = self.specialize(
                    |bump, e, row, col| error::TagVariant::Arg(bump.alloc(e), row, col),
                    |p| p.type_expr(),
                )?;
                args.push(arg);
                match self.peek() {
                    Some(b',') => {
                        self.advance();
                        self.chomp();
                        if self.peek() == Some(b')') {
                            self.advance();
                            break;
                        }
                    }
                    Some(b')') => {
                        self.advance();
                        break;
                    }
                    _ => {
                        let (row, col) = self.position();
                        return Err(error::TagVariant::ArgEnd(row, col));
                    }
                }
            }
        }
        Ok(TagVariant {
            name,
            args: args.into_bump_slice(),
        })
    }

    /// After `{`: fields with `?` and `r |` extension. Shared with enum record variants.
    ///
    /// Consumes through the `}`; does not chomp afterwards.
    pub(crate) fn field_types(
        &mut self,
    ) -> Result<(&'a [FieldType<'a>], Option<Name<'a>>), TRecord<'a>> {
        self.chomp();
        let mut fields = BumpVec::new_in(self.bump);
        if self.peek() == Some(b'}') {
            self.advance();
            return Ok((fields.into_bump_slice(), None));
        }

        // The first name is either the extension variable (`r |`) or a field.
        let name = self.located_lower(TRecord::Field)?;
        self.chomp();
        let ext = if self.peek() == Some(b'|') {
            self.advance();
            self.chomp();
            if self.peek() == Some(b'}') {
                let (row, col) = self.position();
                return Err(TRecord::ExtField(row, col));
            }
            let first = self.located_lower(TRecord::Field)?;
            fields.push(self.field_type_after_name(first)?);
            Some(name)
        } else {
            fields.push(self.field_type_after_name(name)?);
            None
        };

        loop {
            match self.peek() {
                Some(b',') => {
                    self.advance();
                    self.chomp();
                    if self.peek() == Some(b'}') {
                        self.advance();
                        break;
                    }
                    let name = self.located_lower(TRecord::Field)?;
                    fields.push(self.field_type_after_name(name)?);
                }
                Some(b'}') => {
                    self.advance();
                    break;
                }
                _ => {
                    let (row, col) = self.position();
                    return Err(TRecord::End(row, col));
                }
            }
        }
        Ok((fields.into_bump_slice(), ext))
    }

    /// The rest of a field after its name: `[?] ':' type`.
    fn field_type_after_name(&mut self, field: Name<'a>) -> Result<FieldType<'a>, TRecord<'a>> {
        self.chomp();
        let optional = if self.peek() == Some(b'?') {
            let start = self.get_position();
            self.advance();
            let region = Region::new(start, self.get_position());
            self.chomp();
            Some(region)
        } else {
            None
        };
        self.word1(b':', TRecord::Colon)?;
        self.chomp();
        let typ = self.specialize(
            |bump, e, row, col| TRecord::Type(bump.alloc(e), row, col),
            |p| p.type_expr(),
        )?;
        Ok(FieldType {
            field,
            optional,
            typ,
        })
    }

    /// At `fn`: `fn(A, B) -> C`. The return type is a full `type_expr`, so
    /// `fn(a) -> fn(b) -> c` nests to the right.
    fn type_fn(&mut self, start: Position) -> Result<&'a Located<Type<'a>>, TFn<'a>> {
        self.advance_by(2); // `fn` (peeked by the caller)
        self.chomp();
        self.word1(b'(', TFn::Open)?;
        self.chomp();
        let mut params = BumpVec::new_in(self.bump);
        if self.peek() == Some(b')') {
            self.advance();
        } else {
            loop {
                let param = self.specialize(
                    |bump, e, row, col| TFn::Param(bump.alloc(e), row, col),
                    |p| p.type_expr(),
                )?;
                params.push(param);
                match self.peek() {
                    Some(b',') => {
                        self.advance();
                        self.chomp();
                        if self.peek() == Some(b')') {
                            self.advance();
                            break;
                        }
                    }
                    Some(b')') => {
                        self.advance();
                        break;
                    }
                    _ => {
                        let (row, col) = self.position();
                        return Err(TFn::ParamEnd(row, col));
                    }
                }
            }
        }
        self.chomp();
        self.word2(b'-', b'>', TFn::Arrow)?;
        self.chomp();
        let ret = self.specialize(
            |bump, e, row, col| TFn::Ret(bump.alloc(e), row, col),
            |p| p.type_expr(),
        )?;
        // `type_expr` chomped after the return type; its region end is ours.
        let region = Region::new(start, ret.region.end);
        Ok(self.alloc(Located::at(
            region,
            Type::Fn {
                params: params.into_bump_slice(),
                ret,
            },
        )))
    }

    /// At `(`: `()`, `(T)` (returned as `T`), or `(T, U, …)`.
    fn type_tuple(&mut self, start: Position) -> Result<&'a Located<Type<'a>>, TTuple<'a>> {
        self.advance(); // `(`
        self.chomp();
        if self.peek() == Some(b')') {
            self.advance();
            return Ok(self.add_end(start, Type::Unit));
        }
        let first = self.tuple_entry()?;
        let mut rest = BumpVec::new_in(self.bump);
        loop {
            match self.peek() {
                Some(b',') => {
                    self.advance();
                    self.chomp();
                    if self.peek() == Some(b')') {
                        self.advance();
                        break;
                    }
                    rest.push(self.tuple_entry()?);
                }
                Some(b')') => {
                    self.advance();
                    break;
                }
                _ => {
                    let (row, col) = self.position();
                    return Err(TTuple::End(row, col));
                }
            }
        }
        match rest.into_bump_slice().split_first() {
            None => Ok(first),
            Some((second, rest)) => Ok(self.add_end(
                start,
                Type::Tuple {
                    first,
                    second,
                    rest,
                },
            )),
        }
    }

    fn tuple_entry(&mut self) -> Result<&'a Located<Type<'a>>, TTuple<'a>> {
        self.specialize(
            |bump, e, row, col| TTuple::Type(bump.alloc(e), row, col),
            |p| p.type_expr(),
        )
    }

    /// At `[`: `[:tag(T) | :tag | r]`. Also `[]` (closed, empty) and `[r]`
    /// (a bare row variable).
    fn type_error_row(&mut self, start: Position) -> Result<&'a Located<Type<'a>>, TErrorRow<'a>> {
        self.advance(); // `[`
        self.chomp();
        let mut tags = BumpVec::new_in(self.bump);
        let mut ext = None;
        if self.peek() == Some(b']') {
            self.advance();
        } else {
            let mut after_bar = false;
            loop {
                if self.peek_lower() {
                    ext = Some(self.located_lower(TErrorRow::Ext)?);
                    self.chomp();
                    self.word1(b']', TErrorRow::End)?;
                    break;
                }
                if after_bar && self.peek() != Some(b':') {
                    let (row, col) = self.position();
                    return Err(TErrorRow::Ext(row, col));
                }
                let tag = self.specialize(
                    |bump, e, row, col| TErrorRow::Tag(bump.alloc(e), row, col),
                    |p| p.tag_variant(),
                )?;
                tags.push(tag);
                self.chomp();
                match self.peek() {
                    Some(b'|') => {
                        self.advance();
                        self.chomp();
                        after_bar = true;
                    }
                    Some(b']') => {
                        self.advance();
                        break;
                    }
                    _ => {
                        let (row, col) = self.position();
                        return Err(TErrorRow::End(row, col));
                    }
                }
            }
        }
        Ok(self.add_end(
            start,
            Type::ErrorRow {
                tags: tags.into_bump_slice(),
                ext,
            },
        ))
    }
}

/// Snapshot test macro for successful type parsing.
#[cfg(test)]
macro_rules! assert_type_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let result = parser
            .type_expr()
            .unwrap_or_else(|e| panic!("expected Ok, got Err: {e:#?}\n\nSource:\n{code}"));
        assert!(
            parser.is_eof(),
            "unconsumed input at {:?}\n\nSource:\n{code}",
            parser.position()
        );
        insta::with_settings!({
            description => code,
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }};
}

/// Snapshot test macro for type parse errors.
#[cfg(test)]
macro_rules! assert_type_error_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let err = parser
            .type_expr()
            .err()
            .unwrap_or_else(|| panic!("expected Err, got Ok\n\nSource:\n{code}"));
        insta::with_settings!({
            description => code,
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(err);
        });
    }};
}

// The re-exports exist for submodules (docs/parser-internals.md §7.1);
// `type_.rs` has none and its own tests reach the pair through textual
// scope, so the imports are unused until something imports them.
#[cfg(test)]
#[allow(unused)]
pub(crate) use assert_type_error_snapshot;
#[cfg(test)]
#[allow(unused)]
pub(crate) use assert_type_snapshot;

#[cfg(test)]
mod tests {
    // ---- variables and names

    #[test]
    fn var() {
        assert_type_snapshot!("a");
    }

    #[test]
    fn var_applied() {
        assert_type_snapshot!("f[a]");
    }

    #[test]
    fn var_applied_nested() {
        assert_type_snapshot!("t[f[a]]");
    }

    #[test]
    fn named_simple() {
        assert_type_snapshot!("User");
    }

    #[test]
    fn named_qualified() {
        assert_type_snapshot!("Option::Foo");
    }

    // ---- application

    #[test]
    fn app_one_arg() {
        assert_type_snapshot!("Array[User]");
    }

    #[test]
    fn app_many_args() {
        assert_type_snapshot!("Result[User, AuthError]");
    }

    #[test]
    fn app_nested() {
        assert_type_snapshot!("Map[String, Array[User]]");
    }

    #[test]
    fn app_result_shorthand() {
        assert_type_snapshot!("Result[User]");
    }

    #[test]
    fn app_trailing_comma() {
        assert_type_snapshot!("Map[String, Number,]");
    }

    // ---- functions

    #[test]
    fn fn_no_params() {
        assert_type_snapshot!("fn() -> Number");
    }

    #[test]
    fn fn_one_param() {
        assert_type_snapshot!("fn(a) -> b");
    }

    #[test]
    fn fn_many_params() {
        assert_type_snapshot!("fn(String, Number) -> Bool");
    }

    #[test]
    fn fn_returning_fn() {
        assert_type_snapshot!("fn(a) -> fn(b) -> c");
    }

    #[test]
    fn fn_hkt() {
        assert_type_snapshot!("fn(a) -> f[b]");
    }

    #[test]
    fn fn_param_is_fn() {
        assert_type_snapshot!("fn(fn(a) -> b, Array[a]) -> Array[b]");
    }

    // ---- unit and tuples

    #[test]
    fn unit() {
        assert_type_snapshot!("()");
    }

    #[test]
    fn tuple_pair() {
        assert_type_snapshot!("(a, b)");
    }

    #[test]
    fn tuple_triple() {
        assert_type_snapshot!("(String, Number, Bool)");
    }

    #[test]
    fn parenthesized() {
        assert_type_snapshot!("(Array[a])");
    }

    // ---- records

    #[test]
    fn record_empty() {
        assert_type_snapshot!("{}");
    }

    #[test]
    fn record_fields() {
        assert_type_snapshot!("{ id: Id, name: String }");
    }

    #[test]
    fn record_optional_field() {
        assert_type_snapshot!("{ nickname?: String }");
    }

    #[test]
    fn record_extension() {
        assert_type_snapshot!("{ r | name: String }");
    }

    #[test]
    fn record_trailing_comma() {
        assert_type_snapshot!("{ id: Id, }");
    }

    #[test]
    fn record_multiline() {
        assert_type_snapshot!(
            r#"
            {
                id: Id,
                name: String,
                nickname?: String,
            }
            "#
        );
    }

    // ---- error rows

    #[test]
    fn error_row_empty() {
        assert_type_snapshot!("[]");
    }

    #[test]
    fn error_row_single() {
        assert_type_snapshot!("[:timeout]");
    }

    #[test]
    fn error_row_args() {
        assert_type_snapshot!("[:invalid(String, Number)]");
    }

    #[test]
    fn error_row_open() {
        assert_type_snapshot!("[:not_found(Id) | :timeout | r]");
    }

    #[test]
    fn error_row_var_only() {
        assert_type_snapshot!("[r]");
    }

    #[test]
    fn error_row_in_result() {
        assert_type_snapshot!("Result[User, [:timeout | r]]");
    }

    // ---- errors

    #[test]
    fn error_app_unclosed() {
        assert_type_error_snapshot!("Array[User");
    }

    #[test]
    fn error_app_empty() {
        assert_type_error_snapshot!("Array[]");
    }

    #[test]
    fn error_app_bad_arg() {
        assert_type_error_snapshot!("Array[1]");
    }

    #[test]
    fn error_path_dangling() {
        assert_type_error_snapshot!("Option::");
    }

    #[test]
    fn error_fn_missing_arrow() {
        assert_type_error_snapshot!("fn(a) b");
    }

    #[test]
    fn error_fn_missing_parens() {
        assert_type_error_snapshot!("fn -> a");
    }

    #[test]
    fn error_fn_param_end() {
        assert_type_error_snapshot!("fn(a b) -> c");
    }

    #[test]
    fn error_fn_missing_ret() {
        assert_type_error_snapshot!("fn(a) ->");
    }

    #[test]
    fn error_tuple_unclosed() {
        assert_type_error_snapshot!("(a, b");
    }

    #[test]
    fn error_record_missing_colon() {
        assert_type_error_snapshot!("{ name String }");
    }

    #[test]
    fn error_record_ext_no_fields() {
        assert_type_error_snapshot!("{ r | }");
    }

    #[test]
    fn error_record_field() {
        assert_type_error_snapshot!("{ Name: String }");
    }

    #[test]
    fn error_record_unclosed() {
        assert_type_error_snapshot!("{ name: String");
    }

    #[test]
    fn error_row_bad_tag() {
        assert_type_error_snapshot!("[:1]");
    }

    #[test]
    fn error_row_bad_ext() {
        assert_type_error_snapshot!("[:timeout | 1]");
    }

    #[test]
    fn error_row_unclosed() {
        assert_type_error_snapshot!("[:timeout");
    }

    #[test]
    fn error_row_tag_args_unclosed() {
        assert_type_error_snapshot!("[:not_found(Id]");
    }

    #[test]
    fn error_start() {
        assert_type_error_snapshot!("42");
    }

    #[test]
    fn error_reserved() {
        assert_type_error_snapshot!("match");
    }
}
