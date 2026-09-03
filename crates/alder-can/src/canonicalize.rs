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

use crate::environment::Env;
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
    let mut env = Env::new(context.home);
    let mut errors = load_imports(bump, &mut env, context.imports, context.interfaces);
    errors.extend(predeclare(&mut env, source));
    let enums = canonicalize_enums(bump, &mut env, source, &mut errors);
    if !errors.is_empty() {
        return Err(errors);
    }

    let mut items = Vec::new();
    for item in source.items {
        if matches!(item.value.kind, SourceItemKind::Import(_)) {
            continue;
        }
        match canonicalize_item(bump, &mut env, item, &enums) {
            Ok(item) => items.push(item),
            Err(mut item_errors) => errors.append(&mut item_errors),
        }
    }
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
        env.insert_foreign_value(
            binding.value,
            binding.region,
            value.reference,
            value.annotation,
        )
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
        env.insert_foreign_trait(
            binding.value,
            binding.region,
            trait_.reference,
            trait_.params.len(),
        )
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
        if let Err(first) =
            env.insert_foreign_value(value.exported_as, region, value.reference, value.annotation)
        {
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
        if let Err(first) = env.insert_foreign_trait(
            trait_.exported_as,
            region,
            trait_.reference,
            trait_.params.len(),
        ) {
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
                name: param,
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
        params: bump.alloc_slice_fill_iter(enum_.params.iter().map(|param| alder_ast::TypeParam {
            name: Located::at(Region::zero(), *param),
            kind: alder_ast::Kind::Type,
        })),
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
        SourceItemKind::Impl(decl) => ItemKind::Impl(canonicalize_impl(bump, env, decl)?),
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
            for nested in items {
                if matches!(nested.value.kind, SourceItemKind::Import(_)) {
                    continue;
                }
                canonical.push(canonicalize_item(
                    bump,
                    &mut nested_env,
                    nested,
                    &nested_enums,
                )?);
            }
            ItemKind::Tests(bump.alloc_slice_copy(&canonical))
        }
        SourceItemKind::Import(_) => unreachable!("imports are filtered before item conversion"),
    };
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
    let name = env
        .find_trait(bump, source.name.region, None, source.name.value)
        .map_err(|error| vec![error])?
        .reference;
    let constraints = canonicalize_constraints(bump, env, source.where_clause, &variables)?;
    let mut items = Vec::with_capacity(source.items.len());
    for item in source.items {
        items.push(match item {
            alder_source::TraitItem::AssocType(name) => TraitItem::AssocType(*name),
            alder_source::TraitItem::Fn(function) => {
                TraitItem::Fn(canonicalize_trait_fn(bump, env, function, &variables)?)
            }
        });
    }
    Ok(bump.alloc(TraitDecl {
        name,
        params: source.params,
        constraints,
        items: bump.alloc_slice_copy(&items),
    }))
}

fn canonicalize_trait_fn<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a alder_source::FnDecl<'a>,
    trait_variables: &BTreeSet<&'a str>,
) -> Result<&'a TraitFn<'a>, Vec<Error<'a>>> {
    let mut variables = trait_variables.clone();
    variables.extend(signature_variables(source));
    env.push_scope();
    let saved_control = env.control;
    env.control.function_depth += 1;
    env.control.loop_depth = 0;
    let result = (|| {
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
            name: source.name,
            params,
            ret,
            constraints,
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
) -> Result<&'a ImplDecl<'a>, Vec<Error<'a>>> {
    let trait_name = source
        .trait_
        .segments
        .last()
        .expect("trait path is nonempty");
    let trait_ = env
        .find_trait(
            bump,
            source.trait_.region(),
            (source.trait_.segments.len() > 1).then(|| source.trait_.segments[0].value),
            trait_name.value,
        )
        .map_err(|error| vec![error])?
        .reference;
    let mut variables = BTreeSet::new();
    for arg in source.args {
        collect_type_variables(arg, &mut variables);
    }
    let mut args = Vec::with_capacity(source.args.len());
    for arg in source.args {
        args.push(canonicalize_impl_head_type(bump, env, &variables, arg)?);
    }
    let constraints = canonicalize_constraints(bump, env, source.where_clause, &variables)?;
    let mut items = Vec::with_capacity(source.items.len());
    for item in source.items {
        items.push(match item {
            alder_source::ImplItem::AssocType { name, typ } => ImplItem::AssocType {
                name: *name,
                typ: canonicalize_type(bump, env, &variables, typ)?,
            },
            alder_source::ImplItem::Fn(function) => {
                ImplItem::Fn(canonicalize_impl_fn(bump, env, function, &variables)?)
            }
        });
    }
    Ok(bump.alloc(ImplDecl {
        trait_,
        args: bump.alloc_slice_copy(&args),
        constraints,
        items: bump.alloc_slice_copy(&items),
    }))
}

fn canonicalize_impl_fn<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a alder_source::FnDecl<'a>,
    impl_variables: &BTreeSet<&'a str>,
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
            name: source.name,
            params,
            ret,
            constraints,
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
    for constraint in source {
        constraints.push(match constraint {
            alder_source::Constraint::Bound { var, bounds } => {
                if !variables.contains(var.value) {
                    return Err(vec![Error::new(
                        var.region,
                        ErrorKind::Type(TypeError::UnboundVariable { name: var.value }),
                    )]);
                }
                let mut traits = Vec::with_capacity(bounds.len());
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
                    traits.push(binding.reference);
                }
                TypeConstraint::Bound {
                    var: *var,
                    traits: bump.alloc_slice_copy(&traits),
                }
            }
            alder_source::Constraint::AssocEq { var, assoc, typ } => {
                if !variables.contains(var.value) {
                    return Err(vec![Error::new(
                        var.region,
                        ErrorKind::Type(TypeError::UnboundVariable { name: var.value }),
                    )]);
                }
                TypeConstraint::AssocEq {
                    var: *var,
                    assoc: *assoc,
                    typ: canonicalize_type(bump, env, variables, typ)?,
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
                return Err(vec![Error::new(
                    attribute.region,
                    ErrorKind::Attribute(AttributeError::DeriveUnavailable),
                )]);
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
