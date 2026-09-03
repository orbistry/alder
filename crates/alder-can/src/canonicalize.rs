use std::collections::{BTreeMap, BTreeSet};

use alder_ast::{
    Annotation, Attribute, ComponentDecl, ConstructorName, ConstructorRef, EnumDecl, ErrorGroup,
    ErrorTagType, ExternDecl, FnDecl, ImplDecl, ImplFn, ImplItem, Interface, Item, ItemKind,
    MacroDecl, Modifier, Module, ModuleId, Param, QualifiedName, ResolvedImport,
    ResolvedImportKind, SchemaDecl, SchemaItem, TableColumn, TableDecl, TestDecl, TopLevelLet,
    TraitDecl, TraitFn, TraitItem, Type, TypeAlias, TypeConstraint, Variant, VariantPayload,
    Visibility,
};
use alder_region::{Located, Region};
use alder_source::{Item as SourceItem, ItemKind as SourceItemKind, Module as SourceModule};
use bumpalo::Bump;

use crate::environment::{Env, MethodBinding};
use crate::expression::{canonicalize_block, canonicalize_expr};
use crate::pattern::{BindingMode, canonicalize_pattern};
use crate::types::{canonicalize_impl_head_type, canonicalize_type, is_task_type};
use crate::{
    AttributeError, Error, ErrorKind, ImportError, ItemError, NameError, TypeError, Warning,
};

/// All dependency information needed to canonicalize one source module.
#[derive(Clone, Copy)]
pub struct Context<'a> {
    pub home: ModuleId<'a>,
    pub imports: &'a [ResolvedImport<'a>],
    pub interfaces: &'a [Interface<'a>],
}

#[derive(Debug)]
pub struct CanResult<'a> {
    pub module: &'a Module<'a>,
    pub warnings: &'a [Warning<'a>],
}

/// Canonicalize one parsed module after the driver has resolved its imports.
pub fn canonicalize<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    source: &SourceModule<'a>,
) -> Result<CanResult<'a>, Vec<Error<'a>>> {
    let mut env = Env::new(bump, context.home);
    let mut errors = load_imports(bump, &mut env, context.imports, context.interfaces);
    errors.extend(predeclare(&mut env, source));
    let enums = canonicalize_enums(bump, &mut env, source, &mut errors);
    errors.extend(predeclare_trait_members(bump, &mut env, source));
    if !errors.is_empty() {
        return Err(errors);
    }

    let mut items = Vec::new();
    let mut automatic_impls = Vec::new();
    for (item_ordinal, item) in source.items.iter().enumerate() {
        if matches!(item.value.kind, SourceItemKind::Import(_)) {
            continue;
        }
        match canonicalize_item(bump, &mut env, item, &enums, item_ordinal as u32) {
            Ok(canonical_item) => {
                items.push(canonical_item);
                if let ItemKind::Enum(enum_) = &canonical_item.value.kind {
                    if enum_supports_structural_derive(enum_) {
                        automatic_impls.push(enum_derive_impl(
                            bump,
                            context.home,
                            enum_,
                            canonical_item.region,
                            alder_ast::DeriveKind::Eq,
                            alder_ast::ImplOrigin::AutomaticEq {
                                type_ordinal: item_ordinal as u32,
                            },
                        ));
                    }
                    for attribute in canonical_item.value.attributes {
                        let Attribute::Derive { region, names } = attribute else {
                            continue;
                        };
                        for (derive_index, name) in names.iter().enumerate() {
                            let kind = derive_kind(name.name)
                                .expect("derive attributes contain validated built-in names");
                            if kind == alder_ast::DeriveKind::Eq {
                                if !enum_supports_structural_derive(enum_) {
                                    errors.push(Error::new(
                                        *region,
                                        ErrorKind::Attribute(AttributeError::InvalidDerive {
                                            reason: "function-valued fields cannot derive Eq",
                                        }),
                                    ));
                                }
                                continue;
                            }
                            if !enum_supports_structural_derive(enum_) {
                                errors.push(Error::new(
                                    *region,
                                    ErrorKind::Attribute(AttributeError::InvalidDerive {
                                        reason: "function-valued fields cannot use built-in derives",
                                    }),
                                ));
                                continue;
                            }
                            automatic_impls.push(enum_derive_impl(
                                bump,
                                context.home,
                                enum_,
                                canonical_item.region,
                                kind,
                                alder_ast::ImplOrigin::Derived {
                                    type_ordinal: item_ordinal as u32,
                                    derive_index: derive_index as u16,
                                },
                            ));
                        }
                    }
                }
                if let ItemKind::ErrorGroup(group) = &canonical_item.value.kind {
                    if error_group_supports_structural_derive(group) {
                        automatic_impls.push(error_group_derive_impl(
                            bump,
                            context.home,
                            group,
                            canonical_item.region,
                            alder_ast::DeriveKind::Eq,
                            alder_ast::ImplOrigin::AutomaticEq {
                                type_ordinal: item_ordinal as u32,
                            },
                        ));
                    }
                    for attribute in canonical_item.value.attributes {
                        let Attribute::Derive { region, names } = attribute else {
                            continue;
                        };
                        for (derive_index, name) in names.iter().enumerate() {
                            let kind = derive_kind(name.name)
                                .expect("derive attributes contain validated built-in names");
                            if kind == alder_ast::DeriveKind::Eq {
                                if !error_group_supports_structural_derive(group) {
                                    errors.push(Error::new(
                                        *region,
                                        ErrorKind::Attribute(AttributeError::InvalidDerive {
                                            reason: "function-valued fields cannot derive Eq",
                                        }),
                                    ));
                                }
                                continue;
                            }
                            if !error_group_supports_structural_derive(group) {
                                errors.push(Error::new(
                                    *region,
                                    ErrorKind::Attribute(AttributeError::InvalidDerive {
                                        reason: "function-valued fields cannot use built-in derives",
                                    }),
                                ));
                                continue;
                            }
                            automatic_impls.push(error_group_derive_impl(
                                bump,
                                context.home,
                                group,
                                canonical_item.region,
                                kind,
                                alder_ast::ImplOrigin::Derived {
                                    type_ordinal: item_ordinal as u32,
                                    derive_index: derive_index as u16,
                                },
                            ));
                        }
                    }
                }
            }
            Err(mut item_errors) => errors.append(&mut item_errors),
        }
    }
    items.extend(automatic_impls);
    if !errors.is_empty() {
        return Err(errors);
    }

    let items = bump.alloc_slice_copy(&items);
    let value_sccs = crate::value_scc::build(bump, context.home, items);
    let module = bump.alloc(Module {
        id: context.home,
        imports: context.imports,
        items,
        value_sccs,
    });

    Ok(CanResult {
        module,
        warnings: &[],
    })
}

fn enum_supports_structural_derive(enum_: &EnumDecl<'_>) -> bool {
    enum_.variants.iter().all(|variant| match variant.payload {
        VariantPayload::Unit => true,
        VariantPayload::Tuple(types) => {
            types.iter().all(|typ| type_supports_structural_derive(typ))
        }
        VariantPayload::Record(fields) => fields
            .iter()
            .all(|field| type_supports_structural_derive(field.typ)),
    })
}

fn error_group_supports_structural_derive(group: &ErrorGroup<'_>) -> bool {
    group.tags.iter().all(|tag| {
        tag.args
            .iter()
            .all(|typ| type_supports_structural_derive(typ))
    })
}

fn type_supports_structural_derive(typ: &Located<Type<'_>>) -> bool {
    match &typ.value {
        Type::Fn { .. } => false,
        Type::Var { args, .. } | Type::Named { args, .. } => args
            .iter()
            .all(|argument| type_supports_structural_derive(argument)),
        Type::Partial { slots, .. } => slots.iter().all(|slot| match slot {
            alder_ast::TypeSlot::Hole(_) => true,
            alder_ast::TypeSlot::Fixed(typ) => type_supports_structural_derive(typ),
        }),
        Type::Projection(projection) => projection
            .trait_ref
            .args
            .iter()
            .all(|argument| type_supports_structural_derive(argument)),
        Type::Tuple(items) => items
            .iter()
            .all(|item| type_supports_structural_derive(item)),
        Type::Record { fields, .. } => fields
            .iter()
            .all(|field| type_supports_structural_derive(field.typ)),
        Type::Alias { target, .. } => match target {
            alder_ast::AliasType::Open(typ) | alder_ast::AliasType::Filled(typ) => {
                type_supports_structural_derive(typ)
            }
        },
        Type::Unit | Type::ErrorRow { .. } => true,
    }
}

fn derive_kind(name: &str) -> Option<alder_ast::DeriveKind> {
    match name {
        "Show" => Some(alder_ast::DeriveKind::Show),
        "Eq" => Some(alder_ast::DeriveKind::Eq),
        "Ord" => Some(alder_ast::DeriveKind::Ord),
        "Hash" => Some(alder_ast::DeriveKind::Hash),
        "Json" => Some(alder_ast::DeriveKind::Json),
        _ => None,
    }
}

fn enum_derive_impl<'a>(
    bump: &'a Bump,
    home: ModuleId<'a>,
    enum_: &'a EnumDecl<'a>,
    region: Region,
    kind: alder_ast::DeriveKind,
    origin: alder_ast::ImplOrigin,
) -> &'a Located<Item<'a>> {
    let params = bump.alloc_slice_fill_iter(enum_.params.iter().map(|name| alder_ast::TypeParam {
        name: *name,
        kind: alder_ast::Kind::Type,
    }));
    let arguments = bump.alloc_slice_fill_iter(enum_.params.iter().map(|name| {
        bump.alloc(Located::at(
            name.region,
            Type::Var {
                name: name.value,
                args: &[],
            },
        )) as &Located<Type<'a>>
    }));
    let subject = bump.alloc(Located::at(
        region,
        Type::Named {
            reference: enum_.name,
            args: arguments,
        },
    ));
    let trait_ = alder_ast::TraitId(QualifiedName {
        module: ModuleId {
            package: alder_ast::PackageId::Builtin,
            path: &[],
        },
        name: match kind {
            alder_ast::DeriveKind::Show => "Show",
            alder_ast::DeriveKind::Eq => "Eq",
            alder_ast::DeriveKind::Ord => "Ord",
            alder_ast::DeriveKind::Hash => "Hash",
            alder_ast::DeriveKind::Json => "Json",
        },
    });
    let trait_predicates = enum_
        .params
        .iter()
        .zip(arguments.iter().copied())
        .filter(|(parameter, _)| enum_payload_mentions(enum_, parameter.value))
        .map(|(_, argument)| alder_ast::TraitRef {
            trait_,
            args: bump.alloc_slice_copy(&[argument]),
        })
        .collect::<Vec<_>>();
    let trait_predicates = bump.alloc_slice_copy(&trait_predicates);
    let trait_ref = alder_ast::TraitRef {
        trait_,
        args: bump.alloc_slice_copy(&[subject as &Located<Type<'a>>]),
    };
    bump.alloc(Located::at(
        region,
        Item {
            visibility: Visibility::Private,
            attributes: &[],
            kind: ItemKind::Impl(bump.alloc(ImplDecl {
                id: alder_ast::ImplId {
                    module: home,
                    origin,
                },
                trait_: trait_.0,
                args: trait_ref.args,
                trait_ref,
                params,
                constraints: &[],
                trait_predicates,
                projection_equalities: &[],
                assoc_bindings: &[],
                items: &[],
                synthetic: Some(kind),
                region,
            })),
        },
    ))
}

fn error_group_derive_impl<'a>(
    bump: &'a Bump,
    home: ModuleId<'a>,
    group: &'a ErrorGroup<'a>,
    region: Region,
    kind: alder_ast::DeriveKind,
    origin: alder_ast::ImplOrigin,
) -> &'a Located<Item<'a>> {
    let subject = bump.alloc(Located::at(
        region,
        Type::Named {
            reference: group.name,
            args: &[],
        },
    ));
    let trait_ = alder_ast::TraitId(QualifiedName {
        module: ModuleId {
            package: alder_ast::PackageId::Builtin,
            path: &[],
        },
        name: match kind {
            alder_ast::DeriveKind::Show => "Show",
            alder_ast::DeriveKind::Eq => "Eq",
            alder_ast::DeriveKind::Ord => "Ord",
            alder_ast::DeriveKind::Hash => "Hash",
            alder_ast::DeriveKind::Json => "Json",
        },
    });
    let trait_ref = alder_ast::TraitRef {
        trait_,
        args: bump.alloc_slice_copy(&[subject as &Located<Type<'a>>]),
    };
    bump.alloc(Located::at(
        region,
        Item {
            visibility: Visibility::Private,
            attributes: &[],
            kind: ItemKind::Impl(bump.alloc(ImplDecl {
                id: alder_ast::ImplId {
                    module: home,
                    origin,
                },
                trait_: trait_.0,
                args: trait_ref.args,
                trait_ref,
                params: &[],
                constraints: &[],
                trait_predicates: &[],
                projection_equalities: &[],
                assoc_bindings: &[],
                items: &[],
                synthetic: Some(kind),
                region,
            })),
        },
    ))
}

fn enum_payload_mentions(enum_: &EnumDecl<'_>, variable: &str) -> bool {
    enum_.variants.iter().any(|variant| match variant.payload {
        VariantPayload::Unit => false,
        VariantPayload::Tuple(types) => types
            .iter()
            .any(|typ| canonical_type_mentions(&typ.value, variable)),
        VariantPayload::Record(fields) => fields
            .iter()
            .any(|field| canonical_type_mentions(&field.typ.value, variable)),
    })
}

fn canonical_type_mentions(typ: &Type<'_>, variable: &str) -> bool {
    match typ {
        Type::Var { name, args } => {
            *name == variable
                || args
                    .iter()
                    .any(|argument| canonical_type_mentions(&argument.value, variable))
        }
        Type::Named { args, .. } | Type::Tuple(args) => args
            .iter()
            .any(|argument| canonical_type_mentions(&argument.value, variable)),
        Type::Partial { slots, .. } => slots.iter().any(|slot| match slot {
            alder_ast::TypeSlot::Hole(_) => false,
            alder_ast::TypeSlot::Fixed(typ) => canonical_type_mentions(&typ.value, variable),
        }),
        Type::Projection(projection) => projection
            .trait_ref
            .args
            .iter()
            .any(|argument| canonical_type_mentions(&argument.value, variable)),
        Type::Fn { params, ret } => {
            params
                .iter()
                .any(|parameter| canonical_type_mentions(&parameter.value, variable))
                || canonical_type_mentions(&ret.value, variable)
        }
        Type::Record { fields, .. } => fields
            .iter()
            .any(|field| canonical_type_mentions(&field.typ.value, variable)),
        Type::Alias { target, .. } => match target {
            alder_ast::AliasType::Open(typ) | alder_ast::AliasType::Filled(typ) => {
                canonical_type_mentions(&typ.value, variable)
            }
        },
        Type::ErrorRow { tags, .. } => tags.iter().any(|tag| {
            tag.args
                .iter()
                .any(|argument| canonical_type_mentions(&argument.value, variable))
        }),
        Type::Unit => false,
    }
}

fn load_imports<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    imports: &'a [ResolvedImport<'a>],
    interfaces: &'a [Interface<'a>],
) -> Vec<Error<'a>> {
    let mut errors = Vec::new();
    for import in imports {
        let interface = interfaces
            .iter()
            .find(|interface| interface.home == import.module);
        match import.kind {
            ResolvedImportKind::Module { binding } => {
                if let Err(first) =
                    env.insert_module(binding.value, binding.region, import.module, interface)
                {
                    errors.push(Error::new(
                        binding.region,
                        ErrorKind::Import(ImportError::AliasCollision {
                            name: binding.value,
                            first,
                        }),
                    ));
                }
            }
            ResolvedImportKind::Names(names) => {
                let Some(interface) = interface else {
                    errors.push(missing_import_interface(import));
                    continue;
                };
                for name in names {
                    if let Err(error) =
                        import_name(bump, env, interface, name.source.value, name.binding)
                    {
                        errors.push(error);
                    }
                }
            }
            ResolvedImportKind::All => {
                let Some(interface) = interface else {
                    errors.push(missing_import_interface(import));
                    continue;
                };
                import_all(bump, env, interface, import.region, &mut errors);
            }
        }
    }
    errors
}

fn missing_import_interface<'a>(import: &ResolvedImport<'a>) -> Error<'a> {
    Error::new(
        import.region,
        ErrorKind::Import(ImportError::NameNotFound {
            module: import.module,
            name: "<interface>",
            available: &[],
        }),
    )
}

fn import_name<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    interface: &'a Interface<'a>,
    source: &'a str,
    binding: alder_ast::Name<'a>,
) -> Result<(), Error<'a>> {
    let mut found = false;
    for value in interface
        .values
        .iter()
        .filter(|value| value.exported_as == source)
    {
        found = true;
        import_value(env, binding.value, binding.region, *value)
            .map_err(|first| import_collision(binding, first))?;
    }
    for typ in interface
        .types
        .iter()
        .filter(|typ| typ.exported_as == source)
    {
        found = true;
        env.insert_foreign_type(
            binding.value,
            binding.region,
            typ.reference,
            typ.params.len(),
        )
        .map_err(|first| import_collision(binding, first))?;
    }
    for enum_ in interface
        .enums
        .iter()
        .filter(|enum_| enum_.exported_as == source)
    {
        found = true;
        import_enum(bump, env, enum_, binding)?;
    }
    for trait_ in interface
        .traits
        .iter()
        .filter(|trait_| trait_.exported_as == source)
    {
        found = true;
        import_trait(bump, env, trait_, binding.value, binding.region)
            .map_err(|first| import_collision(binding, first))?;
    }
    for module in interface
        .modules
        .iter()
        .filter(|module| module.exported_as == source)
    {
        found = true;
        env.insert_module(binding.value, binding.region, module.module, None)
            .map_err(|first| import_collision(binding, first))?;
    }
    if found {
        return Ok(());
    }
    if interface
        .private_names
        .iter()
        .any(|private| private.name == source)
    {
        return Err(Error::new(
            binding.region,
            ErrorKind::Import(ImportError::Name(NameError::Private {
                owner: interface.home,
                namespace: alder_ast::Namespace::Value,
                name: source,
            })),
        ));
    }
    let available_names: Vec<_> = interface
        .values
        .iter()
        .map(|value| value.exported_as)
        .chain(interface.types.iter().map(|typ| typ.exported_as))
        .chain(interface.enums.iter().map(|enum_| enum_.exported_as))
        .chain(interface.traits.iter().map(|trait_| trait_.exported_as))
        .chain(interface.modules.iter().map(|module| module.exported_as))
        .collect();
    let available = bump.alloc_slice_copy(&available_names);
    Err(Error::new(
        binding.region,
        ErrorKind::Import(ImportError::NameNotFound {
            module: interface.home,
            name: source,
            available,
        }),
    ))
}

fn import_all<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    interface: &'a Interface<'a>,
    region: Region,
    errors: &mut Vec<Error<'a>>,
) {
    for value in interface.values {
        if let Err(first) = import_value(env, value.exported_as, region, *value) {
            errors.push(import_collision(
                Located::at(region, value.exported_as),
                first,
            ));
        }
    }
    for typ in interface.types {
        if let Err(first) =
            env.insert_foreign_type(typ.exported_as, region, typ.reference, typ.params.len())
        {
            errors.push(import_collision(
                Located::at(region, typ.exported_as),
                first,
            ));
        }
    }
    for enum_ in interface.enums {
        if let Err(error) = import_enum(bump, env, enum_, Located::at(region, enum_.exported_as)) {
            errors.push(error);
        }
    }
    for trait_ in interface.traits {
        if let Err(first) = import_trait(bump, env, trait_, trait_.exported_as, region) {
            errors.push(import_collision(
                Located::at(region, trait_.exported_as),
                first,
            ));
        }
    }
    for module in interface.modules {
        if let Err(first) = env.insert_module(module.exported_as, region, module.module, None) {
            errors.push(import_collision(
                Located::at(region, module.exported_as),
                first,
            ));
        }
    }
}

fn import_value<'a>(
    env: &mut Env<'a>,
    name: &'a str,
    region: Region,
    value: alder_ast::InterfaceValue<'a>,
) -> Result<(), Region> {
    match value.identity {
        alder_ast::InterfaceValueIdentity::Binding(reference) => {
            env.insert_foreign_value(name, region, reference, value.annotation)
        }
        alder_ast::InterfaceValueIdentity::TraitMethod(method) => {
            env.insert_trait_method(MethodBinding {
                id: method,
                annotation: value.annotation,
                region,
                has_default: false,
            })
        }
    }
}

fn import_trait<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    trait_: &'a alder_ast::InterfaceTrait<'a>,
    name: &'a str,
    region: Region,
) -> Result<(), Region> {
    env.insert_foreign_trait(
        name,
        region,
        trait_.id.0,
        trait_.params.len(),
        bump.alloc_slice_fill_iter(
            trait_
                .associated_types
                .iter()
                .map(|associated| associated.id),
        ),
        bump.alloc_slice_fill_iter(trait_.methods.iter().map(|method| MethodBinding {
            id: method.id,
            annotation: method.scheme,
            region,
            has_default: method.has_default,
        })),
    )
}

fn import_enum<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    enum_: &'a alder_ast::InterfaceEnum<'a>,
    binding: alder_ast::Name<'a>,
) -> Result<(), Error<'a>> {
    env.insert_foreign_type(
        binding.value,
        binding.region,
        enum_.reference,
        enum_.params.len(),
    )
    .map_err(|first| import_collision(binding, first))?;
    let constructors =
        bump.alloc_slice_fill_iter(enum_.variants.iter().map(|variant| ConstructorRef {
            name: variant.name,
            index: variant.index,
            alternatives: variant.alternatives,
            payload: variant.payload,
            annotation: interface_constructor_annotation(bump, enum_, *variant),
        }));
    env.register_enum_as(binding.value, enum_.reference, constructors);
    Ok(())
}

fn import_collision<'a>(binding: alder_ast::Name<'a>, first: Region) -> Error<'a> {
    Error::new(
        binding.region,
        ErrorKind::Import(ImportError::AliasCollision {
            name: binding.value,
            first,
        }),
    )
}

fn predeclare<'a>(env: &mut Env<'a>, source: &SourceModule<'a>) -> Vec<Error<'a>> {
    let mut errors = Vec::new();
    for item in source.items {
        match item.value.kind {
            SourceItemKind::Fn(decl) => {
                insert_value(env, decl.name.value, decl.name.region, false, &mut errors);
            }
            SourceItemKind::Let(decl) => {
                let mut names = Vec::new();
                collect_pattern_names(decl.pattern, &mut names);
                for name in names {
                    insert_value(
                        env,
                        name.value,
                        name.region,
                        decl.mutable.is_some(),
                        &mut errors,
                    );
                }
            }
            SourceItemKind::TypeAlias(decl) => insert_type(
                env,
                decl.name.value,
                decl.name.region,
                decl.params.len(),
                &mut errors,
            ),
            SourceItemKind::OpaqueType(name) => {
                insert_type(env, name.value, name.region, 0, &mut errors)
            }
            SourceItemKind::Enum(decl) => insert_type(
                env,
                decl.name.value,
                decl.name.region,
                decl.params.len(),
                &mut errors,
            ),
            SourceItemKind::Trait(decl) => {
                if let Err(first) =
                    env.insert_trait(decl.name.value, decl.name.region, decl.params.len())
                {
                    errors.push(duplicate(
                        decl.name.value,
                        decl.name.region,
                        first,
                        alder_ast::Namespace::Trait,
                    ));
                }
            }
            SourceItemKind::Error(decl) => {
                insert_type(env, decl.name.value, decl.name.region, 0, &mut errors)
            }
            SourceItemKind::Component(decl) => {
                insert_value(env, decl.name.value, decl.name.region, false, &mut errors)
            }
            SourceItemKind::Table(decl) => {
                insert_type(env, decl.name.value, decl.name.region, 0, &mut errors);
            }
            SourceItemKind::Schema(decl) => {
                insert_type(env, decl.name.value, decl.name.region, 0, &mut errors);
            }
            SourceItemKind::Impl(_)
            | SourceItemKind::Macro(_)
            | SourceItemKind::Comptime(_)
            | SourceItemKind::Test(_)
            | SourceItemKind::Tests(_)
            | SourceItemKind::Import(_) => {}
        }
    }
    errors
}

fn predeclare_trait_members<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &SourceModule<'a>,
) -> Vec<Error<'a>> {
    let mut errors = Vec::new();
    for item in source.items {
        let SourceItemKind::Trait(declaration) = item.value.kind else {
            continue;
        };
        let trait_binding =
            match env.find_trait(bump, declaration.name.region, None, declaration.name.value) {
                Ok(binding) => binding,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            };
        let trait_id = alder_ast::TraitId(trait_binding.reference);
        let mut seen_parameters = BTreeMap::new();
        for parameter in declaration.params {
            if let Some(first) = seen_parameters.insert(parameter.value, parameter.region) {
                errors.push(Error::new(
                    parameter.region,
                    ErrorKind::Type(TypeError::DuplicateParameter {
                        name: parameter.value,
                        first,
                    }),
                ));
            }
        }
        let mut associated_types = Vec::new();
        let mut methods = Vec::new();
        let mut associated_names = BTreeMap::new();
        let mut method_names = BTreeMap::new();
        let trait_variables: BTreeSet<_> =
            declaration.params.iter().map(|param| param.value).collect();
        let mut trait_arities = BTreeMap::new();
        for parameter in declaration.params {
            trait_arities.insert(parameter.value, 0);
        }
        for member in declaration.items {
            if let alder_source::TraitItem::Fn(function) = member {
                collect_signature_variable_arities(function, &mut trait_arities);
            }
        }

        for member in declaration.items {
            let alder_source::TraitItem::AssocType(name) = member else {
                continue;
            };
            if let Some(first) = associated_names.insert(name.value, name.region) {
                errors.push(duplicate(
                    name.value,
                    name.region,
                    first,
                    alder_ast::Namespace::AssociatedItem,
                ));
                continue;
            }
            associated_types.push(alder_ast::AssocTypeId {
                trait_: trait_id,
                index: associated_types.len() as u16,
                name: name.value,
            });
        }
        let associated_types = bump.alloc_slice_copy(&associated_types);
        env.register_trait_members(declaration.name.value, associated_types, &[]);
        let trait_args = bump.alloc_slice_fill_iter(declaration.params.iter().map(|parameter| {
            bump.alloc(Located::at(
                parameter.region,
                Type::Var {
                    name: parameter.value,
                    args: &[],
                },
            )) as &Located<Type<'a>>
        }));
        let trait_ref = alder_ast::TraitRef {
            trait_: trait_id,
            args: trait_args,
        };
        env.push_associated_types(
            associated_types
                .iter()
                .map(|associated| {
                    (
                        associated.name,
                        alder_ast::ProjectionType {
                            trait_ref,
                            assoc: *associated,
                        },
                    )
                })
                .collect(),
        );

        for member in declaration.items {
            match member {
                alder_source::TraitItem::AssocType(_) => {}
                alder_source::TraitItem::Fn(function) => {
                    if let Some(first) =
                        method_names.insert(function.name.value, function.name.region)
                    {
                        errors.push(duplicate(
                            function.name.value,
                            function.name.region,
                            first,
                            alder_ast::Namespace::Value,
                        ));
                        continue;
                    }
                    let id = alder_ast::MethodId {
                        trait_: trait_id,
                        index: methods.len() as u16,
                        name: function.name.value,
                    };
                    match trait_method_annotation(
                        bump,
                        env,
                        function,
                        &trait_variables,
                        &trait_arities,
                    ) {
                        Ok(annotation) => methods.push(MethodBinding {
                            id,
                            annotation,
                            region: function.name.region,
                            has_default: function.body.is_some(),
                        }),
                        Err(mut method_errors) => errors.append(&mut method_errors),
                    }
                }
            }
        }

        env.pop_associated_types();
        let methods = bump.alloc_slice_copy(&methods);
        env.register_trait_members(declaration.name.value, associated_types, methods);
        for method in methods.iter() {
            if let Err(first) = env.insert_trait_method(*method) {
                errors.push(duplicate(
                    method.id.name,
                    method.region,
                    first,
                    alder_ast::Namespace::Value,
                ));
            }
        }
    }
    errors
}

fn trait_method_annotation<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    source: &'a alder_source::FnDecl<'a>,
    trait_variables: &BTreeSet<&'a str>,
    trait_arities: &BTreeMap<&'a str, usize>,
) -> Result<&'a Annotation<'a>, Vec<Error<'a>>> {
    let mut variables = trait_variables.clone();
    variables.extend(signature_variables(source));
    let mut arities = trait_arities.clone();
    collect_signature_variable_arities(source, &mut arities);
    let mut params = Vec::with_capacity(source.params.len());
    for param in source.params {
        let Some(annotation) = param.annotation else {
            return Err(vec![Error::new(
                param.pattern.region,
                ErrorKind::Type(TypeError::MissingAnnotation {
                    name: source.name.value,
                    position: "parameter",
                }),
            )]);
        };
        params.push(canonicalize_type(bump, env, &variables, annotation)?);
    }
    let Some(ret) = source.ret else {
        return Err(vec![Error::new(
            source.name.region,
            ErrorKind::Type(TypeError::MissingAnnotation {
                name: source.name.value,
                position: "return type",
            }),
        )]);
    };
    let ret = canonicalize_type(bump, env, &variables, ret)?;
    let constraints = canonicalize_constraints(bump, env, source.where_clause, &variables)?;
    let mut predicates = Vec::new();
    for constraint in constraints {
        if let TypeConstraint::Bound { var, traits } = constraint {
            for trait_ in *traits {
                let argument = bump.alloc(Located::at(
                    var.region,
                    Type::Var {
                        name: var.value,
                        args: &[],
                    },
                ));
                predicates.push(alder_ast::TraitRef {
                    trait_: alder_ast::TraitId(*trait_),
                    args: bump.alloc_slice_copy(&[argument as &Located<Type<'a>>]),
                });
            }
        }
    }
    let typ = bump.alloc(Located::at(
        source.name.region,
        Type::Fn {
            params: bump.alloc_slice_copy(&params),
            ret,
        },
    ));
    let type_params =
        bump.alloc_slice_fill_iter(variables.into_iter().map(|name| alder_ast::TypeParam {
            name: Located::at(source.name.region, name),
            kind: kind_from_arity(bump, arities.get(name).copied().unwrap_or(0)),
        }));
    Ok(bump.alloc(Annotation {
        params: type_params,
        trait_predicates: bump.alloc_slice_copy(&predicates),
        projection_equalities: projection_equalities_from_constraints(bump, constraints),
        typ,
    }))
}

fn insert_value<'a>(
    env: &mut Env<'a>,
    name: &'a str,
    region: Region,
    mutable: bool,
    errors: &mut Vec<Error<'a>>,
) {
    if let Err(first) = env.insert_top_level(name, region, mutable) {
        errors.push(duplicate(name, region, first, alder_ast::Namespace::Value));
    }
}

fn insert_type<'a>(
    env: &mut Env<'a>,
    name: &'a str,
    region: Region,
    arity: usize,
    errors: &mut Vec<Error<'a>>,
) {
    if let Err(first) = env.insert_type(name, region, arity) {
        errors.push(duplicate(name, region, first, alder_ast::Namespace::Type));
    }
}

fn duplicate<'a>(
    name: &'a str,
    region: Region,
    first: Region,
    namespace: alder_ast::Namespace,
) -> Error<'a> {
    Error::new(
        region,
        ErrorKind::Item(ItemError::DuplicateDefinition {
            namespace,
            name,
            first,
        }),
    )
}

fn canonicalize_enums<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &SourceModule<'a>,
    errors: &mut Vec<Error<'a>>,
) -> BTreeMap<&'a str, &'a EnumDecl<'a>> {
    let mut enums = BTreeMap::new();
    for item in source.items {
        let SourceItemKind::Enum(source_enum) = item.value.kind else {
            continue;
        };
        match canonicalize_enum(bump, env, source_enum) {
            Ok(enum_) => {
                let constructors =
                    bump.alloc_slice_fill_iter(enum_.variants.iter().map(|variant| {
                        let annotation = constructor_annotation(bump, enum_, *variant);
                        ConstructorRef {
                            name: variant.name,
                            index: variant.index,
                            alternatives: variant.alternatives,
                            payload: variant.payload,
                            annotation,
                        }
                    }));
                env.register_enum(enum_.name, constructors);
                enums.insert(source_enum.name.value, enum_);
            }
            Err(mut enum_errors) => errors.append(&mut enum_errors),
        }
    }
    enums
}

fn canonicalize_enum<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    source: &'a alder_source::EnumDecl<'a>,
) -> Result<&'a EnumDecl<'a>, Vec<Error<'a>>> {
    let variables: BTreeSet<_> = source.params.iter().map(|param| param.value).collect();
    let binding = env
        .find_type(bump, source.name.region, None, source.name.value)
        .map_err(|error| vec![error])?;
    let alternatives = source.variants.len() as u16;
    let mut variants = Vec::with_capacity(source.variants.len());
    let mut seen = BTreeMap::new();
    for (index, variant) in source.variants.iter().enumerate() {
        if let Some(first) = seen.insert(variant.name.value, variant.name.region) {
            return Err(vec![duplicate(
                variant.name.value,
                variant.name.region,
                first,
                alder_ast::Namespace::Constructor,
            )]);
        }
        let payload = match variant.payload {
            alder_source::VariantPayload::Unit => VariantPayload::Unit,
            alder_source::VariantPayload::Tuple(types) => {
                let mut canonical = Vec::with_capacity(types.len());
                for typ in types {
                    canonical.push(canonicalize_type(bump, env, &variables, typ)?);
                }
                VariantPayload::Tuple(bump.alloc_slice_copy(&canonical))
            }
            alder_source::VariantPayload::Record(fields) => {
                let mut canonical = Vec::with_capacity(fields.len());
                for (field_index, field) in fields.iter().enumerate() {
                    canonical.push(alder_ast::RecordTypeField {
                        index: field_index as u16,
                        name: field.field.value,
                        presence: alder_ast::FieldPresence::Required,
                        typ: canonicalize_type(bump, env, &variables, field.typ)?,
                    });
                }
                VariantPayload::Record(bump.alloc_slice_copy(&canonical))
            }
        };
        variants.push(Variant {
            name: ConstructorName {
                enum_: binding.reference,
                variant: variant.name.value,
            },
            index: index as u16,
            alternatives,
            payload,
        });
    }
    Ok(bump.alloc(EnumDecl {
        name: binding.reference,
        params: source.params,
        variants: bump.alloc_slice_copy(&variants),
    }))
}

fn constructor_annotation<'a>(
    bump: &'a Bump,
    enum_: &'a EnumDecl<'a>,
    variant: Variant<'a>,
) -> &'a Annotation<'a> {
    let args = bump.alloc_slice_fill_iter(enum_.params.iter().map(|param| {
        bump.alloc(Located::at(
            param.region,
            Type::Var {
                name: param.value,
                args: &[],
            },
        )) as &Located<Type<'a>>
    }));
    let result = bump.alloc(Located::at(
        Region::zero(),
        Type::Named {
            reference: enum_.name,
            args,
        },
    ));
    let params = match variant.payload {
        VariantPayload::Unit => &[] as &[&Located<Type<'a>>],
        VariantPayload::Tuple(types) => types,
        VariantPayload::Record(fields) => {
            bump.alloc_slice_fill_iter(fields.iter().map(|field| field.typ))
        }
    };
    let typ = if params.is_empty() {
        result
    } else {
        bump.alloc(Located::at(
            Region::zero(),
            Type::Fn {
                params,
                ret: result,
            },
        ))
    };
    bump.alloc(Annotation {
        params: bump.alloc_slice_fill_iter(enum_.params.iter().map(|param| alder_ast::TypeParam {
            name: *param,
            kind: alder_ast::Kind::Type,
        })),
        trait_predicates: &[],
        projection_equalities: &[],
        typ,
    })
}

fn interface_constructor_annotation<'a>(
    bump: &'a Bump,
    enum_: &'a alder_ast::InterfaceEnum<'a>,
    variant: Variant<'a>,
) -> &'a Annotation<'a> {
    let args = bump.alloc_slice_fill_iter(enum_.params.iter().map(|param| {
        bump.alloc(Located::at(
            Region::zero(),
            Type::Var {
                name: param.name.value,
                args: &[],
            },
        )) as &Located<Type<'a>>
    }));
    let result = bump.alloc(Located::at(
        Region::zero(),
        Type::Named {
            reference: enum_.reference,
            args,
        },
    ));
    let params = match variant.payload {
        VariantPayload::Unit => &[] as &[&Located<Type<'a>>],
        VariantPayload::Tuple(types) => types,
        VariantPayload::Record(fields) => {
            bump.alloc_slice_fill_iter(fields.iter().map(|field| field.typ))
        }
    };
    let typ = if params.is_empty() {
        result
    } else {
        bump.alloc(Located::at(
            Region::zero(),
            Type::Fn {
                params,
                ret: result,
            },
        ))
    };
    bump.alloc(Annotation {
        params: enum_.params,
        trait_predicates: &[],
        projection_equalities: &[],
        typ,
    })
}

fn canonicalize_item<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    item: &'a Located<SourceItem<'a>>,
    enums: &BTreeMap<&'a str, &'a EnumDecl<'a>>,
    item_ordinal: u32,
) -> Result<&'a Located<Item<'a>>, Vec<Error<'a>>> {
    let visibility = match item.value.visibility {
        alder_source::Visibility::Private => Visibility::Private,
        alder_source::Visibility::Pub(region) => Visibility::Public(region),
    };
    let attributes = canonicalize_attributes(bump, item.value.attributes)?;
    let kind = match item.value.kind {
        SourceItemKind::Fn(decl) if decl.body.is_some() => {
            ItemKind::Fn(canonicalize_fn(bump, env, decl)?)
        }
        SourceItemKind::Fn(decl) => {
            let (module, symbol) = extern_strings(item.value.attributes)?;
            let variables = signature_variables(decl);
            let Some(ret) = decl.ret else {
                return Err(vec![invalid_extern(
                    decl.name.region,
                    "extern functions require an explicit return type",
                )]);
            };
            env.push_scope();
            let params = canonicalize_params(bump, env, decl.params, &variables)?;
            env.pop_scope();
            let constraints = canonicalize_constraints(bump, env, decl.where_clause, &variables)?;
            ItemKind::Extern(bump.alloc(ExternDecl::Fn {
                module,
                symbol,
                name: top_level_name(env, decl.name.value),
                params,
                ret: canonicalize_type(bump, env, &variables, ret)?,
                constraints,
            }))
        }
        SourceItemKind::Let(decl) => {
            let value = canonicalize_expr(bump, env, decl.value)?;
            let annotation = match decl.annotation {
                Some(typ) => {
                    let vars = type_variables(typ);
                    Some(canonicalize_type(bump, env, &vars, typ)?)
                }
                None => None,
            };
            let pattern = canonicalize_pattern(bump, env, decl.pattern, BindingMode::TopLevel)?;
            let mut bindings = Vec::new();
            let mut source_names = Vec::new();
            collect_pattern_names(decl.pattern, &mut source_names);
            for name in source_names {
                bindings.push(top_level_name(env, name.value));
            }
            ItemKind::Let(bump.alloc(TopLevelLet {
                bindings: bump.alloc_slice_copy(&bindings),
                mutable: decl.mutable.is_some(),
                pattern,
                annotation,
                value,
            }))
        }
        SourceItemKind::TypeAlias(decl) => {
            let variables: BTreeSet<_> = decl.params.iter().map(|param| param.value).collect();
            ItemKind::TypeAlias(bump.alloc(TypeAlias {
                name: top_level_type_name(env, decl.name.value),
                params: decl.params,
                typ: canonicalize_type(bump, env, &variables, decl.typ)?,
            }))
        }
        SourceItemKind::OpaqueType(name) => {
            if !item
                .value
                .attributes
                .iter()
                .any(|attribute| attribute.value.name.value == "extern")
            {
                return Err(vec![invalid_extern(
                    name.region,
                    "a bodiless type requires #[extern]",
                )]);
            }
            ItemKind::Extern(bump.alloc(ExternDecl::Type {
                name: top_level_type_name(env, name.value),
            }))
        }
        SourceItemKind::Enum(decl) => ItemKind::Enum(enums[decl.name.value]),
        SourceItemKind::Trait(decl) => ItemKind::Trait(canonicalize_trait(bump, env, decl)?),
        SourceItemKind::Impl(decl) => ItemKind::Impl(canonicalize_impl(
            bump,
            env,
            decl,
            item_ordinal,
            item.region,
        )?),
        SourceItemKind::Error(decl) => {
            ItemKind::ErrorGroup(canonicalize_error_group(bump, env, decl)?)
        }
        SourceItemKind::Component(decl) => {
            ItemKind::Component(canonicalize_component(bump, env, decl)?)
        }
        SourceItemKind::Table(decl) => ItemKind::Table(canonicalize_table(bump, env, decl)?),
        SourceItemKind::Schema(decl) => ItemKind::Schema(canonicalize_schema(bump, env, decl)?),
        SourceItemKind::Macro(decl) => ItemKind::Macro(bump.alloc(MacroDecl {
            name: QualifiedName {
                module: env.home,
                name: decl.name.value,
            },
            params: decl.params,
            body: decl.body,
        })),
        SourceItemKind::Comptime(_) => {
            return Err(vec![Error::new(
                item.region,
                ErrorKind::Attribute(AttributeError::MacroUnavailable),
            )]);
        }
        SourceItemKind::Test(decl) => ItemKind::Test(bump.alloc(TestDecl {
            name: decl.name,
            body: canonicalize_block(bump, env, decl.body)?,
        })),
        SourceItemKind::Tests(items) => {
            let mut nested_env = env.clone();
            let nested_module = SourceModule {
                items,
                comments: &[],
            };
            let mut nested_errors = predeclare(&mut nested_env, &nested_module);
            let nested_enums =
                canonicalize_enums(bump, &mut nested_env, &nested_module, &mut nested_errors);
            if !nested_errors.is_empty() {
                return Err(nested_errors);
            }
            let mut canonical = Vec::with_capacity(items.len());
            for (nested_ordinal, nested) in items.iter().enumerate() {
                if matches!(nested.value.kind, SourceItemKind::Import(_)) {
                    continue;
                }
                canonical.push(canonicalize_item(
                    bump,
                    &mut nested_env,
                    nested,
                    &nested_enums,
                    nested_ordinal as u32,
                )?);
            }
            ItemKind::Tests(bump.alloc_slice_copy(&canonical))
        }
        SourceItemKind::Import(_) => unreachable!("imports are filtered before item conversion"),
    };
    if attributes
        .iter()
        .any(|attribute| matches!(attribute, Attribute::Derive { .. }))
        && !matches!(kind, ItemKind::Enum(_) | ItemKind::ErrorGroup(_))
    {
        return Err(vec![Error::new(
            item.region,
            ErrorKind::Attribute(AttributeError::InvalidDerive {
                reason: "built-in derives are only available on enums and error groups",
            }),
        )]);
    }
    Ok(bump.alloc(Located::at(
        item.region,
        Item {
            visibility,
            attributes,
            kind,
        },
    )))
}

fn canonicalize_trait<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a alder_source::TraitDecl<'a>,
) -> Result<&'a TraitDecl<'a>, Vec<Error<'a>>> {
    let variables: BTreeSet<_> = source.params.iter().map(|param| param.value).collect();
    let binding = env
        .find_trait(bump, source.name.region, None, source.name.value)
        .map_err(|error| vec![error])?;
    let name = binding.reference;
    let constraints = canonicalize_constraints(bump, env, source.where_clause, &variables)?;
    let superclasses = trait_refs_from_constraints(bump, constraints);
    let mut arities = BTreeMap::new();
    for parameter in source.params {
        arities.insert(parameter.value, 0);
    }
    for item in source.items {
        if let alder_source::TraitItem::Fn(function) = item {
            collect_signature_variable_arities(function, &mut arities);
        }
    }
    let type_params =
        bump.alloc_slice_fill_iter(source.params.iter().map(|parameter| alder_ast::TypeParam {
            name: *parameter,
            kind: kind_from_arity(bump, arities.get(parameter.value).copied().unwrap_or(0)),
        }));
    let associated_types = source
        .items
        .iter()
        .filter_map(|item| {
            let alder_source::TraitItem::AssocType(associated) = item else {
                return None;
            };
            let id = binding
                .associated_types
                .iter()
                .find(|candidate| candidate.name == associated.value)
                .copied()
                .expect("associated types were registered before canonicalization");
            Some(alder_ast::AssocTypeDecl {
                id,
                kind: alder_ast::Kind::Type,
                region: associated.region,
            })
        })
        .collect::<Vec<_>>();
    let associated_types = bump.alloc_slice_copy(&associated_types);
    let trait_args = bump.alloc_slice_fill_iter(source.params.iter().map(|parameter| {
        bump.alloc(Located::at(
            parameter.region,
            Type::Var {
                name: parameter.value,
                args: &[],
            },
        )) as &Located<Type<'a>>
    }));
    let trait_ref = alder_ast::TraitRef {
        trait_: alder_ast::TraitId(name),
        args: trait_args,
    };
    env.push_associated_types(
        binding
            .associated_types
            .iter()
            .map(|associated| {
                (
                    associated.name,
                    alder_ast::ProjectionType {
                        trait_ref,
                        assoc: *associated,
                    },
                )
            })
            .collect(),
    );
    let items = (|| {
        let mut items = Vec::with_capacity(source.items.len());
        for item in source.items {
            items.push(match item {
                alder_source::TraitItem::AssocType(name) => TraitItem::AssocType(*name),
                alder_source::TraitItem::Fn(function) => TraitItem::Fn(canonicalize_trait_fn(
                    bump, env, function, &variables, name.name,
                )?),
            });
        }
        Ok::<_, Vec<Error<'a>>>(bump.alloc_slice_copy(&items))
    })();
    env.pop_associated_types();
    let items = items?;
    Ok(bump.alloc(TraitDecl {
        id: alder_ast::TraitId(name),
        name,
        params: source.params,
        type_params,
        constraints,
        superclasses,
        associated_types,
        items,
    }))
}

fn trait_refs_from_constraints<'a>(
    bump: &'a Bump,
    constraints: &'a [TypeConstraint<'a>],
) -> &'a [alder_ast::TraitRef<'a>] {
    let mut predicates = Vec::new();
    for constraint in constraints {
        if let TypeConstraint::Bound { var, traits } = constraint {
            for trait_ in *traits {
                let argument = bump.alloc(Located::at(
                    var.region,
                    Type::Var {
                        name: var.value,
                        args: &[],
                    },
                ));
                predicates.push(alder_ast::TraitRef {
                    trait_: alder_ast::TraitId(*trait_),
                    args: bump.alloc_slice_copy(&[argument as &Located<Type<'a>>]),
                });
            }
        }
    }
    bump.alloc_slice_copy(&predicates)
}

fn projection_equalities_from_constraints<'a>(
    bump: &'a Bump,
    constraints: &'a [TypeConstraint<'a>],
) -> &'a [alder_ast::ProjectionEquality<'a>] {
    let equalities = constraints
        .iter()
        .filter_map(|constraint| match constraint {
            TypeConstraint::AssocEq {
                projection,
                typ,
                region,
            } => Some(alder_ast::ProjectionEquality {
                projection: *projection,
                typ,
                region: *region,
            }),
            TypeConstraint::Bound { .. } => None,
        })
        .collect::<Vec<_>>();
    bump.alloc_slice_copy(&equalities)
}

fn canonicalize_trait_fn<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a alder_source::FnDecl<'a>,
    trait_variables: &BTreeSet<&'a str>,
    trait_name: &'a str,
) -> Result<&'a TraitFn<'a>, Vec<Error<'a>>> {
    let mut variables = trait_variables.clone();
    variables.extend(signature_variables(source));
    env.push_scope();
    let saved_control = env.control;
    env.control.function_depth += 1;
    env.control.loop_depth = 0;
    let result = (|| {
        let method = env
            .find_trait_method(trait_name, source.name.value)
            .expect("trait methods were registered before body canonicalization");
        let params = canonicalize_params(bump, env, source.params, &variables)?;
        let ret = source
            .ret
            .map(|typ| canonicalize_type(bump, env, &variables, typ))
            .transpose()?;
        env.control.task_return = ret.is_some_and(is_task_type);
        let constraints = canonicalize_constraints(bump, env, source.where_clause, &variables)?;
        let body = source
            .body
            .map(|body| canonicalize_block(bump, env, body))
            .transpose()?;
        Ok(bump.alloc(TraitFn {
            id: method.id,
            name: source.name,
            params,
            ret,
            constraints,
            scheme: method.annotation,
            body,
        }) as &'a TraitFn<'a>)
    })();
    env.control = saved_control;
    env.pop_scope();
    result
}

fn canonicalize_impl<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a alder_source::ImplDecl<'a>,
    item_ordinal: u32,
    region: Region,
) -> Result<&'a ImplDecl<'a>, Vec<Error<'a>>> {
    let trait_name = source
        .trait_
        .segments
        .last()
        .expect("trait path is nonempty");
    let trait_binding = env
        .find_trait(
            bump,
            source.trait_.region(),
            (source.trait_.segments.len() > 1).then(|| source.trait_.segments[0].value),
            trait_name.value,
        )
        .map_err(|error| vec![error])?;
    if source.args.len() != trait_binding.arity {
        return Err(vec![Error::new(
            source.trait_.region(),
            ErrorKind::Type(TypeError::BadArity {
                name: trait_binding.reference.name,
                expected: trait_binding.arity,
                actual: source.args.len(),
            }),
        )]);
    }
    let trait_ = trait_binding.reference;
    let mut variables = BTreeSet::new();
    for arg in source.args {
        collect_type_variables(arg, &mut variables);
    }
    let mut args = Vec::with_capacity(source.args.len());
    for arg in source.args {
        args.push(canonicalize_impl_head_type(bump, env, &variables, arg)?);
    }
    let constraints = canonicalize_constraints(bump, env, source.where_clause, &variables)?;
    let trait_predicates = trait_refs_from_constraints(bump, constraints);
    let projection_equalities = projection_equalities_from_constraints(bump, constraints);
    let mut arities = BTreeMap::new();
    for variable in &variables {
        arities.insert(*variable, 0);
    }
    for argument in source.args {
        collect_type_variable_arities(argument, &mut arities);
    }
    let params = bump.alloc_slice_fill_iter(variables.iter().map(|name| alder_ast::TypeParam {
        name: Located::at(region, *name),
        kind: kind_from_arity(bump, arities.get(name).copied().unwrap_or(0)),
    }));
    let trait_ref = alder_ast::TraitRef {
        trait_: alder_ast::TraitId(trait_),
        args: bump.alloc_slice_copy(&args),
    };
    env.push_associated_types(
        trait_binding
            .associated_types
            .iter()
            .map(|associated| {
                (
                    associated.name,
                    alder_ast::ProjectionType {
                        trait_ref,
                        assoc: *associated,
                    },
                )
            })
            .collect(),
    );
    let members = (|| {
        let mut items = Vec::with_capacity(source.items.len());
        let mut assoc_bindings = Vec::new();
        let mut seen_associated = BTreeMap::new();
        let mut seen_methods = BTreeMap::new();
        for item in source.items {
            items.push(match item {
                alder_source::ImplItem::AssocType { name, typ } => {
                    if let Some(first) = seen_associated.insert(name.value, name.region) {
                        return Err(vec![duplicate(
                            name.value,
                            name.region,
                            first,
                            alder_ast::Namespace::AssociatedItem,
                        )]);
                    }
                    let Some(assoc) = trait_binding
                        .associated_types
                        .iter()
                        .find(|associated| associated.name == name.value)
                        .copied()
                    else {
                        return Err(vec![Error::new(
                            name.region,
                            ErrorKind::Type(TypeError::UnknownImplItem {
                                trait_name: trait_.name,
                                name: name.value,
                                item_kind: "associated type",
                            }),
                        )]);
                    };
                    let typ = canonicalize_type(bump, env, &variables, typ)?;
                    assoc_bindings.push(alder_ast::AssocBinding {
                        assoc,
                        typ,
                        region: name.region,
                    });
                    ImplItem::AssocType { name: *name, typ }
                }
                alder_source::ImplItem::Fn(function) => {
                    if let Some(first) =
                        seen_methods.insert(function.name.value, function.name.region)
                    {
                        return Err(vec![duplicate(
                            function.name.value,
                            function.name.region,
                            first,
                            alder_ast::Namespace::Value,
                        )]);
                    }
                    let Some(method) = trait_binding
                        .methods
                        .iter()
                        .find(|method| method.id.name == function.name.value)
                        .copied()
                    else {
                        return Err(vec![Error::new(
                            function.name.region,
                            ErrorKind::Type(TypeError::UnknownImplItem {
                                trait_name: trait_.name,
                                name: function.name.value,
                                item_kind: "method",
                            }),
                        )]);
                    };
                    ImplItem::Fn(canonicalize_impl_fn(
                        bump, env, function, &variables, method,
                    )?)
                }
            });
        }
        for associated in trait_binding.associated_types {
            if !seen_associated.contains_key(associated.name) {
                return Err(vec![Error::new(
                    region,
                    ErrorKind::Type(TypeError::MissingImplItem {
                        trait_name: trait_.name,
                        name: associated.name,
                        item_kind: "associated type",
                    }),
                )]);
            }
        }
        for method in trait_binding.methods {
            if !method.has_default && !seen_methods.contains_key(method.id.name) {
                return Err(vec![Error::new(
                    region,
                    ErrorKind::Type(TypeError::MissingImplItem {
                        trait_name: trait_.name,
                        name: method.id.name,
                        item_kind: "method",
                    }),
                )]);
            }
        }
        Ok::<_, Vec<Error<'a>>>((items, assoc_bindings))
    })();
    env.pop_associated_types();
    let (items, assoc_bindings) = members?;
    let args = bump.alloc_slice_copy(&args);
    Ok(bump.alloc(ImplDecl {
        id: alder_ast::ImplId {
            module: env.home,
            origin: alder_ast::ImplOrigin::Source { item_ordinal },
        },
        trait_,
        args,
        trait_ref: alder_ast::TraitRef {
            trait_: trait_ref.trait_,
            args,
        },
        params,
        constraints,
        trait_predicates,
        projection_equalities,
        assoc_bindings: bump.alloc_slice_copy(&assoc_bindings),
        items: bump.alloc_slice_copy(&items),
        synthetic: None,
        region,
    }))
}

fn canonicalize_impl_fn<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a alder_source::FnDecl<'a>,
    impl_variables: &BTreeSet<&'a str>,
    method: MethodBinding<'a>,
) -> Result<&'a ImplFn<'a>, Vec<Error<'a>>> {
    let mut variables = impl_variables.clone();
    variables.extend(signature_variables(source));
    env.push_scope();
    let saved_control = env.control;
    env.control.function_depth += 1;
    env.control.loop_depth = 0;
    env.control.task_return = false;
    let result = (|| {
        let params = canonicalize_params(bump, env, source.params, &variables)?;
        let ret = source
            .ret
            .map(|typ| canonicalize_type(bump, env, &variables, typ))
            .transpose()?;
        env.control.task_return = ret.is_some_and(is_task_type);
        let constraints = canonicalize_constraints(bump, env, source.where_clause, &variables)?;
        let Some(body) = source.body else {
            return Err(vec![Error::new(
                source.name.region,
                ErrorKind::Attribute(AttributeError::InvalidExtern {
                    reason: "implementation methods require a body",
                }),
            )]);
        };
        let body = canonicalize_block(bump, env, body)?;
        Ok(bump.alloc(ImplFn {
            method: method.id,
            name: source.name,
            params,
            ret,
            constraints,
            scheme: method.annotation,
            body,
        }) as &'a ImplFn<'a>)
    })();
    env.control = saved_control;
    env.pop_scope();
    result
}

fn canonicalize_constraints<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    source: &'a [alder_source::Constraint<'a>],
    variables: &BTreeSet<&'a str>,
) -> Result<&'a [TypeConstraint<'a>], Vec<Error<'a>>> {
    let mut constraints = Vec::with_capacity(source.len());
    let mut resolved_bounds: BTreeMap<&'a str, Vec<alder_ast::QualifiedName<'a>>> = BTreeMap::new();
    for constraint in source {
        if let alder_source::Constraint::Bound { var, bounds } = constraint {
            if !variables.contains(var.value) {
                return Err(vec![Error::new(
                    var.region,
                    ErrorKind::Type(TypeError::UnboundVariable { name: var.value }),
                )]);
            }
            for path in *bounds {
                let name = path.segments.last().expect("trait path is nonempty");
                let binding = env
                    .find_trait(
                        bump,
                        path.region(),
                        (path.segments.len() > 1).then(|| path.segments[0].value),
                        name.value,
                    )
                    .map_err(|error| vec![error])?;
                if binding.arity != 1 {
                    return Err(vec![Error::new(
                        path.region(),
                        ErrorKind::Type(TypeError::BadArity {
                            name: binding.reference.name,
                            expected: binding.arity,
                            actual: 1,
                        }),
                    )]);
                }
                resolved_bounds
                    .entry(var.value)
                    .or_default()
                    .push(binding.reference);
            }
        }
    }
    for constraint in source {
        constraints.push(match constraint {
            alder_source::Constraint::Bound { var, bounds } => {
                let traits = resolved_bounds
                    .get(var.value)
                    .expect("bounds were resolved in the first pass");
                debug_assert_eq!(traits.len(), bounds.len());
                TypeConstraint::Bound {
                    var: *var,
                    traits: bump.alloc_slice_copy(traits),
                }
            }
            alder_source::Constraint::AssocEq { var, assoc, typ } => {
                if !variables.contains(var.value) {
                    return Err(vec![Error::new(
                        var.region,
                        ErrorKind::Type(TypeError::UnboundVariable { name: var.value }),
                    )]);
                }
                let mut candidates = resolved_bounds
                    .get(var.value)
                    .into_iter()
                    .flatten()
                    .filter_map(|trait_| {
                        env.associated_type(*trait_, assoc.value)
                            .map(|associated| (*trait_, associated))
                    })
                    .collect::<Vec<_>>();
                if candidates.is_empty()
                    && let Some(projection) = env.find_associated_type(assoc.value)
                    && matches!(
                        projection.trait_ref.args,
                        [argument]
                            if matches!(argument.value, Type::Var { name, args: [] } if name == var.value)
                    )
                {
                    candidates.push((projection.trait_ref.trait_.0, projection.assoc));
                }
                let [(trait_, associated)] = candidates.as_slice() else {
                    let kind = if candidates.is_empty() {
                        TypeError::UnknownAssocType { name: assoc.value }
                    } else {
                        TypeError::AmbiguousAssocType {
                            name: assoc.value,
                            traits: bump.alloc_slice_fill_iter(
                                candidates.iter().map(|(trait_, _)| alder_ast::TraitId(*trait_)),
                            ),
                        }
                    };
                    return Err(vec![Error::new(assoc.region, ErrorKind::Type(kind))]);
                };
                let argument = bump.alloc(Located::at(
                    var.region,
                    Type::Var {
                        name: var.value,
                        args: &[],
                    },
                ));
                TypeConstraint::AssocEq {
                    projection: alder_ast::ProjectionType {
                        trait_ref: alder_ast::TraitRef {
                            trait_: alder_ast::TraitId(*trait_),
                            args: bump.alloc_slice_copy(&[argument as &Located<Type<'a>>]),
                        },
                        assoc: *associated,
                    },
                    typ: canonicalize_type(bump, env, variables, typ)?,
                    region: Region::span_across(&var.region, &typ.region),
                }
            }
        });
    }
    Ok(bump.alloc_slice_copy(&constraints))
}

fn canonicalize_error_group<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    source: &'a alder_source::ErrorDecl<'a>,
) -> Result<&'a ErrorGroup<'a>, Vec<Error<'a>>> {
    let name = top_level_type_name(env, source.name.value);
    let mut tags = Vec::with_capacity(source.tags.len());
    let mut seen = BTreeMap::new();
    for (index, tag) in source.tags.iter().enumerate() {
        if let Some(first) = seen.insert(tag.name.value, tag.name.region) {
            return Err(vec![duplicate(
                tag.name.value,
                tag.name.region,
                first,
                alder_ast::Namespace::Constructor,
            )]);
        }
        let mut args = Vec::with_capacity(tag.args.len());
        for arg in tag.args {
            args.push(canonicalize_type(bump, env, &BTreeSet::new(), arg)?);
        }
        tags.push(ErrorTagType {
            index: index as u16,
            name: tag.name.value,
            args: bump.alloc_slice_copy(&args),
        });
    }
    Ok(bump.alloc(ErrorGroup {
        name,
        tags: bump.alloc_slice_copy(&tags),
    }))
}

fn canonicalize_component<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a alder_source::ComponentDecl<'a>,
) -> Result<&'a ComponentDecl<'a>, Vec<Error<'a>>> {
    let mut variables = BTreeSet::new();
    for param in source.params {
        if let Some(annotation) = param.annotation {
            collect_type_variables(annotation, &mut variables);
        }
    }
    env.push_scope();
    let saved_control = env.control;
    env.control.function_depth += 1;
    env.control.loop_depth = 0;
    env.control.task_return = false;
    let result = (|| {
        let params = canonicalize_params(bump, env, source.params, &variables)?;
        let body = canonicalize_block(bump, env, source.body)?;
        Ok(bump.alloc(ComponentDecl {
            name: top_level_name(env, source.name.value),
            params,
            body,
        }) as &'a ComponentDecl<'a>)
    })();
    env.control = saved_control;
    env.pop_scope();
    result
}

fn canonicalize_table<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a alder_source::TableDecl<'a>,
) -> Result<&'a TableDecl<'a>, Vec<Error<'a>>> {
    let mut columns = Vec::with_capacity(source.columns.len());
    for column in source.columns {
        env.control.opaque_names_depth += 1;
        let builder = canonicalize_expr(bump, env, column.builder);
        env.control.opaque_names_depth -= 1;
        columns.push(TableColumn {
            name: column.name,
            builder: builder?,
            modifiers: canonicalize_modifiers(bump, env, column.modifiers)?,
        });
    }
    Ok(bump.alloc(TableDecl {
        name: top_level_type_name(env, source.name.value),
        columns: bump.alloc_slice_copy(&columns),
    }))
}

fn canonicalize_schema<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a alder_source::SchemaDecl<'a>,
) -> Result<&'a SchemaDecl<'a>, Vec<Error<'a>>> {
    let from = source
        .from
        .map(|name| {
            env.find_type(bump, name.region, None, name.value)
                .map(|binding| binding.reference)
                .map_err(|error| vec![error])
        })
        .transpose()?;
    let mut items = Vec::with_capacity(source.items.len());
    for item in source.items {
        items.push(match item {
            alder_source::SchemaItem::Pick(names) => SchemaItem::Pick(names),
            alder_source::SchemaItem::Field { name, typ, rules } => SchemaItem::Field {
                name: *name,
                typ: typ
                    .map(|typ| canonicalize_type(bump, env, &type_variables(typ), typ))
                    .transpose()?,
                rules: canonicalize_modifiers(bump, env, rules)?,
            },
        });
    }
    Ok(bump.alloc(SchemaDecl {
        name: top_level_type_name(env, source.name.value),
        from,
        items: bump.alloc_slice_copy(&items),
    }))
}

fn canonicalize_modifiers<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a [alder_source::Modifier<'a>],
) -> Result<&'a [Modifier<'a>], Vec<Error<'a>>> {
    let mut modifiers = Vec::with_capacity(source.len());
    for modifier in source {
        let mut args = Vec::with_capacity(modifier.args.len());
        env.control.opaque_names_depth += 1;
        for arg in modifier.args {
            let result = canonicalize_expr(bump, env, arg);
            if let Err(errors) = result {
                env.control.opaque_names_depth -= 1;
                return Err(errors);
            }
            args.push(result.expect("checked above"));
        }
        env.control.opaque_names_depth -= 1;
        modifiers.push(Modifier {
            name: modifier.name,
            args: bump.alloc_slice_copy(&args),
        });
    }
    Ok(bump.alloc_slice_copy(&modifiers))
}

fn canonicalize_fn<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a alder_source::FnDecl<'a>,
) -> Result<&'a FnDecl<'a>, Vec<Error<'a>>> {
    let variables = signature_variables(source);
    env.push_scope();
    let saved_control = env.control;
    env.control.function_depth += 1;
    env.control.loop_depth = 0;
    let params = canonicalize_params(bump, env, source.params, &variables)?;
    let ret = match source.ret {
        Some(ret) => Some(canonicalize_type(bump, env, &variables, ret)?),
        None => None,
    };
    let constraints = canonicalize_constraints(bump, env, source.where_clause, &variables)?;
    env.control.task_return = ret.is_some_and(is_task_type);
    let body = canonicalize_block(bump, env, source.body.expect("body checked by caller"))?;
    env.control = saved_control;
    env.pop_scope();
    Ok(bump.alloc(FnDecl {
        name: top_level_name(env, source.name.value),
        params,
        ret,
        constraints,
        body,
    }))
}

fn canonicalize_params<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a [alder_source::Param<'a>],
    variables: &BTreeSet<&'a str>,
) -> Result<&'a [Param<'a>], Vec<Error<'a>>> {
    let mut params = Vec::with_capacity(source.len());
    for param in source {
        let annotation = match param.annotation {
            Some(typ) => Some(canonicalize_type(bump, env, variables, typ)?),
            None => None,
        };
        let pattern = canonicalize_pattern(
            bump,
            env,
            param.pattern,
            BindingMode::Local {
                mutable: param.mutable.is_some(),
            },
        )?;
        params.push(Param {
            mutable: param.mutable.is_some(),
            pattern,
            annotation,
        });
    }
    Ok(bump.alloc_slice_copy(&params))
}

fn canonicalize_attributes<'a>(
    bump: &'a Bump,
    source: &'a [Located<alder_source::Attribute<'a>>],
) -> Result<&'a [Attribute<'a>], Vec<Error<'a>>> {
    let mut attributes = Vec::with_capacity(source.len());
    for attribute in source {
        match attribute.value.name.value {
            "extern" => {
                let strings: Vec<_> = attribute
                    .value
                    .args
                    .iter()
                    .filter_map(|arg| match arg.value {
                        alder_source::Expr::Str(value) => Some(value),
                        _ => None,
                    })
                    .collect();
                if strings.len() != attribute.value.args.len() || !matches!(strings.len(), 0 | 2) {
                    return Err(vec![invalid_extern(
                        attribute.value.name.region,
                        "#[extern] takes either no arguments or two string arguments",
                    )]);
                }
                attributes.push(Attribute::Extern {
                    module: strings.first().copied(),
                    symbol: strings.get(1).copied(),
                });
            }
            "derive" => {
                let mut names = Vec::with_capacity(attribute.value.args.len());
                let mut seen = BTreeMap::new();
                for argument in attribute.value.args {
                    let alder_source::Expr::Path(path) = argument.value else {
                        return Err(vec![Error::new(
                            argument.region,
                            ErrorKind::Attribute(AttributeError::InvalidDerive {
                                reason: "derive arguments must be trait names",
                            }),
                        )]);
                    };
                    let [name] = path.segments else {
                        return Err(vec![Error::new(
                            argument.region,
                            ErrorKind::Attribute(AttributeError::InvalidDerive {
                                reason: "derive names cannot be qualified",
                            }),
                        )]);
                    };
                    if !matches!(name.value, "Show" | "Eq" | "Ord" | "Hash" | "Json") {
                        return Err(vec![Error::new(
                            name.region,
                            ErrorKind::Attribute(AttributeError::InvalidDerive {
                                reason: "unknown built-in derive",
                            }),
                        )]);
                    }
                    if seen.insert(name.value, name.region).is_some() {
                        return Err(vec![Error::new(
                            name.region,
                            ErrorKind::Attribute(AttributeError::InvalidDerive {
                                reason: "a derive may only be listed once",
                            }),
                        )]);
                    }
                    names.push(QualifiedName {
                        module: ModuleId {
                            package: alder_ast::PackageId::Builtin,
                            path: &[],
                        },
                        name: name.value,
                    });
                }
                if names.is_empty() {
                    return Err(vec![Error::new(
                        attribute.region,
                        ErrorKind::Attribute(AttributeError::InvalidDerive {
                            reason: "derive requires at least one trait name",
                        }),
                    )]);
                }
                attributes.push(Attribute::Derive {
                    region: attribute.region,
                    names: bump.alloc_slice_copy(&names),
                });
            }
            _ => attributes.push(Attribute::Other {
                name: attribute.value.name,
            }),
        }
    }
    Ok(bump.alloc_slice_copy(&attributes))
}

fn extern_strings<'a>(
    attributes: &'a [Located<alder_source::Attribute<'a>>],
) -> Result<(&'a str, &'a str), Vec<Error<'a>>> {
    let Some(attribute) = attributes
        .iter()
        .find(|attribute| attribute.value.name.value == "extern")
    else {
        return Err(vec![invalid_extern(
            Region::zero(),
            "a bodiless function requires #[extern(\"module\", \"symbol\")]",
        )]);
    };
    let [module, symbol] = attribute.value.args else {
        return Err(vec![invalid_extern(
            attribute.region,
            "extern functions require a module and symbol",
        )]);
    };
    match (&module.value, &symbol.value) {
        (alder_source::Expr::Str(module), alder_source::Expr::Str(symbol)) => Ok((module, symbol)),
        _ => Err(vec![invalid_extern(
            attribute.region,
            "extern module and symbol must be strings",
        )]),
    }
}

fn invalid_extern<'a>(region: Region, reason: &'a str) -> Error<'a> {
    Error::new(
        region,
        ErrorKind::Attribute(AttributeError::InvalidExtern { reason }),
    )
}

fn top_level_name<'a>(env: &Env<'a>, name: &'a str) -> QualifiedName<'a> {
    match env
        .find_value(name)
        .expect("top-level value was predeclared")
        .reference
    {
        alder_ast::ValueRef::TopLevel(reference) => reference,
        _ => unreachable!("module scope contains a top-level reference"),
    }
}

fn top_level_type_name<'a>(env: &Env<'a>, name: &'a str) -> QualifiedName<'a> {
    env.type_binding(name)
        .expect("top-level type was predeclared")
        .reference
}

fn signature_variables<'a>(source: &'a alder_source::FnDecl<'a>) -> BTreeSet<&'a str> {
    let mut variables = BTreeSet::new();
    for param in source.params {
        if let Some(typ) = param.annotation {
            collect_type_variables(typ, &mut variables);
        }
    }
    if let Some(ret) = source.ret {
        collect_type_variables(ret, &mut variables);
    }
    variables
}

fn collect_signature_variable_arities<'a>(
    source: &'a alder_source::FnDecl<'a>,
    arities: &mut BTreeMap<&'a str, usize>,
) {
    for parameter in source.params {
        if let Some(annotation) = parameter.annotation {
            collect_type_variable_arities(annotation, arities);
        }
    }
    if let Some(ret) = source.ret {
        collect_type_variable_arities(ret, arities);
    }
}

fn collect_type_variable_arities<'a>(
    source: &'a Located<alder_source::Type<'a>>,
    arities: &mut BTreeMap<&'a str, usize>,
) {
    match source.value {
        alder_source::Type::Var { name, args } => {
            arities
                .entry(name)
                .and_modify(|arity| *arity = (*arity).max(args.len()))
                .or_insert(args.len());
            for argument in args {
                collect_type_variable_arities(argument, arities);
            }
        }
        alder_source::Type::Named { args, .. } => {
            for argument in args {
                collect_type_variable_arities(argument, arities);
            }
        }
        alder_source::Type::Fn { params, ret } => {
            for parameter in params {
                collect_type_variable_arities(parameter, arities);
            }
            collect_type_variable_arities(ret, arities);
        }
        alder_source::Type::Tuple {
            first,
            second,
            rest,
        } => {
            collect_type_variable_arities(first, arities);
            collect_type_variable_arities(second, arities);
            for item in rest {
                collect_type_variable_arities(item, arities);
            }
        }
        alder_source::Type::Record { fields, .. } => {
            for field in fields {
                collect_type_variable_arities(field.typ, arities);
            }
        }
        alder_source::Type::ErrorRow { tags, .. } => {
            for tag in tags {
                for argument in tag.args {
                    collect_type_variable_arities(argument, arities);
                }
            }
        }
        alder_source::Type::Hole | alder_source::Type::Unit => {}
    }
}

fn kind_from_arity<'a>(bump: &'a Bump, arity: usize) -> alder_ast::Kind<'a> {
    let mut kind = alder_ast::Kind::Type;
    for _ in 0..arity {
        kind = alder_ast::Kind::Arrow {
            param: bump.alloc(alder_ast::Kind::Type),
            result: bump.alloc(kind),
        };
    }
    kind
}

fn type_variables<'a>(source: &'a Located<alder_source::Type<'a>>) -> BTreeSet<&'a str> {
    let mut variables = BTreeSet::new();
    collect_type_variables(source, &mut variables);
    variables
}

fn collect_type_variables<'a>(
    source: &'a Located<alder_source::Type<'a>>,
    variables: &mut BTreeSet<&'a str>,
) {
    match source.value {
        alder_source::Type::Hole => {}
        alder_source::Type::Var { name, args } => {
            variables.insert(name);
            for arg in args {
                collect_type_variables(arg, variables);
            }
        }
        alder_source::Type::Named { args, .. } => {
            for arg in args {
                collect_type_variables(arg, variables);
            }
        }
        alder_source::Type::Fn { params, ret } => {
            for param in params {
                collect_type_variables(param, variables);
            }
            collect_type_variables(ret, variables);
        }
        alder_source::Type::Tuple {
            first,
            second,
            rest,
        } => {
            collect_type_variables(first, variables);
            collect_type_variables(second, variables);
            for typ in rest {
                collect_type_variables(typ, variables);
            }
        }
        alder_source::Type::Record { fields, ext } => {
            for field in fields {
                collect_type_variables(field.typ, variables);
            }
            if let Some(ext) = ext {
                variables.insert(ext.value);
            }
        }
        alder_source::Type::ErrorRow { tags, ext } => {
            for tag in tags {
                for arg in tag.args {
                    collect_type_variables(arg, variables);
                }
            }
            if let Some(ext) = ext {
                variables.insert(ext.value);
            }
        }
        alder_source::Type::Unit => {}
    }
}

fn collect_pattern_names<'a>(
    pattern: &'a Located<alder_source::Pattern<'a>>,
    names: &mut Vec<alder_source::Name<'a>>,
) {
    match pattern.value {
        alder_source::Pattern::Var(name) => names.push(Located::at(pattern.region, name)),
        alder_source::Pattern::Ctor { args, .. } | alder_source::Pattern::Tag { args, .. } => {
            for pattern in args {
                collect_pattern_names(pattern, names);
            }
        }
        alder_source::Pattern::CtorRecord { fields, .. }
        | alder_source::Pattern::Record { fields, .. } => {
            for field in fields {
                match field.pattern {
                    Some(pattern) => collect_pattern_names(pattern, names),
                    None => names.push(field.name),
                }
            }
        }
        alder_source::Pattern::Tuple {
            first,
            second,
            rest,
        } => {
            collect_pattern_names(first, names);
            collect_pattern_names(second, names);
            for pattern in rest {
                collect_pattern_names(pattern, names);
            }
        }
        alder_source::Pattern::Array { elements, rest } => {
            for pattern in elements {
                collect_pattern_names(pattern, names);
            }
            if let Some(name) = rest.and_then(|rest| rest.name) {
                names.push(name);
            }
        }
        alder_source::Pattern::Alias { pattern, name } => {
            collect_pattern_names(pattern, names);
            names.push(name);
        }
        alder_source::Pattern::Anything
        | alder_source::Pattern::Pin(_)
        | alder_source::Pattern::Number(_)
        | alder_source::Pattern::BigInt(_)
        | alder_source::Pattern::Str(_)
        | alder_source::Pattern::Bool(_)
        | alder_source::Pattern::Unit => {}
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use alder_ast::{Expr, PackageId, Pattern, Stmt, UseId};

    fn context() -> Context<'static> {
        Context {
            home: ModuleId {
                package: PackageId::Application,
                path: &[],
            },
            imports: &[],
            interfaces: &[],
        }
    }

    fn can<'a>(bump: &'a Bump, text: &str) -> CanResult<'a> {
        let source_text = bump.alloc_str(text);
        let source = alder_parse::parse_module(bump, source_text).expect("source parses");
        canonicalize(bump, context(), &source).expect("source canonicalizes")
    }

    fn can_error(text: &str) -> String {
        let bump = Bump::new();
        let source_text = bump.alloc_str(text);
        let source = alder_parse::parse_module(&bump, source_text).expect("source parses");
        format!(
            "{:#?}",
            canonicalize(&bump, context(), &source).unwrap_err()
        )
    }

    #[test]
    fn empty_module() {
        let bump = Bump::new();
        let result = can(&bump, "");
        assert!(result.module.items.is_empty());
    }

    #[test]
    fn function_body_resolves_parameter() {
        let bump = Bump::new();
        let result = can(&bump, "fn identity(value: a) -> a { value }");
        let ItemKind::Fn(function) = &result.module.items[0].value.kind else {
            panic!("expected function")
        };
        let Some(body) = function.body.value.tail else {
            panic!("expected tail")
        };
        assert!(matches!(
            body.value,
            Expr::Var {
                reference: alder_ast::ValueRef::Local(_),
                ..
            }
        ));
    }

    #[test]
    fn trait_methods_are_registered_before_bodies() {
        let bump = Bump::new();
        let result = can(
            &bump,
            indoc::indoc! {r#"
                trait Show[a] { fn show(value: a) -> String }
                fn bare(value: a) -> String where a: Show { show(value) }
                fn qualified(value: a) -> String where a: Show { Show::show(value) }
            "#},
        );
        let ItemKind::Trait(trait_) = &result.module.items[0].value.kind else {
            panic!("expected trait")
        };
        let TraitItem::Fn(trait_method) = trait_.items[0] else {
            panic!("expected trait method")
        };
        assert_eq!(trait_method.id.index, 0);
        assert_eq!(trait_method.id.name, "show");
        assert_eq!(trait_method.scheme.params.len(), 1);

        for item in &result.module.items[1..] {
            let ItemKind::Fn(function) = &item.value.kind else {
                panic!("expected function")
            };
            let Some(tail) = function.body.value.tail else {
                panic!("expected tail")
            };
            let Expr::Call { function, .. } = tail.value else {
                panic!("expected call")
            };
            assert!(matches!(
                function.value,
                Expr::Var {
                    reference: alder_ast::ValueRef::TraitMethod { method, .. },
                    ..
                } if method == trait_method.id && method.name == "show"
            ));
        }
    }

    #[test]
    fn trait_method_schemes_preserve_higher_kinded_parameters() {
        let bump = Bump::new();
        let result = can(
            &bump,
            "trait Functor[f] { fn map(apply: fn(a) -> b, value: f[a]) -> f[b] }",
        );
        let ItemKind::Trait(trait_) = &result.module.items[0].value.kind else {
            panic!("expected trait")
        };
        let TraitItem::Fn(method) = trait_.items[0] else {
            panic!("expected method")
        };
        let constructor = method
            .scheme
            .params
            .iter()
            .find(|parameter| parameter.name.value == "f")
            .expect("constructor parameter");
        assert!(matches!(constructor.kind, alder_ast::Kind::Arrow { .. }));
    }

    #[test]
    fn public_trait_methods_survive_interfaces_and_trait_only_imports() {
        let bump = Bump::new();
        let producer_home = ModuleId {
            package: PackageId::Application,
            path: bump.alloc_slice_copy(&["Producer"]),
        };
        let producer_text = bump.alloc_str("pub trait Show[a] { fn show(value: a) -> String }");
        let producer_source =
            alder_parse::parse_module(&bump, producer_text).expect("producer parses");
        let producer = canonicalize(
            &bump,
            Context {
                home: producer_home,
                imports: &[],
                interfaces: &[],
            },
            &producer_source,
        )
        .expect("producer canonicalizes");
        let annotations = crate::Annotations::new();
        let interface = crate::from_module(&bump, producer.module, &annotations);
        assert_eq!(interface.traits[0].methods.len(), 1);
        assert!(matches!(
            interface.values[0].identity,
            alder_ast::InterfaceValueIdentity::TraitMethod(method) if method.name == "show"
        ));

        let consumer_home = ModuleId {
            package: PackageId::Application,
            path: bump.alloc_slice_copy(&["Consumer"]),
        };
        let imports = bump.alloc_slice_copy(&[ResolvedImport {
            module: producer_home,
            region: Region::zero(),
            visibility: Visibility::Private,
            kind: ResolvedImportKind::Names(bump.alloc_slice_copy(&[
                alder_ast::ResolvedImportName {
                    source: Located::at(Region::zero(), "Show"),
                    binding: Located::at(Region::zero(), "Show"),
                },
            ])),
        }]);
        let interfaces = bump.alloc_slice_copy(&[interface]);
        let consumer_text =
            bump.alloc_str("fn render(value: a) -> String where a: Show { Show::show(value) }");
        let consumer_source =
            alder_parse::parse_module(&bump, consumer_text).expect("consumer parses");
        canonicalize(
            &bump,
            Context {
                home: consumer_home,
                imports,
                interfaces,
            },
            &consumer_source,
        )
        .expect("consumer canonicalizes against trait interface");
    }

    #[test]
    fn trait_method_parameters_require_annotations() {
        insta::assert_snapshot!(can_error("trait Show[a] { fn show(value) -> String }"));
    }

    #[test]
    fn trait_parameters_must_be_unique() {
        insta::assert_snapshot!(can_error(
            "trait Convert[a, a] { fn convert(value: a) -> a }"
        ));
    }

    #[test]
    fn trait_method_returns_require_annotations() {
        insta::assert_snapshot!(can_error("trait Show[a] { fn show(value: a) }"));
    }

    #[test]
    fn evidence_sites_receive_stable_lexical_use_ids() {
        let bump = Bump::new();
        let result = can(
            &bump,
            indoc::indoc! {r#"
                fn evidence(mut x, y, f) {
                    x = y
                    x += f(-y)
                    match x { ^y => x + y, _ => x }
                }
            "#},
        );
        let ItemKind::Fn(function) = &result.module.items[0].value.kind else {
            panic!("expected function")
        };

        let Stmt::Assign {
            use_id: None,
            value: set_value,
            ..
        } = function.body.value.statements[0].value
        else {
            panic!("expected plain assignment")
        };
        assert!(matches!(
            set_value.value,
            Expr::Var {
                use_id: UseId(0),
                ..
            }
        ));

        let Stmt::Assign {
            use_id: Some(UseId(1)),
            value: add_value,
            ..
        } = function.body.value.statements[1].value
        else {
            panic!("expected compound assignment")
        };
        let Expr::Call {
            use_id: UseId(3),
            function: callee,
            arguments,
        } = add_value.value
        else {
            panic!("expected call")
        };
        assert!(matches!(
            callee.value,
            Expr::Var {
                use_id: UseId(2),
                ..
            }
        ));
        assert!(matches!(
            arguments[0].value,
            Expr::Negate {
                use_id: UseId(4),
                expr: Located {
                    value: Expr::Var {
                        use_id: UseId(5),
                        ..
                    },
                    ..
                },
            }
        ));

        let Some(tail) = function.body.value.tail else {
            panic!("expected match tail")
        };
        let Expr::Match { scrutinee, arms } = tail.value else {
            panic!("expected match")
        };
        assert!(matches!(
            scrutinee.value,
            Expr::Var {
                use_id: UseId(6),
                ..
            }
        ));
        assert!(matches!(
            arms[0].patterns[0].value,
            Pattern::Pin {
                use_id: UseId(7),
                value: Located {
                    value: Expr::Var {
                        use_id: UseId(8),
                        ..
                    },
                    ..
                },
            }
        ));
        assert!(matches!(
            arms[0].body.value,
            Expr::Binop {
                use_id: UseId(10),
                left: Located {
                    value: Expr::Var {
                        use_id: UseId(9),
                        ..
                    },
                    ..
                },
                right: Located {
                    value: Expr::Var {
                        use_id: UseId(11),
                        ..
                    },
                    ..
                },
                ..
            }
        ));
        assert!(matches!(
            arms[1].body.value,
            Expr::Var {
                use_id: UseId(12),
                ..
            }
        ));
    }

    #[test]
    fn value_sccs_are_complete_dependency_ordered_and_recursive() {
        let bump = Bump::new();
        let result = can(
            &bump,
            indoc::indoc! {r#"
                fn dependent() { base() }
                fn base() { 1 }
                fn even(n) { odd(n) }
                fn odd(n) { even(n) }
                fn self_recursive() { self_recursive() }
            "#},
        );
        let groups = result
            .module
            .value_sccs
            .iter()
            .map(|group| {
                (
                    group.recursive,
                    group
                        .members
                        .iter()
                        .map(|member| member.name)
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            groups,
            vec![
                (true, vec!["self_recursive"]),
                (true, vec!["even", "odd"]),
                (false, vec!["base"]),
                (false, vec!["dependent"]),
            ]
        );
    }

    #[test]
    fn enum_constructor_is_namespaced() {
        let bump = Bump::new();
        let result = can(
            &bump,
            "enum Maybe[a] { Nothing, Just(a) }\nfn none() { Maybe::Nothing }",
        );
        let ItemKind::Fn(function) = &result.module.items[1].value.kind else {
            panic!("expected function")
        };
        assert!(matches!(
            function.body.value.tail.unwrap().value,
            Expr::Constructor(_)
        ));
    }

    #[test]
    fn unknown_value_error() {
        insta::assert_snapshot!(can_error("fn read() { missing }"));
    }

    #[test]
    fn duplicate_definition_error() {
        insta::assert_snapshot!(can_error("let value = 1\nlet value = 2"));
    }

    #[test]
    fn immutable_assignment_error() {
        insta::assert_snapshot!(can_error("fn change() { let value = 1\n value = 2 }"));
    }

    #[test]
    fn loop_control_errors() {
        insta::assert_snapshot!(can_error("fn stop() { break }"));
        insta::assert_snapshot!(can_error("fn skip() { continue }"));
    }

    #[test]
    fn return_outside_function_error() {
        insta::assert_snapshot!(can_error("test \"invalid return\" { return 1 }"));
    }

    #[test]
    fn unqualified_constructor_error() {
        insta::assert_snapshot!(can_error("enum Maybe { Nothing }\nfn none() { Nothing }"));
    }

    #[test]
    fn duplicate_pattern_binding_error() {
        insta::assert_snapshot!(can_error("fn same((left, left)) { left }"));
    }

    #[test]
    fn unavailable_feature_errors() {
        insta::assert_snapshot!(can_error("#[derive(Eq)]\ntype Id = Number"));
        insta::assert_snapshot!(can_error("fn expand() { generated!() }"));
    }

    #[test]
    fn enum_derives_generate_stable_semantic_impls() {
        let bump = Bump::new();
        let result = can(
            &bump,
            "#[derive(Show, Eq, Ord, Hash, Json)]\nenum Status { Ready }",
        );
        let implementations = result
            .module
            .items
            .iter()
            .filter_map(|item| match item.value.kind {
                ItemKind::Impl(implementation) => Some(implementation),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(implementations.len(), 5);
        assert_eq!(
            implementations[0].synthetic,
            Some(alder_ast::DeriveKind::Eq)
        );
        assert_eq!(
            implementations[1].synthetic,
            Some(alder_ast::DeriveKind::Show)
        );
        assert_eq!(
            implementations[2].synthetic,
            Some(alder_ast::DeriveKind::Ord)
        );
        assert_eq!(
            implementations[3].synthetic,
            Some(alder_ast::DeriveKind::Hash)
        );
        assert_eq!(
            implementations[4].synthetic,
            Some(alder_ast::DeriveKind::Json)
        );
        assert!(matches!(
            implementations[1].id.origin,
            alder_ast::ImplOrigin::Derived {
                type_ordinal: 0,
                derive_index: 0
            }
        ));
    }

    #[test]
    fn enum_derives_only_require_payload_type_parameters() {
        let bump = Bump::new();
        let result = can(&bump, "#[derive(Show)]\nenum Phantom[a, b] { Phantom(a) }");
        let implementations = result
            .module
            .items
            .iter()
            .filter_map(|item| match item.value.kind {
                ItemKind::Impl(implementation) => Some(implementation),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(implementations.len(), 2);
        for implementation in implementations {
            assert_eq!(implementation.trait_predicates.len(), 1);
            let Type::Var { name, args: [] } = implementation.trait_predicates[0].args[0].value
            else {
                panic!("derive prerequisite should target a type parameter")
            };
            assert_eq!(name, "a");
        }
    }

    #[test]
    fn error_groups_receive_automatic_and_explicit_derives() {
        let bump = Bump::new();
        let result = can(
            &bump,
            "#[derive(Show, Eq, Ord, Hash, Json)]\nerror Failure { :later, :first(Number) }",
        );
        let implementations = result
            .module
            .items
            .iter()
            .filter_map(|item| match item.value.kind {
                ItemKind::Impl(implementation) => Some(implementation),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(implementations.len(), 5);
        assert_eq!(
            implementations
                .iter()
                .map(|implementation| implementation.synthetic.unwrap())
                .collect::<Vec<_>>(),
            vec![
                alder_ast::DeriveKind::Eq,
                alder_ast::DeriveKind::Show,
                alder_ast::DeriveKind::Ord,
                alder_ast::DeriveKind::Hash,
                alder_ast::DeriveKind::Json,
            ]
        );
        assert!(implementations.iter().all(|implementation| {
            matches!(
                implementation.trait_ref.args[0].value,
                Type::Named { reference, args: [] } if reference.name == "Failure"
            )
        }));
    }

    #[test]
    fn invalid_derive_arguments_are_structured() {
        insta::assert_snapshot!(can_error("#[derive(Missing)]\nenum Status { Ready }"));
        insta::assert_snapshot!(can_error("#[derive(\"Eq\")]\nenum Status { Ready }"));
        insta::assert_snapshot!(can_error("#[derive(Eq, Eq)]\nenum Status { Ready }"));
        insta::assert_snapshot!(can_error(
            "#[derive(Eq)]\nenum Callback { Callback(fn() -> ()) }"
        ));
    }

    #[test]
    fn function_fields_do_not_receive_automatic_equality() {
        let bump = Bump::new();
        let result = can(&bump, "enum Callback { Callback(fn() -> ()) }");
        assert!(
            !result
                .module
                .items
                .iter()
                .any(|item| matches!(item.value.kind, ItemKind::Impl(_)))
        );
    }

    #[test]
    fn expression_shape_errors() {
        insta::assert_snapshot!(can_error("fn compare() { 1 < 2 < 3 }"));
        insta::assert_snapshot!(can_error("fn duplicate() { { first: 1, first: 2 } }"));
    }

    #[test]
    fn unknown_type_error() {
        insta::assert_snapshot!(can_error("fn invalid(value: Missing) { value }"));
    }

    #[test]
    fn function_where_bounds_are_preserved() {
        let bump = Bump::new();
        let result = can(
            &bump,
            indoc::indoc! {r#"
                trait Show[a] { fn show(value: a) -> String }
                fn keep(value: a) -> a where a: Show { value }
            "#},
        );
        let ItemKind::Fn(function) = &result.module.items[1].value.kind else {
            panic!("expected function")
        };
        assert!(matches!(
            function.constraints,
            [alder_ast::TypeConstraint::Bound { var, traits }]
                if var.value == "a" && traits.len() == 1 && traits[0].name == "Show"
        ));
    }

    #[test]
    fn associated_equality_resolves_to_semantic_projection() {
        let bump = Bump::new();
        let result = can(
            &bump,
            indoc::indoc! {r#"
                trait Iterator[i] {
                    type Item
                    fn next(value: i) -> Item
                }
                fn count(value: i) -> Number where i: Iterator, i.Item == Number { 0 }
            "#},
        );
        let ItemKind::Fn(function) = &result.module.items[1].value.kind else {
            panic!("expected function")
        };
        let [
            _,
            alder_ast::TypeConstraint::AssocEq {
                projection, typ, ..
            },
        ] = function.constraints
        else {
            panic!("expected an associated equality")
        };
        assert_eq!(projection.trait_ref.trait_.0.name, "Iterator");
        assert_eq!(projection.assoc.name, "Item");
        assert!(matches!(typ.value, Type::Named { reference, .. } if reference.name == "Number"));
    }

    #[test]
    fn associated_equality_requires_a_matching_bound() {
        insta::assert_snapshot!(can_error(indoc::indoc! {r#"
            trait Iterator[i] { type Item }
            trait Show[a] { fn show(value: a) -> String }
            fn bad(value: i) where i: Show, i.Item == Number { value }
        "#}));
    }

    #[test]
    fn associated_equality_rejects_ambiguous_associated_names() {
        insta::assert_snapshot!(can_error(indoc::indoc! {r#"
            trait First[a] { type Item }
            trait Second[a] { type Item }
            fn bad(value: a) where a: First + Second, a.Item == Number { value }
        "#}));
    }

    #[test]
    fn function_where_bound_variable_must_occur_in_signature() {
        insta::assert_snapshot!(can_error(indoc::indoc! {r#"
            trait Show[a] { fn show(value: a) -> String }
            fn bad(value: Number) where a: Show { value }
        "#}));
    }

    #[test]
    fn colon_bound_requires_a_unary_trait() {
        insta::assert_snapshot!(can_error(indoc::indoc! {r#"
            trait Convert[a, b] { fn convert(value: a) -> b }
            fn bad(value: a) where a: Convert { value }
        "#}));
    }

    #[test]
    fn impl_where_variable_must_occur_in_the_impl_head() {
        insta::assert_snapshot!(can_error(indoc::indoc! {r#"
            trait Show[a] { fn show(value: a) -> String }
            impl Show[Array[a]] where b: Show {
                fn show(value: Array[a]) -> String { "array" }
            }
        "#}));
    }

    #[test]
    fn impl_headers_have_stable_identity_and_semantic_members() {
        let bump = Bump::new();
        let result = can(
            &bump,
            indoc::indoc! {r#"
                trait Iterator[i] {
                    type Item
                    fn next(value: i) -> Item
                }
                impl Iterator[Array[a]] where a: Show {
                    type Item = a
                    fn next(value: Array[a]) -> a { value[0] }
                }
                trait Show[a] { fn show(value: a) -> String }
            "#},
        );
        let ItemKind::Impl(implementation) = &result.module.items[1].value.kind else {
            panic!("expected impl")
        };
        assert_eq!(
            implementation.id.origin,
            alder_ast::ImplOrigin::Source { item_ordinal: 1 }
        );
        assert_eq!(implementation.trait_ref.trait_.0.name, "Iterator");
        assert_eq!(implementation.params.len(), 1);
        assert_eq!(implementation.trait_predicates.len(), 1);
        assert_eq!(implementation.assoc_bindings[0].assoc.name, "Item");
        let ImplItem::Fn(method) = implementation.items[1] else {
            panic!("expected method")
        };
        assert_eq!(method.method.name, "next");
        let interface = crate::from_module(&bump, result.module, &crate::Annotations::new());
        assert_eq!(interface.instances.len(), 1);
        assert_eq!(interface.instances[0].id, implementation.id);
        assert_eq!(
            interface.instances[0].dictionary_kind,
            alder_ast::DictionaryKind::Factory
        );
        assert_eq!(interface.instances[0].methods.len(), 1);
    }

    #[test]
    fn impl_trait_head_arity_is_checked() {
        insta::assert_snapshot!(can_error(indoc::indoc! {r#"
            trait Convert[a, b] { fn convert(value: a) -> b }
            impl Convert[Number] { fn convert(value: Number) -> Number { value } }
        "#}));
    }

    #[test]
    fn impl_unknown_method_is_rejected() {
        insta::assert_snapshot!(can_error(indoc::indoc! {r#"
            trait Show[a] { fn show(value: a) -> String }
            impl Show[Number] {
                fn display(value: Number) -> String { "number" }
            }
        "#}));
    }

    #[test]
    fn impl_missing_required_method_is_rejected() {
        insta::assert_snapshot!(can_error(indoc::indoc! {r#"
            trait Show[a] { fn show(value: a) -> String }
            impl Show[Number] {}
        "#}));
    }

    #[test]
    fn impl_missing_associated_type_is_rejected() {
        insta::assert_snapshot!(can_error(indoc::indoc! {r#"
            trait Iterator[i] {
                type Item
                fn next(value: i) -> Item
            }
            impl Iterator[Number] {
                fn next(value: Number) -> Number { value }
            }
        "#}));
    }

    #[test]
    fn impl_may_use_a_default_method() {
        let bump = Bump::new();
        can(
            &bump,
            indoc::indoc! {r#"
                trait Named[a] { fn name(value: a) -> String { "default" } }
                impl Named[Number] {}
            "#},
        );
    }

    #[test]
    fn await_requires_explicit_task_return_in_m2() {
        insta::assert_snapshot!(can_error("fn wait() { Task.sleep(1).await }"));

        let bump = Bump::new();
        can(&bump, "fn wait() -> Task[()] { Task.sleep(1).await }");
    }

    #[test]
    fn first_party_stdlib_sources_canonicalize() {
        let std_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std");
        let mut paths = fs::read_dir(std_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "ald"))
            .collect::<Vec<_>>();
        paths.sort();

        for path in paths {
            let source = fs::read_to_string(&path).unwrap();
            let bump = Bump::new();
            can(&bump, &source);
        }
    }
}
