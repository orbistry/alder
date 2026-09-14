use super::*;
use alder_codegen::support::remote::{WireNode, WireSchema, WireVariant};

pub(super) fn schema(
    typ: &OwnedType,
    declarations: &BTreeMap<OwnedQualifiedName, &OwnedTypeDecl>,
) -> Result<WireSchema, String> {
    let mut builder = Builder {
        declarations,
        nodes: vec![],
        memo: BTreeMap::new(),
    };
    let typ = concrete(typ, &BTreeMap::new())?;
    let root = builder.node(&typ)?;
    Ok(WireSchema {
        root,
        nodes: builder.nodes,
    })
}

struct Builder<'a> {
    declarations: &'a BTreeMap<OwnedQualifiedName, &'a OwnedTypeDecl>,
    nodes: Vec<WireNode>,
    memo: BTreeMap<Vec<u8>, u32>,
}

impl Builder<'_> {
    fn node(&mut self, typ: &OwnedType) -> Result<u32, String> {
        let key = bincode::serialize(typ).expect("owned type serializes");
        if let Some(index) = self.memo.get(&key) {
            return Ok(*index);
        }
        if self.nodes.len() >= 4096 {
            return Err("wire schema exceeds 4096 distinct instantiated types".into());
        }
        let index = self.nodes.len() as u32;
        self.memo.insert(key, index);
        self.nodes.push(WireNode::Unit);
        let node = match typ {
            OwnedType::Unit => WireNode::Unit,
            OwnedType::Tuple(items) => WireNode::Tuple(
                items
                    .iter()
                    .map(|item| self.node(&item.typ))
                    .collect::<Result<_, _>>()?,
            ),
            OwnedType::Record { fields, ext: None } => WireNode::Record(self.fields(fields)?),
            OwnedType::ErrorRow { tags, ext } => WireNode::ErrorRow {
                variants: self.tags(tags)?,
                open: ext.is_some(),
            },
            OwnedType::Named { reference, args } => {
                if reference.module.package == OwnedPackageId::Builtin
                    && reference.module.path.is_empty()
                {
                    match (reference.name.as_str(), args.as_slice()) {
                        ("String", []) => WireNode::String,
                        ("Number", []) => WireNode::Number,
                        ("Bool", []) => WireNode::Bool,
                        ("BigInt", []) => WireNode::BigInt,
                        ("Date", []) => WireNode::Date,
                        ("Array", [item]) => WireNode::Array(self.node(&item.typ)?),
                        ("Option", [item]) => WireNode::Option(self.node(&item.typ)?),
                        ("Set", [item]) => WireNode::Set(self.node(&item.typ)?),
                        ("Map", [key, value]) => {
                            WireNode::Map(self.node(&key.typ)?, self.node(&value.typ)?)
                        }
                        ("Result", [ok, err]) => WireNode::Enum(vec![
                            WireVariant {
                                tag: "Ok".into(),
                                fields: vec![("_0".into(), self.node(&ok.typ)?)],
                            },
                            WireVariant {
                                tag: "Err".into(),
                                fields: vec![("_0".into(), self.node(&err.typ)?)],
                            },
                        ]),
                        _ => {
                            return Err(format!("opaque or unsupported type `{}`", reference.name));
                        }
                    }
                } else {
                    let Some(declaration) = self.declarations.get(reference).copied() else {
                        return Err(format!("opaque or unsupported type `{}`", reference.name));
                    };
                    if declaration.params.len() != args.len() {
                        return Err(format!("unsaturated type `{}`", reference.name));
                    }
                    let substitutions = declaration
                        .params
                        .iter()
                        .zip(args)
                        .map(|(param, arg)| (param.name.clone(), arg.typ.clone()))
                        .collect();
                    match &declaration.body {
                        OwnedPublicTypeBody::Alias(target) => {
                            let target = concrete(&target.typ, &substitutions)?;
                            let target = self.node(&target)?;
                            self.nodes[target as usize].clone()
                        }
                        OwnedPublicTypeBody::Enum(variants) => {
                            let mut output = Vec::new();
                            for variant in variants {
                                let fields = match &variant.payload {
                                    OwnedVariantPayload::Unit => vec![],
                                    OwnedVariantPayload::Tuple(items) => items
                                        .iter()
                                        .enumerate()
                                        .map(|(index, item)| {
                                            Ok((
                                                format!("_{index}"),
                                                self.node(&concrete(&item.typ, &substitutions)?)?,
                                            ))
                                        })
                                        .collect::<Result<_, String>>()?,
                                    OwnedVariantPayload::Record(fields) => fields
                                        .iter()
                                        .map(|field| {
                                            Ok((
                                                field.name.clone(),
                                                self.node(&concrete(
                                                    &field.typ.typ,
                                                    &substitutions,
                                                )?)?,
                                            ))
                                        })
                                        .collect::<Result<_, String>>()?,
                                };
                                output.push(WireVariant {
                                    tag: variant.name.clone(),
                                    fields,
                                });
                            }
                            WireNode::Enum(output)
                        }
                        OwnedPublicTypeBody::ErrorGroup(tags) => {
                            let tags = tags
                                .iter()
                                .map(|tag| {
                                    Ok(OwnedErrorTag {
                                        index: tag.index,
                                        name: tag.name.clone(),
                                        args: tag
                                            .args
                                            .iter()
                                            .map(|arg| concrete(&arg.typ, &substitutions).map(at))
                                            .collect::<Result<_, String>>()?,
                                    })
                                })
                                .collect::<Result<Vec<_>, String>>()?;
                            WireNode::ErrorRow {
                                variants: self.tags(&tags)?,
                                open: false,
                            }
                        }
                        OwnedPublicTypeBody::Opaque(_) => {
                            return Err(format!("opaque type `{}`", reference.name));
                        }
                    }
                }
            }
            _ => return Err("wire type is not a concrete serializable value".into()),
        };
        self.nodes[index as usize] = node;
        Ok(index)
    }

    fn fields(&mut self, fields: &[OwnedRecordField]) -> Result<Vec<(String, u32)>, String> {
        fields
            .iter()
            .map(|field| Ok((field.name.clone(), self.node(&field.typ.typ)?)))
            .collect()
    }

    fn tags(&mut self, tags: &[OwnedErrorTag]) -> Result<Vec<WireVariant>, String> {
        tags.iter()
            .map(|tag| {
                Ok(WireVariant {
                    tag: format!(":{}", tag.name),
                    fields: tag
                        .args
                        .iter()
                        .enumerate()
                        .map(|(index, arg)| Ok((format!("_{index}"), self.node(&arg.typ)?)))
                        .collect::<Result<_, String>>()?,
                })
            })
            .collect()
    }
}

/// Remove aliases, source regions, and record declaration order from schema
/// keys. Generic nominal recursion then shares a node instead of expanding.
fn concrete(
    typ: &OwnedType,
    substitutions: &BTreeMap<String, OwnedType>,
) -> Result<OwnedType, String> {
    match underlying(typ) {
        OwnedType::Unit => Ok(OwnedType::Unit),
        OwnedType::Var { name, args } if args.is_empty() => {
            substitutions.get(name).cloned().ok_or_else(|| {
                format!("unresolved type parameter `{name}`; annotate the wire payload")
            })
        }
        OwnedType::Named { reference, args } => Ok(OwnedType::Named {
            reference: reference.clone(),
            args: args
                .iter()
                .map(|arg| concrete(&arg.typ, substitutions).map(at))
                .collect::<Result<_, _>>()?,
        }),
        OwnedType::Tuple(items) => Ok(OwnedType::Tuple(
            items
                .iter()
                .map(|item| concrete(&item.typ, substitutions).map(at))
                .collect::<Result<_, _>>()?,
        )),
        OwnedType::Record { fields, ext: None } => {
            let mut fields = fields
                .iter()
                .map(|field| {
                    Ok(OwnedRecordField {
                        index: 0,
                        name: field.name.clone(),
                        typ: at(concrete(&field.typ.typ, substitutions)?),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            fields.sort_by(|a, b| a.name.cmp(&b.name));
            for (index, field) in fields.iter_mut().enumerate() {
                field.index = index as u16;
            }
            Ok(OwnedType::Record { fields, ext: None })
        }
        OwnedType::ErrorRow { tags, ext } => Ok(OwnedType::ErrorRow {
            tags: tags
                .iter()
                .map(|tag| {
                    Ok(OwnedErrorTag {
                        index: tag.index,
                        name: tag.name.clone(),
                        args: tag
                            .args
                            .iter()
                            .map(|arg| concrete(&arg.typ, substitutions).map(at))
                            .collect::<Result<_, _>>()?,
                    })
                })
                .collect::<Result<_, String>>()?,
            ext: ext.as_ref().map(|_| "open".into()),
        }),
        OwnedType::Fn { .. } => Err("functions cannot cross HTTP".into()),
        OwnedType::Record { ext: Some(_), .. } => Err("open record rows cannot cross HTTP".into()),
        _ => Err("unresolved or higher-kinded types cannot cross HTTP".into()),
    }
}
