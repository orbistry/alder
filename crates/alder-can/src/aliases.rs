//! Capture-avoiding expansion of canonical alias bodies.

use std::collections::BTreeMap;

use alder_ast::{AliasArgument, AliasType, Node, QualifiedName, RowExtension, Type, TypeSlot};
use alder_region::{Located, Region};
use bumpalo::Bump;

use crate::{Error, ErrorKind, TypeError};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Definition<'a> {
    pub params: &'a [&'a str],
    pub body: Node<'a, Type<'a>>,
}

pub(crate) fn instantiate<'a>(
    bump: &'a Bump,
    reference: QualifiedName<'a>,
    definition: &Definition<'a>,
    args: &'a [Node<'a, Type<'a>>],
    region: Region,
) -> Result<Node<'a, Type<'a>>, Vec<Error<'a>>> {
    let bindings = definition
        .params
        .iter()
        .copied()
        .zip(args.iter().copied())
        .collect();
    let target = substitute(bump, definition.body, &bindings)?;
    let arguments = definition
        .params
        .iter()
        .zip(args)
        .map(|(&name, &typ)| AliasArgument { name, typ })
        .collect::<Vec<_>>();
    Ok(bump.alloc(Located::at(
        region,
        Type::Alias {
            reference,
            arguments: bump.alloc_slice_copy(&arguments),
            target: AliasType::Filled(target),
        },
    )))
}

fn invalid(region: Region) -> Vec<Error<'static>> {
    vec![Error::new(
        region,
        ErrorKind::Type(TypeError::InvalidAliasArgument),
    )]
}

fn real<'a>(mut typ: Node<'a, Type<'a>>) -> Node<'a, Type<'a>> {
    while let Type::Alias { target, .. } = typ.value {
        typ = match target {
            AliasType::Open(typ) | AliasType::Filled(typ) => typ,
        };
    }
    typ
}

fn substitute<'a>(
    bump: &'a Bump,
    typ: Node<'a, Type<'a>>,
    bindings: &BTreeMap<&'a str, Node<'a, Type<'a>>>,
) -> Result<Node<'a, Type<'a>>, Vec<Error<'a>>> {
    let list =
        |items: &'a [Node<'a, Type<'a>>]| -> Result<&'a [Node<'a, Type<'a>>], Vec<Error<'a>>> {
            let items = items
                .iter()
                .map(|item| substitute(bump, item, bindings))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(bump.alloc_slice_copy(&items))
        };
    let value = match &typ.value {
        Type::Var { name, args } => {
            let args = list(args)?;
            if let Some(&replacement) = bindings.get(name) {
                // Replacement variables belong to the caller, not the alias:
                // never recursively substitute into the replacement itself.
                if args.is_empty() {
                    return Ok(replacement);
                }
                let replacement = real(replacement);
                match &replacement.value {
                    Type::Var {
                        name,
                        args: existing,
                    } => {
                        let combined = existing.iter().chain(args).copied().collect::<Vec<_>>();
                        Type::Var {
                            name,
                            args: bump.alloc_slice_copy(&combined),
                        }
                    }
                    Type::Named {
                        reference,
                        args: existing,
                    } => {
                        let combined = existing.iter().chain(args).copied().collect::<Vec<_>>();
                        Type::Named {
                            reference: *reference,
                            args: bump.alloc_slice_copy(&combined),
                        }
                    }
                    _ => return Err(invalid(typ.region)),
                }
            } else {
                Type::Var { name, args }
            }
        }
        Type::Named { reference, args } => Type::Named {
            reference: *reference,
            args: list(args)?,
        },
        Type::Partial { constructor, slots } => {
            let slots = slots
                .iter()
                .map(|slot| {
                    Ok(match slot {
                        TypeSlot::Hole(index) => TypeSlot::Hole(*index),
                        TypeSlot::Fixed(typ) => TypeSlot::Fixed(substitute(bump, typ, bindings)?),
                    })
                })
                .collect::<Result<Vec<_>, Vec<Error<'a>>>>()?;
            Type::Partial {
                constructor: *constructor,
                slots: bump.alloc_slice_copy(&slots),
            }
        }
        Type::Projection(projection) => Type::Projection(alder_ast::ProjectionType {
            trait_ref: alder_ast::TraitRef {
                trait_: projection.trait_ref.trait_,
                args: list(projection.trait_ref.args)?,
            },
            assoc: projection.assoc,
        }),
        Type::Fn { params, ret } => Type::Fn {
            params: list(params)?,
            ret: substitute(bump, ret, bindings)?,
        },
        Type::Unit => Type::Unit,
        Type::Tuple(items) => Type::Tuple(list(items)?),
        Type::Record { fields, ext } => {
            let mut fields = fields
                .iter()
                .map(|field| {
                    Ok(alder_ast::RecordTypeField {
                        typ: substitute(bump, field.typ, bindings)?,
                        ..*field
                    })
                })
                .collect::<Result<Vec<_>, Vec<Error<'a>>>>()?;
            let mut ext = *ext;
            if let RowExtension::Open(name) = ext
                && let Some(&replacement) = bindings.get(name)
            {
                match &real(replacement).value {
                    Type::Var { name, args: [] } => ext = RowExtension::Open(name),
                    Type::Record {
                        fields: inherited,
                        ext: tail,
                    } => {
                        for field in *inherited {
                            if fields.iter().any(|existing| existing.name == field.name) {
                                return Err(invalid(typ.region));
                            }
                            fields.push(*field);
                        }
                        ext = *tail;
                    }
                    _ => return Err(invalid(typ.region)),
                }
            }
            for (index, field) in fields.iter_mut().enumerate() {
                field.index = index as u16;
            }
            Type::Record {
                fields: bump.alloc_slice_copy(&fields),
                ext,
            }
        }
        Type::ErrorRow { tags, ext } => {
            let mut tags = tags
                .iter()
                .map(|tag| {
                    Ok(alder_ast::ErrorTagType {
                        args: list(tag.args)?,
                        ..*tag
                    })
                })
                .collect::<Result<Vec<_>, Vec<Error<'a>>>>()?;
            let mut ext = *ext;
            if let RowExtension::Open(name) = ext
                && let Some(&replacement) = bindings.get(name)
            {
                match &real(replacement).value {
                    Type::Var { name, args: [] } => ext = RowExtension::Open(name),
                    Type::ErrorRow {
                        tags: inherited,
                        ext: tail,
                    } => {
                        for tag in *inherited {
                            if tags.iter().any(|existing| existing.name == tag.name) {
                                return Err(invalid(typ.region));
                            }
                            tags.push(*tag);
                        }
                        ext = *tail;
                    }
                    _ => return Err(invalid(typ.region)),
                }
            }
            for (index, tag) in tags.iter_mut().enumerate() {
                tag.index = index as u16;
            }
            Type::ErrorRow {
                tags: bump.alloc_slice_copy(&tags),
                ext,
            }
        }
        Type::Alias {
            reference,
            arguments,
            target,
        } => {
            let arguments = arguments
                .iter()
                .map(|argument| {
                    Ok(AliasArgument {
                        name: argument.name,
                        typ: substitute(bump, argument.typ, bindings)?,
                    })
                })
                .collect::<Result<Vec<_>, Vec<Error<'a>>>>()?;
            let target = match target {
                AliasType::Filled(target) => substitute(bump, target, bindings)?,
                AliasType::Open(target) => {
                    let local = arguments
                        .iter()
                        .map(|argument| (argument.name, argument.typ))
                        .collect();
                    substitute(bump, target, &local)?
                }
            };
            Type::Alias {
                reference: *reference,
                arguments: bump.alloc_slice_copy(&arguments),
                target: AliasType::Filled(target),
            }
        }
    };
    Ok(bump.alloc(Located::at(typ.region, value)))
}
