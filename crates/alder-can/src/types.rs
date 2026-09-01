use std::collections::{BTreeMap, BTreeSet};

use alder_ast::{
    AliasArgument as CanAliasArgument, AliasType as CanAliasType, Annotation,
    FieldType as CanFieldType, FreeVars, QualifiedName, Type as CanType,
};
use alder_region::{Located, Region};
use alder_source::Type as SourceType;
use bumpalo::Bump;

use crate::Error;
use crate::accumulate;
use crate::environment::{self, Env, Info};
use crate::error::BadArityContext;

/// Canonicalize a source type and wrap it in an Annotation with free type variables.
/// Mirrors Elm's `Type.toAnnotation`.
pub fn to_annotation<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    source_type: &'a Located<SourceType<'a>>,
) -> Result<&'a Annotation<'a>, Vec<Error<'a>>> {
    let typ = canonicalize_type(bump, env, source_type)?;
    let mut free_var_set: BTreeSet<&'a str> = BTreeSet::new();
    collect_free_vars(&typ.value, &mut free_var_set);
    let free_vars: FreeVars<'a> = bump.alloc_slice_fill_iter(free_var_set);
    Ok(bump.alloc(Annotation { free_vars, typ }))
}

/// Canonicalize a source type using the environment.
/// Mirrors Elm's `Type.canonicalize`.
pub fn canonicalize_type<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    typ: &'a Located<SourceType<'a>>,
) -> Result<&'a Located<CanType<'a>>, Vec<Error<'a>>> {
    Ok(bump.alloc(Located::at(
        typ.region,
        canonicalize_type_value(bump, env, typ.region, &typ.value)?,
    )))
}

/// Canonicalize a slice of type arguments.
pub fn canonicalize_type_arguments<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    args: &'a [&'a Located<SourceType<'a>>],
) -> Result<&'a [&'a Located<CanType<'a>>], Vec<Error<'a>>> {
    accumulate::try_all_alloc_ref(
        bump,
        args.iter()
            .copied()
            .map(|arg| canonicalize_type(bump, env, arg)),
    )
}

fn canonicalize_type_value<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    region: Region,
    typ: &SourceType<'a>,
) -> Result<CanType<'a>, Vec<Error<'a>>> {
    Ok(match typ {
        SourceType::Lambda { from, to } => {
            let (from, to) = accumulate::accumulate2(
                canonicalize_type(bump, env, from),
                canonicalize_type(bump, env, to),
            )?;
            CanType::Lambda { from, to }
        }
        SourceType::Var(name) => CanType::Var(name),
        SourceType::Type {
            region: name_region,
            name,
            args,
        } => {
            let info = find_type(bump, env, *name_region, name)?;
            canonicalize_env_type(bump, env, region, name, args, info)?
        }
        SourceType::TypeQual {
            region: name_region,
            module: type_module,
            name,
            args,
        } => {
            let info = find_type_qual(bump, env, *name_region, type_module, name)?;
            canonicalize_env_type(bump, env, region, name, args, info)?
        }
        SourceType::Record { fields, ext } => {
            let field_dict = check_fields(fields)?;
            let can_fields = accumulate::try_all_alloc(
                bump,
                field_dict
                    .into_iter()
                    .map(|(_, (index, field))| canonicalize_field_type(bump, env, index, field)),
            )?;
            CanType::Record {
                fields: can_fields,
                ext: ext.map(|name| name.value),
            }
        }
        SourceType::Unit => CanType::Unit,
        SourceType::Tuple {
            first,
            second,
            rest,
        } => {
            if rest.len() > 1 {
                return Err(vec![Error::TupleLargerThanThree { region }]);
            }
            let (first, second, rest) = accumulate::accumulate3(
                canonicalize_type(bump, env, first),
                canonicalize_type(bump, env, second),
                canonicalize_type_arguments(bump, env, rest),
            )?;
            CanType::Tuple {
                first,
                second,
                rest,
            }
        }
    })
}

/// Mirrors Elm's `Dups.checkFields`: one `DuplicateField` per duplicated
/// name, in name order, with the first two occurrences. On success the
/// fields come back keyed (and therefore ordered) by name, each carrying
/// its source-position index.
fn check_fields<'a, 'f>(
    fields: &'f [&'a alder_source::FieldType<'a>],
) -> Result<BTreeMap<&'a str, (u16, &'f alder_source::FieldType<'a>)>, Vec<Error<'a>>> {
    let mut occurrences: BTreeMap<&'a str, Vec<(Region, u16, &alder_source::FieldType<'a>)>> =
        BTreeMap::new();
    for (index, field) in fields.iter().enumerate() {
        let index: u16 = index.try_into().expect("record field index exceeds u16");
        occurrences
            .entry(field.field.value)
            .or_default()
            .push((field.field.region, index, field));
    }

    let mut result = BTreeMap::new();
    let mut errors = Vec::new();
    for (name, mut entries) in occurrences {
        if entries.len() > 1 {
            errors.push(Error::DuplicateField {
                name,
                first: entries[0].0,
                second: entries[1].0,
            });
        } else {
            let (_, index, field) = entries.remove(0);
            result.insert(name, (index, field));
        }
    }

    if errors.is_empty() {
        Ok(result)
    } else {
        Err(errors)
    }
}

/// Mirrors Elm's `Env.findType`. Lookup errors point at the type name
/// itself, not the whole application.
fn find_type<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    name_region: Region,
    name: &'a str,
) -> Result<environment::Type<'a>, Vec<Error<'a>>> {
    match env.types.get(name) {
        Some(Info::Specific(_, typ)) => Ok(*typ),
        Some(Info::Ambiguous(first, others)) => Err(vec![Error::AmbiguousType {
            region: name_region,
            prefix: None,
            name,
            first_module: *first,
            other_modules: bump.alloc_slice_fill_iter(others.iter().copied()),
        }]),
        None => Err(vec![Error::NotFoundType {
            region: name_region,
            prefix: None,
            name,
            suggestions: env.possible_type_names(bump),
        }]),
    }
}

/// Mirrors Elm's `Env.findTypeQual`.
fn find_type_qual<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    name_region: Region,
    prefix: &'a str,
    name: &'a str,
) -> Result<environment::Type<'a>, Vec<Error<'a>>> {
    let info = env
        .q_types
        .get(prefix)
        .and_then(|m| m.get(name))
        .ok_or_else(|| {
            vec![Error::NotFoundType {
                region: name_region,
                prefix: Some(prefix),
                name,
                suggestions: env.possible_type_names(bump),
            }]
        })?;

    match info {
        Info::Specific(_, typ) => Ok(*typ),
        Info::Ambiguous(first, others) => Err(vec![Error::AmbiguousType {
            region: name_region,
            prefix: Some(prefix),
            name,
            first_module: *first,
            other_modules: bump.alloc_slice_fill_iter(others.iter().copied()),
        }]),
    }
}

/// Mirrors Elm's `Type.canonicalizeType`: arguments are canonicalized
/// first, and the arity check only runs once they all succeed.
fn canonicalize_env_type<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    region: Region,
    name: &'a str,
    args: &'a [&'a Located<SourceType<'a>>],
    typ: environment::Type<'a>,
) -> Result<CanType<'a>, Vec<Error<'a>>> {
    let can_args = canonicalize_type_arguments(bump, env, args)?;
    match typ {
        environment::Type::Alias {
            arity,
            home,
            parameters,
            typ: alias_typ,
        } => {
            check_arity(region, name, arity, args.len())?;
            let arguments = bump.alloc_slice_fill_iter(
                parameters
                    .iter()
                    .copied()
                    .zip(can_args.iter().copied())
                    .map(|(parameter, typ)| CanAliasArgument {
                        name: parameter,
                        typ,
                    }),
            );
            Ok(CanType::Alias {
                reference: QualifiedName { home, name },
                arguments,
                target: CanAliasType::Open(alias_typ),
            })
        }
        environment::Type::Union { arity, home } => {
            check_arity(region, name, arity, args.len())?;
            Ok(CanType::Named {
                reference: QualifiedName { home, name },
                args: can_args,
            })
        }
    }
}

fn check_arity<'a>(
    region: Region,
    name: &'a str,
    expected: usize,
    actual: usize,
) -> Result<(), Vec<Error<'a>>> {
    if expected == actual {
        Ok(())
    } else {
        Err(vec![Error::BadArity {
            region,
            context: BadArityContext::TypeArity,
            name,
            expected,
            actual,
        }])
    }
}

fn canonicalize_field_type<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    index: u16,
    field: &alder_source::FieldType<'a>,
) -> Result<CanFieldType<'a>, Vec<Error<'a>>> {
    Ok(CanFieldType {
        index,
        field: field.field.value,
        typ: canonicalize_type(bump, env, field.typ)?,
    })
}

pub fn collect_free_vars<'a>(typ: &CanType<'a>, vars: &mut BTreeSet<&'a str>) {
    match typ {
        CanType::Var(name) => {
            vars.insert(name);
        }
        CanType::Lambda { from, to } => {
            collect_free_vars(&from.value, vars);
            collect_free_vars(&to.value, vars);
        }
        CanType::Named { args, .. } => {
            for arg in *args {
                collect_free_vars(&arg.value, vars);
            }
        }
        CanType::Record { fields, ext } => {
            if let Some(name) = ext {
                vars.insert(name);
            }
            for f in *fields {
                collect_free_vars(&f.typ.value, vars);
            }
        }
        CanType::Alias { arguments, .. } => {
            for arg in *arguments {
                collect_free_vars(&arg.typ.value, vars);
            }
        }
        CanType::Unit => {}
        CanType::Tuple {
            first,
            second,
            rest,
        } => {
            collect_free_vars(&first.value, vars);
            collect_free_vars(&second.value, vars);
            for r in *rest {
                collect_free_vars(&r.value, vars);
            }
        }
    }
}

/// Mirrors Elm's `Type.dealias`: fill a `Holey` alias body by substituting
/// the alias arguments for its type variables. `Filled` bodies are already
/// substituted.
pub fn dealias<'a>(
    bump: &'a Bump,
    arguments: &'a [CanAliasArgument<'a>],
    target: &CanAliasType<'a>,
) -> &'a Located<CanType<'a>> {
    match target {
        CanAliasType::Filled(typ) => typ,
        CanAliasType::Open(typ) => {
            let table: BTreeMap<&'a str, &'a Located<CanType<'a>>> =
                arguments.iter().map(|arg| (arg.name, arg.typ)).collect();
            dealias_help(bump, &table, typ)
        }
    }
}

fn dealias_help<'a>(
    bump: &'a Bump,
    table: &BTreeMap<&'a str, &'a Located<CanType<'a>>>,
    typ: &'a Located<CanType<'a>>,
) -> &'a Located<CanType<'a>> {
    let substituted = match &typ.value {
        CanType::Var(name) => return table.get(name).copied().unwrap_or(typ),
        CanType::Unit => return typ,
        CanType::Lambda { from, to } => CanType::Lambda {
            from: dealias_help(bump, table, from),
            to: dealias_help(bump, table, to),
        },
        CanType::Named { reference, args } => CanType::Named {
            reference: *reference,
            args: bump.alloc_slice_fill_iter(args.iter().map(|arg| dealias_help(bump, table, arg))),
        },
        // NOTE: like Elm's `dealiasHelp`, the record extension variable is
        // not substituted.
        CanType::Record { fields, ext } => CanType::Record {
            fields: bump.alloc_slice_fill_iter(fields.iter().map(|f| CanFieldType {
                index: f.index,
                field: f.field,
                typ: dealias_help(bump, table, f.typ),
            })),
            ext: *ext,
        },
        // Like Elm, only the alias arguments are substituted; the target
        // body is left alone (it closes over the argument names).
        CanType::Alias {
            reference,
            arguments,
            target,
        } => CanType::Alias {
            reference: *reference,
            arguments: bump.alloc_slice_fill_iter(arguments.iter().map(|arg| CanAliasArgument {
                name: arg.name,
                typ: dealias_help(bump, table, arg.typ),
            })),
            target: match target {
                CanAliasType::Open(t) => CanAliasType::Open(t),
                CanAliasType::Filled(t) => CanAliasType::Filled(t),
            },
        },
        CanType::Tuple {
            first,
            second,
            rest,
        } => CanType::Tuple {
            first: dealias_help(bump, table, first),
            second: dealias_help(bump, table, second),
            rest: bump.alloc_slice_fill_iter(rest.iter().map(|r| dealias_help(bump, table, r))),
        },
    };
    bump.alloc(Located::at(typ.region, substituted))
}

/// Mirrors Elm's `Type.iteratedDealias`.
pub fn iterated_dealias<'a>(
    bump: &'a Bump,
    typ: &'a Located<CanType<'a>>,
) -> &'a Located<CanType<'a>> {
    match &typ.value {
        CanType::Alias {
            arguments, target, ..
        } => iterated_dealias(bump, dealias(bump, arguments, target)),
        _ => typ,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alder_ast::ModuleName;
    use bumpalo::Bump;

    use crate::environment::{Env, Info, Type as EnvType};

    fn empty_env<'a>(bump: &'a Bump) -> Env<'a> {
        let _ = bump;
        Env {
            home: ModuleName {
                package: None,
                name: "Main",
            },
            vars: Default::default(),
            types: Default::default(),
            ctors: Default::default(),
            binops: Default::default(),
            q_vars: Default::default(),
            q_types: Default::default(),
            q_ctors: Default::default(),
        }
    }

    fn env_with_int<'a>(bump: &'a Bump) -> Env<'a> {
        let home = ModuleName {
            package: None,
            name: "Basics",
        };
        let mut env = empty_env(bump);
        env.types.insert(
            "Int",
            Info::Specific(home, EnvType::Union { arity: 0, home }),
        );
        env
    }

    fn env_with_list_and_int<'a>(bump: &'a Bump) -> Env<'a> {
        let basics = ModuleName {
            package: None,
            name: "Basics",
        };
        let list_mod = ModuleName {
            package: None,
            name: "List",
        };
        let mut env = empty_env(bump);
        env.types.insert(
            "Int",
            Info::Specific(
                basics,
                EnvType::Union {
                    arity: 0,
                    home: basics,
                },
            ),
        );
        env.types.insert(
            "List",
            Info::Specific(
                list_mod,
                EnvType::Union {
                    arity: 1,
                    home: list_mod,
                },
            ),
        );
        env
    }

    fn env_with_maybe_alias<'a>(bump: &'a Bump) -> Env<'a> {
        let home = ModuleName {
            package: None,
            name: "Maybe",
        };
        let mut env = empty_env(bump);
        let alias_type = bump.alloc(Located::at(Region::zero(), CanType::Var("a")));
        env.types.insert(
            "Maybe",
            Info::Specific(
                home,
                EnvType::Alias {
                    arity: 1,
                    home,
                    parameters: bump.alloc_slice_fill_iter(["a"]),
                    typ: alias_type,
                },
            ),
        );
        env
    }

    fn parse_type<'a>(bump: &'a Bump, input: &str) -> &'a Located<SourceType<'a>> {
        let src = bump.alloc_str(input);
        let mut parser = alder_parse::Parser::new(bump, src.as_bytes());
        let (typ, _end) = parser.type_expr().expect("expected successful parse");
        typ
    }

    macro_rules! assert_type_snapshot {
        ($input:expr, $env_fn:ident) => {{
            let bump = Bump::new();
            let env = $env_fn(&bump);
            let typ = parse_type(&bump, $input);
            let result = canonicalize_type(&bump, &env, typ);
            insta::with_settings!({
                description => $input,
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(result.unwrap());
            });
        }};
    }

    macro_rules! assert_type_error_snapshot {
        ($input:expr, $env_fn:ident) => {{
            let bump = Bump::new();
            let env = $env_fn(&bump);
            let typ = parse_type(&bump, $input);
            let result = canonicalize_type(&bump, &env, typ);
            insta::with_settings!({
                description => $input,
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(result.unwrap_err());
            });
        }};
    }

    macro_rules! assert_annotation_snapshot {
        ($input:expr, $env_fn:ident) => {{
            let bump = Bump::new();
            let env = $env_fn(&bump);
            let typ = parse_type(&bump, $input);
            let result = to_annotation(&bump, &env, typ);
            insta::with_settings!({
                description => $input,
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(result.unwrap());
            });
        }};
    }

    #[test]
    fn annotation_simple_var() {
        assert_annotation_snapshot!("a", empty_env);
    }

    #[test]
    fn annotation_function() {
        assert_annotation_snapshot!("a -> b -> a", empty_env);
    }

    #[test]
    fn annotation_no_free_vars() {
        assert_annotation_snapshot!("Int", env_with_int);
    }

    #[test]
    fn annotation_mixed() {
        assert_annotation_snapshot!("a -> List a", env_with_list_and_int);
    }

    #[test]
    fn annotation_record_ext() {
        assert_annotation_snapshot!("{ a | x : Int }", env_with_int);
    }

    #[test]
    fn type_tuple_three() {
        assert_type_snapshot!("( a, b, c )", empty_env);
    }

    #[test]
    fn type_tuple_four_errors() {
        assert_type_error_snapshot!("( a, b, c, d )", empty_env);
    }

    #[test]
    fn type_alias_expansion() {
        assert_type_snapshot!("Maybe a", env_with_maybe_alias);
    }

    #[test]
    fn type_union_reference() {
        assert_type_snapshot!("List a", env_with_list_and_int);
    }

    #[test]
    fn record_fields_sorted_by_name() {
        assert_type_snapshot!("{ z : Int, a : Int }", env_with_int);
    }

    #[test]
    fn bad_args_reported_before_arity() {
        assert_type_error_snapshot!("Maybe Bogus Other", env_with_maybe_alias);
    }

    #[test]
    fn iterated_dealias_substitutes_arguments() {
        let bump = Bump::new();
        let home = ModuleName {
            package: None,
            name: "Main",
        };
        // type alias Transform a = a -> a, applied to Int
        let var_a = bump.alloc(Located::at(Region::zero(), CanType::Var("a")));
        let body = bump.alloc(Located::at(
            Region::zero(),
            CanType::Lambda {
                from: var_a,
                to: var_a,
            },
        ));
        let int = bump.alloc(Located::at(
            Region::zero(),
            CanType::Named {
                reference: QualifiedName { home, name: "Int" },
                args: &[],
            },
        ));
        let arguments = bump.alloc_slice_fill_iter([CanAliasArgument {
            name: "a",
            typ: &*int,
        }]);
        let aliased = bump.alloc(Located::at(
            Region::zero(),
            CanType::Alias {
                reference: QualifiedName {
                    home,
                    name: "Transform",
                },
                arguments,
                target: CanAliasType::Open(body),
            },
        ));
        let dealiased = iterated_dealias(&bump, aliased);
        match &dealiased.value {
            CanType::Lambda { from, to } => {
                assert!(matches!(
                    &from.value,
                    CanType::Named { reference, .. } if reference.name == "Int"
                ));
                assert!(matches!(
                    &to.value,
                    CanType::Named { reference, .. } if reference.name == "Int"
                ));
            }
            other => panic!("expected substituted lambda, got {other:?}"),
        }
    }
}
