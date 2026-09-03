use std::collections::{BTreeMap, BTreeSet};

use alder_ast::{
    ErrorTagType, FieldPresence, RecordTypeField, RowExtension, Type as CanType, TypeSlot,
};
use alder_region::{Located, Region};
use alder_source::Type as SourceType;
use bumpalo::Bump;

use crate::environment::Env;
use crate::{Error, ErrorKind, TypeError};

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
            let qualifier = (path.segments.len() > 1).then(|| path.segments[0].value);
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
    let qualifier = (path.segments.len() > 1).then(|| path.segments[0].value);
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
                presence: if field.optional.is_some() {
                    FieldPresence::Optional
                } else {
                    FieldPresence::Required
                },
                typ,
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
    use alder_ast::{ModuleId, PackageId};
    use bumpalo::Bump;

    use super::*;

    fn parse_type<'a>(bump: &'a Bump, source: &'a str) -> &'a Located<SourceType<'a>> {
        let mut parser = alder_parse::Parser::new(bump, source.as_bytes());
        parser.type_expr().expect("type parses")
    }

    fn env() -> Env<'static> {
        Env::new(ModuleId {
            package: PackageId::Application,
            path: &[],
        })
    }

    #[test]
    fn optional_record_field_is_preserved() {
        let bump = Bump::new();
        let source = bump.alloc_str("{ name: String, nickname?: String }");
        let typ = canonicalize_type(&bump, &env(), &BTreeSet::new(), parse_type(&bump, source))
            .expect("type canonicalizes");
        let CanType::Record { fields, .. } = &typ.value else {
            panic!("expected record")
        };
        assert_eq!(fields[0].name, "name");
        assert_eq!(fields[0].presence, FieldPresence::Required);
        assert_eq!(fields[1].name, "nickname");
        assert_eq!(fields[1].presence, FieldPresence::Optional);
    }

    #[test]
    fn function_arity_is_preserved() {
        let bump = Bump::new();
        let source = bump.alloc_str("fn(Number, String) -> Bool");
        let typ = canonicalize_type(&bump, &env(), &BTreeSet::new(), parse_type(&bump, source))
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
        canonicalize_type(&bump, &env(), &BTreeSet::new(), parse_type(&bump, source))
            .expect("shorthand canonicalizes");
    }

    #[test]
    fn impl_head_partial_constructor_is_preserved() {
        let bump = Bump::new();
        let source = bump.alloc_str("Result[_, String]");
        let typ =
            canonicalize_impl_head_type(&bump, &env(), &BTreeSet::new(), parse_type(&bump, source))
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
        let typ =
            canonicalize_impl_head_type(&bump, &env(), &BTreeSet::new(), parse_type(&bump, source))
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
        let errors = canonicalize_type(&bump, &env(), &BTreeSet::new(), parse_type(&bump, source))
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
        let errors =
            canonicalize_impl_head_type(&bump, &env(), &BTreeSet::new(), parse_type(&bump, source))
                .expect_err("nested type hole must fail");
        assert!(matches!(
            errors[0].kind,
            ErrorKind::Type(TypeError::InvalidHole)
        ));
    }
}
