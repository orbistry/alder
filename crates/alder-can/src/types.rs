use std::collections::{BTreeMap, BTreeSet};

use alder_ast::{ErrorTagType, RecordTypeField, RowExtension, Type as CanType, TypeSlot};
use alder_region::{Located, Region};
use alder_source::Type as SourceType;
use bumpalo::Bump;

use crate::environment::Env;
use crate::{Error, ErrorKind, TypeError};

pub(crate) fn namespace_prefix<'a>(
    bump: &'a Bump,
    path: alder_source::Path<'a>,
) -> Option<&'a str> {
    (path.segments.len() > 1).then(|| {
        &*bump.alloc_str(
            &path.segments[..path.segments.len() - 1]
                .iter()
                .map(|segment| segment.value)
                .collect::<Vec<_>>()
                .join("::"),
        )
    })
}

/// Dependency-first order for local aliases. Enums deliberately remain nominal
/// boundaries; an alias naming an enum does not expand its payload recursively.
pub(crate) fn alias_order<'a>(
    source: &alder_source::Module<'a>,
) -> Result<Vec<&'a alder_source::TypeAlias<'a>>, Vec<Error<'a>>> {
    let aliases: BTreeMap<_, _> = source
        .items
        .iter()
        .filter_map(|item| match item.value.kind {
            alder_source::ItemKind::TypeAlias(alias) => Some((alias.name.value, alias)),
            _ => None,
        })
        .collect();
    let mut dependencies = BTreeMap::new();
    for (&name, alias) in &aliases {
        let mut pending = vec![alias.typ];
        let mut names = BTreeSet::new();
        while let Some(typ) = pending.pop() {
            match typ.value {
                SourceType::Named { path, args } => {
                    if path.segments.len() == 1 && aliases.contains_key(path.segments[0].value) {
                        names.insert(path.segments[0].value);
                    }
                    pending.extend(args);
                }
                SourceType::Var { args, .. } => pending.extend(args),
                SourceType::Fn { params, ret } => {
                    pending.extend(params);
                    pending.push(ret);
                }
                SourceType::Tuple {
                    first,
                    second,
                    rest,
                } => {
                    pending.extend([first, second]);
                    pending.extend(rest);
                }
                SourceType::Record { fields, .. } => {
                    pending.extend(fields.iter().map(|field| field.typ))
                }
                SourceType::ErrorRow { tags, .. } => {
                    for tag in tags {
                        pending.extend(tag.args);
                    }
                }
                SourceType::Hole | SourceType::Unit => {}
            }
        }
        dependencies.insert(name, names);
    }
    let mut active = BTreeSet::new();
    let mut finished = BTreeSet::new();
    let mut ordered = Vec::new();
    for &root in aliases.keys() {
        let mut pending = vec![(root, false)];
        while let Some((name, exiting)) = pending.pop() {
            if exiting {
                active.remove(name);
                finished.insert(name);
                ordered.push(aliases[name]);
            } else if !finished.contains(name) {
                if !active.insert(name) {
                    return Err(vec![Error::new(
                        aliases[name].name.region,
                        ErrorKind::Type(TypeError::RecursiveAlias { name }),
                    )]);
                }
                pending.push((name, true));
                pending.extend(dependencies[name].iter().rev().map(|&name| (name, false)));
            }
        }
    }
    Ok(ordered)
}

pub fn canonicalize_type<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    variables: &BTreeSet<&'a str>,
    source: &'a Located<SourceType<'a>>,
) -> Result<&'a Located<CanType<'a>>, Vec<Error<'a>>> {
    let typ = match source.value {
        SourceType::Hole => {
            return Err(vec![Error::new(
                source.region,
                ErrorKind::Type(TypeError::InvalidHole),
            )]);
        }
        SourceType::Var { name, args } => {
            if !variables.contains(name) {
                return Err(vec![Error::new(
                    source.region,
                    ErrorKind::Type(TypeError::UnboundVariable { name }),
                )]);
            }
            let args = canonicalize_types(bump, env, variables, args)?;
            CanType::Var { name, args }
        }
        SourceType::Named { path, args } => {
            let last = path.segments.last().expect("source paths are nonempty");
            let qualifier = namespace_prefix(bump, path);
            if qualifier.is_none()
                && args.is_empty()
                && let Some(projection) = env.find_associated_type(last.value)
            {
                return Ok(bump.alloc(Located::at(source.region, CanType::Projection(projection))));
            }
            let binding = env
                .find_type(bump, path.region(), qualifier, last.value)
                .map_err(|error| vec![error])?;
            let args = canonicalize_types(bump, env, variables, args)?;
            let result_shorthand = binding.reference.name == "Result" && args.len() == 1;
            if args.len() != binding.arity && !result_shorthand {
                return Err(vec![Error::new(
                    source.region,
                    ErrorKind::Type(TypeError::BadArity {
                        name: binding.reference.name,
                        expected: binding.arity,
                        actual: args.len(),
                    }),
                )]);
            }
            if let Some(alias) = env.alias_definition(bump, binding.reference) {
                return crate::aliases::instantiate(
                    bump,
                    binding.reference,
                    &alias,
                    args,
                    source.region,
                );
            }
            CanType::Named {
                reference: binding.reference,
                args,
            }
        }
        SourceType::Fn { params, ret } => CanType::Fn {
            params: canonicalize_types(bump, env, variables, params)?,
            ret: canonicalize_type(bump, env, variables, ret)?,
        },
        SourceType::Unit => CanType::Unit,
        SourceType::Tuple {
            first,
            second,
            rest,
        } => {
            let mut items = Vec::with_capacity(2 + rest.len());
            items.push(canonicalize_type(bump, env, variables, first)?);
            items.push(canonicalize_type(bump, env, variables, second)?);
            for item in rest {
                items.push(canonicalize_type(bump, env, variables, item)?);
            }
            CanType::Tuple(bump.alloc_slice_copy(&items))
        }
        SourceType::Record { fields, ext } => {
            let fields = canonicalize_record_fields(bump, env, variables, fields)?;
            CanType::Record {
                fields,
                ext: canonicalize_extension(variables, ext, source.region)?,
            }
        }
        SourceType::ErrorRow { tags, ext } => {
            let mut seen = BTreeMap::new();
            let mut canonical = Vec::with_capacity(tags.len());
            let mut errors = Vec::new();
            for (index, tag) in tags.iter().enumerate() {
                if let Some(first) = seen.insert(tag.name.value, tag.name.region) {
                    errors.push(Error::new(
                        tag.name.region,
                        ErrorKind::Type(TypeError::DuplicateTag {
                            name: tag.name.value,
                            first,
                        }),
                    ));
                }
                match canonicalize_types(bump, env, variables, tag.args) {
                    Ok(args) => canonical.push(ErrorTagType {
                        index: index as u16,
                        name: tag.name.value,
                        args,
                    }),
                    Err(mut type_errors) => errors.append(&mut type_errors),
                }
            }
            if !errors.is_empty() {
                return Err(errors);
            }
            canonical.sort_by_key(|tag| tag.name);
            CanType::ErrorRow {
                tags: bump.alloc_slice_copy(&canonical),
                ext: canonicalize_extension(variables, ext, source.region)?,
            }
        }
    };
    Ok(bump.alloc(Located::at(source.region, typ)))
}

pub fn canonicalize_impl_head_type<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    variables: &BTreeSet<&'a str>,
    source: &'a Located<SourceType<'a>>,
) -> Result<&'a Located<CanType<'a>>, Vec<Error<'a>>> {
    let SourceType::Named { path, args } = source.value else {
        return canonicalize_type(bump, env, variables, source);
    };

    let last = path.segments.last().expect("source paths are nonempty");
    let qualifier = namespace_prefix(bump, path);
    let binding = env
        .find_type(bump, path.region(), qualifier, last.value)
        .map_err(|error| vec![error])?;
    if args.is_empty() && binding.arity > 0 {
        let slots = bump
            .alloc_slice_fill_iter((0..binding.arity).map(|index| TypeSlot::Hole(index as u16)));
        return Ok(bump.alloc(Located::at(
            source.region,
            CanType::Partial {
                constructor: binding.reference,
                slots,
            },
        )));
    }
    if !args.iter().any(|arg| matches!(arg.value, SourceType::Hole)) {
        return canonicalize_type(bump, env, variables, source);
    }
    if args.len() != binding.arity {
        return Err(vec![Error::new(
            source.region,
            ErrorKind::Type(TypeError::BadArity {
                name: binding.reference.name,
                expected: binding.arity,
                actual: args.len(),
            }),
        )]);
    }

    let mut next_hole = 0u16;
    let mut slots = Vec::with_capacity(args.len());
    for arg in args {
        if matches!(arg.value, SourceType::Hole) {
            slots.push(TypeSlot::Hole(next_hole));
            next_hole = next_hole
                .checked_add(1)
                .expect("type hole count exceeds u16");
        } else {
            slots.push(TypeSlot::Fixed(canonicalize_type(
                bump, env, variables, arg,
            )?));
        }
    }
    Ok(bump.alloc(Located::at(
        source.region,
        CanType::Partial {
            constructor: binding.reference,
            slots: bump.alloc_slice_copy(&slots),
        },
    )))
}

pub(crate) fn is_task_type(typ: &Located<CanType<'_>>) -> bool {
    if let CanType::Alias { target, .. } = typ.value {
        let target = match target {
            alder_ast::AliasType::Open(target) | alder_ast::AliasType::Filled(target) => target,
        };
        return is_task_type(target);
    }
    matches!(
        typ.value,
        CanType::Named {
            reference,
            args: [_],
        } if reference.name == "Task"
    )
}

fn canonicalize_types<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    variables: &BTreeSet<&'a str>,
    source: &'a [&'a Located<SourceType<'a>>],
) -> Result<&'a [&'a Located<CanType<'a>>], Vec<Error<'a>>> {
    let mut result = Vec::with_capacity(source.len());
    let mut errors = Vec::new();
    for typ in source {
        match canonicalize_type(bump, env, variables, typ) {
            Ok(typ) => result.push(typ),
            Err(mut type_errors) => errors.append(&mut type_errors),
        }
    }
    if errors.is_empty() {
        Ok(bump.alloc_slice_copy(&result))
    } else {
        Err(errors)
    }
}

fn canonicalize_record_fields<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    variables: &BTreeSet<&'a str>,
    source: &'a [alder_source::FieldType<'a>],
) -> Result<&'a [RecordTypeField<'a>], Vec<Error<'a>>> {
    let mut seen = BTreeMap::new();
    let mut fields = Vec::with_capacity(source.len());
    let mut errors = Vec::new();
    for (index, field) in source.iter().enumerate() {
        if let Some(first) = seen.insert(field.field.value, field.field.region) {
            errors.push(Error::new(
                field.field.region,
                ErrorKind::Type(TypeError::DuplicateField {
                    name: field.field.value,
                    first,
                }),
            ));
        }
        match canonicalize_type(bump, env, variables, field.typ) {
            Ok(typ) => fields.push(RecordTypeField {
                index: index as u16,
                name: field.field.value,
                typ: crate::canonicalize::optional_annotation(bump, field.optional.is_some(), typ),
            }),
            Err(mut type_errors) => errors.append(&mut type_errors),
        }
    }
    if errors.is_empty() {
        fields.sort_by_key(|field| field.name);
        Ok(bump.alloc_slice_copy(&fields))
    } else {
        Err(errors)
    }
}

fn canonicalize_extension<'a>(
    variables: &BTreeSet<&'a str>,
    extension: Option<alder_source::Name<'a>>,
    region: Region,
) -> Result<RowExtension<'a>, Vec<Error<'a>>> {
    match extension {
        None => Ok(RowExtension::Closed),
        Some(name) if variables.contains(name.value) => Ok(RowExtension::Open(name.value)),
        Some(name) => Err(vec![Error::new(
            region,
            ErrorKind::Type(TypeError::UnboundVariable { name: name.value }),
        )]),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn aliases_are_ordered_after_dependencies_without_repeating_shared_nodes() {
        let bump = bumpalo::Bump::new();
        let text = bump.alloc_str(indoc::indoc! {r#"
            type Root = (Left, Right)
            type Right = Leaf
            type Left = Leaf
            type Leaf = Number
        "#});
        let source = alder_parse::parse_module(&bump, text).unwrap();
        let order = super::alias_order(&source).unwrap();
        let names: Vec<_> = order.iter().map(|alias| alias.name.value).collect();
        assert_eq!(names, ["Leaf", "Left", "Right", "Root"]);
    }

    use alder_ast::{ModuleId, PackageId};
    use bumpalo::Bump;

    use super::*;

    fn parse_type<'a>(bump: &'a Bump, source: &'a str) -> &'a Located<SourceType<'a>> {
        let mut parser = alder_parse::Parser::new(bump, source.as_bytes());
        parser.type_expr().expect("type parses")
    }

    fn env<'a>(bump: &'a Bump) -> Env<'a> {
        Env::new(
            bump,
            ModuleId {
                package: PackageId::Application,
                path: &[],
            },
        )
    }

    #[test]
    fn optional_record_field_canonicalizes_to_option() {
        let bump = Bump::new();
        let source = bump.alloc_str("{ name: String, nickname?: String }");
        let typ = canonicalize_type(
            &bump,
            &env(&bump),
            &BTreeSet::new(),
            parse_type(&bump, source),
        )
        .expect("type canonicalizes");
        let CanType::Record { fields, .. } = &typ.value else {
            panic!("expected record")
        };
        assert_eq!(fields[0].name, "name");
        assert_eq!(fields[1].name, "nickname");
        let CanType::Named { reference, args } = fields[1].typ.value else {
            panic!("optional syntax must canonicalize to a named Option type")
        };
        assert_eq!(reference.module.package, PackageId::Builtin);
        assert_eq!(reference.name, "Option");
        assert_eq!(args.len(), 1);
        assert!(
            matches!(args[0].value, CanType::Named { reference, .. } if reference.name == "String")
        );
    }

    #[test]
    fn function_arity_is_preserved() {
        let bump = Bump::new();
        let source = bump.alloc_str("fn(Number, String) Bool");
        let typ = canonicalize_type(
            &bump,
            &env(&bump),
            &BTreeSet::new(),
            parse_type(&bump, source),
        )
        .expect("type canonicalizes");
        let CanType::Fn { params, .. } = &typ.value else {
            panic!("expected function")
        };
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn result_one_argument_shorthand_is_accepted() {
        let bump = Bump::new();
        let source = bump.alloc_str("Result[String]");
        canonicalize_type(
            &bump,
            &env(&bump),
            &BTreeSet::new(),
            parse_type(&bump, source),
        )
        .expect("shorthand canonicalizes");
    }

    #[test]
    fn impl_head_partial_constructor_is_preserved() {
        let bump = Bump::new();
        let source = bump.alloc_str("Result[_, String]");
        let typ = canonicalize_impl_head_type(
            &bump,
            &env(&bump),
            &BTreeSet::new(),
            parse_type(&bump, source),
        )
        .expect("partial impl head canonicalizes");
        let CanType::Partial { constructor, slots } = &typ.value else {
            panic!("expected partial constructor")
        };
        assert_eq!(constructor.name, "Result");
        assert!(matches!(slots[0], TypeSlot::Hole(0)));
        assert!(matches!(slots[1], TypeSlot::Fixed(_)));
    }

    #[test]
    fn bare_constructor_impl_head_becomes_a_partial_type() {
        let bump = Bump::new();
        let source = bump.alloc_str("Option");
        let typ = canonicalize_impl_head_type(
            &bump,
            &env(&bump),
            &BTreeSet::new(),
            parse_type(&bump, source),
        )
        .expect("bare constructor impl head canonicalizes");
        let CanType::Partial { constructor, slots } = &typ.value else {
            panic!("expected partial constructor")
        };
        assert_eq!(constructor.name, "Option");
        assert!(matches!(slots, [TypeSlot::Hole(0)]));
    }

    #[test]
    fn ordinary_type_hole_is_rejected() {
        let bump = Bump::new();
        let source = bump.alloc_str("_");
        let errors = canonicalize_type(
            &bump,
            &env(&bump),
            &BTreeSet::new(),
            parse_type(&bump, source),
        )
        .expect_err("ordinary type hole must fail");
        assert!(matches!(
            errors[0].kind,
            ErrorKind::Type(TypeError::InvalidHole)
        ));
    }

    #[test]
    fn nested_impl_head_hole_is_rejected() {
        let bump = Bump::new();
        let source = bump.alloc_str("Result[Array[_], String]");
        let errors = canonicalize_impl_head_type(
            &bump,
            &env(&bump),
            &BTreeSet::new(),
            parse_type(&bump, source),
        )
        .expect_err("nested type hole must fail");
        assert!(matches!(
            errors[0].kind,
            ErrorKind::Type(TypeError::InvalidHole)
        ));
    }
}
