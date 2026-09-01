use std::collections::BTreeMap;

use alder_ast::{ConstructorName, Pattern as CanPattern, PatternCtor, PatternCtorArg};
use alder_region::{Located, Region};
use alder_source::Pattern as SourcePattern;
use bumpalo::Bump;

use crate::Error;
use crate::environment::{self, Env, dups};
use crate::error::{BadArityContext, DuplicatePatternContext};

pub type Bindings<'a> = BTreeMap<&'a str, Region>;

/// Canonicalize a pattern, detect duplicate bindings, return (pattern, bindings).
/// Mirrors Elm's `Pattern.verify` wrapped around a single pattern.
pub fn verify<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    context: DuplicatePatternContext<'a>,
    pattern: &'a Located<SourcePattern<'a>>,
) -> Result<(&'a Located<CanPattern<'a>>, Bindings<'a>), Vec<Error<'a>>> {
    let (patterns, bindings) = verify_all(bump, env, context, std::slice::from_ref(&pattern))?;
    Ok((patterns[0], bindings))
}

/// Canonicalize several patterns inside ONE duplicate-detection scope,
/// like Elm's `Pattern.verify ctx (traverse (Pattern.canonicalize env) args)`.
/// This is what catches `\x x -> ...` and `f x x = ...` across arguments.
pub fn verify_all<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    context: DuplicatePatternContext<'a>,
    patterns: &[&'a Located<SourcePattern<'a>>],
) -> Result<(Vec<&'a Located<CanPattern<'a>>>, Bindings<'a>), Vec<Error<'a>>> {
    let mut bound: Vec<(&'a str, Region)> = Vec::new();
    let mut results = Vec::with_capacity(patterns.len());
    let mut errors = Vec::new();
    for pattern in patterns {
        match canonicalize(bump, env, pattern, &mut bound) {
            Ok(p) => results.push(p),
            Err(errs) => errors.extend(errs),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let bindings = detect_duplicates(context, bound)?;
    Ok((results, bindings))
}

/// Run Elm's `Dups.detect (Error.DuplicatePattern context)` over collected
/// bindings. Exposed so typed definitions can share one scope between
/// `gather_typed_args` and the check.
pub fn detect_duplicates<'a>(
    context: DuplicatePatternContext<'a>,
    bound: Vec<(&'a str, Region)>,
) -> Result<Bindings<'a>, Vec<Error<'a>>> {
    dups::detect(bound, |name, first, second| Error::DuplicatePattern {
        context,
        name,
        first,
        second,
    })
}

pub fn canonicalize<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    pattern: &'a Located<SourcePattern<'a>>,
    bindings: &mut Vec<(&'a str, Region)>,
) -> Result<&'a Located<CanPattern<'a>>, Vec<Error<'a>>> {
    let can = match &pattern.value {
        SourcePattern::Anything => CanPattern::Anything,

        SourcePattern::Var(name) => {
            bindings.push((name, pattern.region));
            CanPattern::Var(name)
        }

        SourcePattern::Record(fields) => {
            for field in *fields {
                bindings.push((field.value, field.region));
            }
            let names = bump.alloc_slice_fill_iter(fields.iter().map(|f| f.value));
            CanPattern::Record(names)
        }

        SourcePattern::Alias {
            pattern: inner,
            name,
        } => {
            let can_inner = canonicalize(bump, env, inner, bindings)?;
            bindings.push((name.value, name.region));
            CanPattern::Alias {
                pattern: can_inner,
                name: name.value,
            }
        }

        SourcePattern::Unit => CanPattern::Unit,

        SourcePattern::Tuple {
            first,
            second,
            rest,
        } => {
            // Like Elm's `PTuple <$> a <*> b <*> canonicalizeTuple`, the
            // element errors and the tuple-size error accumulate together.
            let size_check: Result<(), Vec<Error<'a>>> = if rest.len() > 1 {
                Err(vec![Error::TupleLargerThanThree {
                    region: pattern.region,
                }])
            } else {
                Ok(())
            };
            let (first, second, rest, ()) = crate::accumulate::accumulate4(
                canonicalize(bump, env, first, bindings),
                canonicalize(bump, env, second, bindings),
                canonicalize_list(bump, env, rest, bindings),
                size_check,
            )?;
            CanPattern::Tuple {
                first,
                second,
                rest,
            }
        }

        SourcePattern::Ctor {
            region: name_region,
            name,
            args,
        } => {
            let ctor = env.find_ctor(bump, *name_region, name)?;
            canonicalize_ctor_pattern(bump, env, pattern.region, name, args, &ctor, bindings)?
        }

        SourcePattern::CtorQual {
            region: name_region,
            module,
            name,
            args,
        } => {
            let ctor = env.find_ctor_qual(bump, *name_region, module, name)?;
            canonicalize_ctor_pattern(bump, env, pattern.region, name, args, &ctor, bindings)?
        }

        SourcePattern::List(pats) => {
            let can_pats = canonicalize_list(bump, env, pats, bindings)?;
            CanPattern::List(can_pats)
        }

        SourcePattern::Cons { head, tail } => {
            let (head, tail) = crate::accumulate::accumulate2(
                canonicalize(bump, env, head, bindings),
                canonicalize(bump, env, tail, bindings),
            )?;
            CanPattern::Cons { head, tail }
        }

        SourcePattern::Str(s) => CanPattern::Str(s),
        SourcePattern::Int(n) => CanPattern::Int(*n),
    };

    Ok(bump.alloc(Located::at(pattern.region, can)))
}

fn canonicalize_ctor_pattern<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    region: Region,
    name: &'a str,
    args: &'a [&'a Located<SourcePattern<'a>>],
    ctor: &environment::Ctor<'a>,
    bindings: &mut Vec<(&'a str, Region)>,
) -> Result<CanPattern<'a>, Vec<Error<'a>>> {
    match ctor {
        environment::Ctor::Union {
            home,
            type_name,
            type_vars: _,
            union: union_def,
            index,
            arity,
            arguments: expected_types,
            options,
            alternatives,
        } => {
            if args.len() != *arity as usize {
                return Err(vec![Error::BadArity {
                    region,
                    context: BadArityContext::PatternArity,
                    name,
                    expected: *arity as usize,
                    actual: args.len(),
                }]);
            }

            let mut ctor_args = Vec::with_capacity(args.len());
            let mut errors = Vec::new();
            for (i, (pat, expected_typ)) in args
                .iter()
                .copied()
                .zip(expected_types.iter().copied())
                .enumerate()
            {
                match canonicalize(bump, env, pat, bindings) {
                    Ok(can_pat) => ctor_args.push(PatternCtorArg {
                        index: i as u16,
                        typ: expected_typ,
                        pattern: can_pat,
                    }),
                    Err(mut e) => errors.append(&mut e),
                }
            }
            if !errors.is_empty() {
                return Err(errors);
            }

            Ok(CanPattern::Constructor(PatternCtor {
                reference: ConstructorName {
                    home: *home,
                    union: type_name,
                    name,
                },
                union: union_def,
                index: *index,
                arguments: bump.alloc_slice_fill_iter(ctor_args),
                options: *options,
                alternatives: *alternatives,
            }))
        }

        // `True`/`False` are nullary; like Elm, the arity check runs before
        // the Bool decision, so `True x` is a `BadArity` error.
        environment::Ctor::Bool { union, .. } => {
            if !args.is_empty() {
                return Err(vec![Error::BadArity {
                    region,
                    context: BadArityContext::PatternArity,
                    name,
                    expected: 0,
                    actual: args.len(),
                }]);
            }
            Ok(CanPattern::Bool {
                union,
                value: name == "True",
            })
        }

        environment::Ctor::RecordCtor { .. } => {
            Err(vec![Error::PatternHasRecordCtor { region, name }])
        }
    }
}

fn canonicalize_list<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    patterns: &'a [&'a Located<SourcePattern<'a>>],
    bindings: &mut Vec<(&'a str, Region)>,
) -> Result<&'a [&'a Located<CanPattern<'a>>], Vec<Error<'a>>> {
    let mut results = Vec::with_capacity(patterns.len());
    let mut errors = Vec::new();
    for pat in patterns {
        match canonicalize(bump, env, pat, bindings) {
            Ok(p) => results.push(p),
            Err(mut e) => errors.append(&mut e),
        }
    }
    if errors.is_empty() {
        Ok(bump.alloc_slice_fill_iter(results))
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alder_ast::{CtorOpts, ModuleName, Type as CanType, Union};
    use bumpalo::Bump;

    use crate::environment::{Ctor, Env, Info};

    fn empty_env<'a>(_bump: &'a Bump) -> Env<'a> {
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

    fn env_with_maybe<'a>(bump: &'a Bump) -> Env<'a> {
        let home = ModuleName {
            package: None,
            name: "Maybe",
        };
        let mut env = empty_env(bump);

        let nothing_ctor = bump.alloc(alder_ast::Ctor {
            name: "Nothing",
            index: 1,
            arity: 0,
            arguments: &[],
        });
        let just_arg_typ = bump.alloc(Located::at(Region::zero(), CanType::Var("a")));
        let just_ctor = bump.alloc(alder_ast::Ctor {
            name: "Just",
            index: 0,
            arity: 1,
            arguments: bump.alloc_slice_fill_iter([&*just_arg_typ]),
        });
        let maybe_union: &Union = bump.alloc(Union {
            name: bump.alloc(Located::at(Region::zero(), "Maybe")),
            parameters: bump.alloc_slice_fill_iter(["a"]),
            ctors: bump.alloc_slice_fill_iter([&*nothing_ctor, &*just_ctor]),
            alternatives: 2,
            options: CtorOpts::Normal,
        });

        // Nothing: arity 0
        let nothing = Ctor::Union {
            home,
            type_name: "Maybe",
            type_vars: &["a"],
            union: maybe_union,
            index: 1,
            arity: 0,
            arguments: &[],
            options: CtorOpts::Normal,
            alternatives: 2,
        };
        env.ctors.insert("Nothing", Info::Specific(home, nothing));

        // Just: arity 1
        let just = Ctor::Union {
            home,
            type_name: "Maybe",
            type_vars: &["a"],
            union: maybe_union,
            index: 0,
            arity: 1,
            arguments: bump.alloc_slice_fill_iter([&*just_arg_typ]),
            options: CtorOpts::Normal,
            alternatives: 2,
        };
        env.ctors.insert("Just", Info::Specific(home, just));

        env
    }

    fn env_with_record_ctor<'a>(bump: &'a Bump) -> Env<'a> {
        let home = ModuleName {
            package: None,
            name: "Main",
        };
        let mut env = empty_env(bump);

        let field_typ: &Located<CanType> =
            bump.alloc(Located::at(Region::zero(), CanType::Var("a")));
        let record_type: &Located<CanType> = bump.alloc(Located::at(
            Region::zero(),
            CanType::Record {
                fields: bump.alloc_slice_fill_iter([
                    alder_ast::FieldType {
                        index: 0,
                        field: "x",
                        typ: field_typ,
                    },
                    alder_ast::FieldType {
                        index: 1,
                        field: "y",
                        typ: field_typ,
                    },
                ]),
                ext: None,
            },
        ));
        let ctor = match &record_type.value {
            CanType::Record { fields, .. } => {
                crate::environment::make_record_ctor(bump, home, "Point", &[], record_type, fields)
            }
            _ => unreachable!(),
        };
        env.ctors.insert("Point", Info::Specific(home, ctor));

        env
    }

    fn env_with_bool<'a>(bump: &'a Bump) -> Env<'a> {
        let home = ModuleName {
            package: None,
            name: "Basics",
        };
        let mut env = empty_env(bump);

        let true_ctor = bump.alloc(alder_ast::Ctor {
            name: "True",
            index: 0,
            arity: 0,
            arguments: &[],
        });
        let false_ctor = bump.alloc(alder_ast::Ctor {
            name: "False",
            index: 1,
            arity: 0,
            arguments: &[],
        });
        let bool_union: &Union = bump.alloc(Union {
            name: bump.alloc(Located::at(Region::zero(), "Bool")),
            parameters: &[],
            ctors: bump.alloc_slice_fill_iter([&*true_ctor, &*false_ctor]),
            alternatives: 2,
            options: CtorOpts::Enum,
        });

        env.ctors.insert(
            "True",
            Info::Specific(
                home,
                Ctor::Bool {
                    home,
                    union: bool_union,
                    index: 0,
                },
            ),
        );
        env.ctors.insert(
            "False",
            Info::Specific(
                home,
                Ctor::Bool {
                    home,
                    union: bool_union,
                    index: 1,
                },
            ),
        );

        env
    }

    fn parse_pattern<'a>(bump: &'a Bump, input: &str) -> &'a Located<SourcePattern<'a>> {
        let src = bump.alloc_str(input);
        let mut parser = alder_parse::Parser::new(bump, src.as_bytes());
        let (pat, _end) = parser.pattern_expr().expect("expected successful parse");
        pat
    }

    macro_rules! assert_pattern_snapshot {
        ($input:expr, $env_fn:ident) => {{
            let bump = Bump::new();
            let env = $env_fn(&bump);
            let pat = parse_pattern(&bump, $input);
            let result = verify(&bump, &env, DuplicatePatternContext::CaseBranch, pat);
            insta::with_settings!({
                description => $input,
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(result.unwrap());
            });
        }};
    }

    macro_rules! assert_pattern_error_snapshot {
        ($input:expr, $env_fn:ident) => {{
            let bump = Bump::new();
            let env = $env_fn(&bump);
            let pat = parse_pattern(&bump, $input);
            let result = verify(&bump, &env, DuplicatePatternContext::CaseBranch, pat);
            insta::with_settings!({
                description => $input,
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(result.unwrap_err());
            });
        }};
    }

    // === Success tests ===

    #[test]
    fn wildcard() {
        assert_pattern_snapshot!("_", empty_env);
    }

    #[test]
    fn variable() {
        assert_pattern_snapshot!("x", empty_env);
    }

    #[test]
    fn record_pattern() {
        assert_pattern_snapshot!("{ x, y }", empty_env);
    }

    #[test]
    fn unit() {
        assert_pattern_snapshot!("()", empty_env);
    }

    #[test]
    fn tuple_two() {
        assert_pattern_snapshot!("( a, b )", empty_env);
    }

    #[test]
    fn tuple_three() {
        assert_pattern_snapshot!("( a, b, c )", empty_env);
    }

    #[test]
    fn literal_int() {
        assert_pattern_snapshot!("42", empty_env);
    }

    #[test]
    fn literal_str() {
        assert_pattern_snapshot!(r#""hello""#, empty_env);
    }

    #[test]
    fn list_pattern() {
        assert_pattern_snapshot!("[ a, b ]", empty_env);
    }

    #[test]
    fn cons_pattern() {
        assert_pattern_snapshot!("x :: xs", empty_env);
    }

    #[test]
    fn ctor_no_args() {
        assert_pattern_snapshot!("Nothing", env_with_maybe);
    }

    #[test]
    fn ctor_with_args() {
        assert_pattern_snapshot!("Just x", env_with_maybe);
    }

    #[test]
    fn bool_true_pattern() {
        assert_pattern_snapshot!("True", env_with_bool);
    }

    #[test]
    fn bool_false_pattern() {
        assert_pattern_snapshot!("False", env_with_bool);
    }

    #[test]
    fn alias_pattern() {
        assert_pattern_snapshot!("(x, y) as pair", empty_env);
    }

    // === Error tests ===

    #[test]
    fn tuple_four() {
        assert_pattern_error_snapshot!("( a, b, c, d )", empty_env);
    }

    #[test]
    fn ctor_wrong_arity() {
        assert_pattern_error_snapshot!("Just x y", env_with_maybe);
    }

    #[test]
    fn ctor_not_found() {
        assert_pattern_error_snapshot!("Foo", empty_env);
    }

    #[test]
    fn record_ctor_in_pattern() {
        assert_pattern_error_snapshot!("Point", env_with_record_ctor);
    }

    #[test]
    fn duplicate_vars() {
        assert_pattern_error_snapshot!("( x, x )", empty_env);
    }

    #[test]
    fn bool_pattern_with_args_is_bad_arity() {
        assert_pattern_error_snapshot!("True x", env_with_bool);
    }

    #[test]
    fn duplicate_across_sibling_patterns() {
        let bump = Bump::new();
        let env = empty_env(&bump);
        let first = parse_pattern(&bump, "x");
        let second = parse_pattern(&bump, "x");
        let result = verify_all(
            &bump,
            &env,
            DuplicatePatternContext::LambdaArgs,
            &[first, second],
        );
        insta::with_settings!({
            description => "verify_all over `x` and `x`",
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result.unwrap_err());
        });
    }
}
