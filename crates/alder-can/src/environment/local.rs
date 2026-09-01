use alder_ast::{Alias as CanAlias, Type as CanType, Union as CanUnion};
use alder_region::Located;
use alder_source::{Alias as SourceAlias, Infix, Union as SourceUnion, Value as SourceValue};
use bumpalo::Bump;

use super::{Ctor, Env, Type, Var, dups};
use crate::Error;

pub fn add_union_types<'a>(
    env: &mut Env<'a>,
    unions: &'a [&'a Located<SourceUnion<'a>>],
    aliases: &'a [&'a Located<SourceAlias<'a>>],
) -> Result<(), Vec<Error<'a>>> {
    let items = aliases
        .iter()
        .map(|a| (a.value.name.value, a.value.name.region))
        .chain(
            unions
                .iter()
                .map(|u| (u.value.name.value, u.value.name.region)),
        );
    dups::detect(items, |name, first, second| Error::DuplicateType {
        name,
        first,
        second,
    })?;

    for union in unions {
        let typ = Type::Union {
            arity: union.value.arguments.len(),
            home: env.home,
        };
        env.insert_local_type(union.value.name.value, typ);
    }
    Ok(())
}

/// Add an alias's type entry to the env. The record constructor (if any)
/// is added later by `add_ctors`, matching Elm's `addTypes`/`addCtors` split.
pub fn add_alias_type<'a>(env: &mut Env<'a>, can_alias: &CanAlias<'a>) {
    let typ = Type::Alias {
        arity: can_alias.parameters.len(),
        home: env.home,
        parameters: can_alias.parameters,
        typ: can_alias.typ,
    };
    env.insert_local_type(can_alias.name.value, typ);
}

/// Mirrors Elm's `addCtors`: detect duplicate constructors (union ctors
/// first, then record-alias ctors), each at its own region, then add them
/// to the env. `source_unions` supplies the constructor name regions that
/// the canonical AST does not keep.
pub fn add_ctors<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source_unions: &'a [&'a Located<SourceUnion<'a>>],
    unions: &'a [&'a Located<CanUnion<'a>>],
    aliases: &'a [&'a Located<CanAlias<'a>>],
) -> Result<(), Vec<Error<'a>>> {
    let union_ctors = source_unions
        .iter()
        .flat_map(|u| u.value.ctors.iter().map(|c| (c.name.value, c.name.region)));
    let alias_ctors = aliases
        .iter()
        .filter(|a| matches!(&a.value.typ.value, CanType::Record { ext: None, .. }))
        .map(|a| (a.value.name.value, a.value.name.region));
    dups::detect(union_ctors.chain(alias_ctors), |name, first, second| {
        Error::DuplicateCtor {
            name,
            first,
            second,
        }
    })?;

    for union in unions {
        for ctor in union.value.ctors {
            let info = if env.home.name == "Basics" && union.value.name.value == "Bool" {
                Ctor::Bool {
                    home: env.home,
                    union: &union.value,
                    index: ctor.index,
                }
            } else {
                Ctor::Union {
                    home: env.home,
                    type_name: union.value.name.value,
                    type_vars: union.value.parameters,
                    union: &union.value,
                    index: ctor.index,
                    arity: ctor.arity,
                    arguments: ctor.arguments,
                    options: union.value.options,
                    alternatives: union.value.alternatives,
                }
            };
            env.insert_local_ctor(ctor.name, info);
        }
    }

    for alias in aliases {
        if let CanType::Record { fields, ext: None } = &alias.value.typ.value {
            let info = super::make_record_ctor(
                bump,
                env.home,
                alias.value.name.value,
                alias.value.parameters,
                alias.value.typ,
                fields,
            );
            env.insert_local_ctor(alias.value.name.value, info);
        }
    }

    Ok(())
}

pub fn add_vars<'a>(
    env: &mut Env<'a>,
    values: &'a [&'a Located<SourceValue<'a>>],
) -> Result<(), Vec<Error<'a>>> {
    dups::detect(
        values
            .iter()
            .map(|v| (v.value.name.value, v.value.name.region)),
        |name, first, second| Error::DuplicateDecl {
            name,
            first,
            second,
        },
    )?;

    for value in values {
        env.vars.insert(
            value.value.name.value,
            Var::TopLevel(value.value.name.region),
        );
    }
    Ok(())
}

/// Validate local `infix` declarations.
///
/// Like Elm, local operators do NOT enter the env (only imported binops
/// are in scope; the defining module calls the underlying function).
/// Elm never needed these checks because `infix` is kernel-only there;
/// Alder allows user-defined operators, so duplicates and dangling
/// function references must be real errors instead of silent last-wins
/// and an interface-extraction crash.
pub fn check_binops<'a>(
    env: &Env<'a>,
    binops: &'a [&'a Located<Infix<'a>>],
) -> Result<(), Vec<Error<'a>>> {
    dups::detect(
        binops.iter().map(|b| (b.value.op, b.region)),
        |name, first, second| Error::DuplicateBinop {
            name,
            first,
            second,
        },
    )?;

    let mut errors = Vec::new();
    for binop in binops {
        if !matches!(env.vars.get(binop.value.name), Some(Var::TopLevel(_))) {
            errors.push(Error::BinopFunctionNotFound {
                region: binop.region,
                op: binop.value.op,
                function: binop.value.name,
            });
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
