use alder_ast::{
    DictionaryKind, Interface, InterfaceEnum, InterfaceImpl, InterfaceMethod, InterfaceModule,
    InterfaceTrait, InterfaceType, InterfaceValue, InterfaceValueIdentity, ItemKind, Kind,
    MethodImplementation, Module, Namespace, OpaqueKind, PrivateName, PublicTypeBody,
    ResolvedImportKind, TypeParam, ValueKind, Visibility,
};
use bumpalo::Bump;

use crate::Annotations;

const BUILTIN_TRAITS_SOURCE: &str = include_str!("../stdlib/Traits.ald");

const BUILTIN_VALUE_SOURCES: &[(&str, &str)] = &[
    ("Array", include_str!("../stdlib/Array.ald")),
    ("BigInt", include_str!("../stdlib/BigInt.ald")),
    ("Cli", include_str!("../stdlib/Cli.ald")),
    ("Fiber", include_str!("../stdlib/Fiber.ald")),
    ("Io", include_str!("../stdlib/Io.ald")),
    ("Json", include_str!("../stdlib/Json.ald")),
    ("Map", include_str!("../stdlib/Map.ald")),
    ("Number", include_str!("../stdlib/Number.ald")),
    ("Option", include_str!("../stdlib/Option.ald")),
    ("Ref", include_str!("../stdlib/Ref.ald")),
    ("Result", include_str!("../stdlib/Result.ald")),
    ("Semaphore", include_str!("../stdlib/Semaphore.ald")),
    (
        "SynchronizedRef",
        include_str!("../stdlib/SynchronizedRef.ald"),
    ),
    ("Set", include_str!("../stdlib/Set.ald")),
    ("String", include_str!("../stdlib/String.ald")),
    ("Task", include_str!("../stdlib/Task.ald")),
];

pub(crate) fn builtin_type_interface<'a>(
    bump: &'a Bump,
    module: alder_ast::ModuleId<'a>,
) -> Option<&'a Interface<'a>> {
    if module.package != alder_ast::PackageId::Builtin {
        return None;
    }
    let (_, source) = BUILTIN_VALUE_SOURCES
        .iter()
        .find(|(name, _)| module.path == [*name])?;
    let parsed =
        alder_parse::parse_module(bump, source).expect("packaged stdlib declarations must parse");
    let headers = crate::canonicalize_headers(
        bump,
        crate::Context {
            home: module,
            imports: &[],
            interfaces: &[],
        },
        &parsed,
    )
    .expect("packaged stdlib type declarations must canonicalize");
    Some(bump.alloc(headers_from_module(bump, headers.module, &[])))
}

pub(crate) fn builtin_value_annotations<'a>(
    bump: &'a Bump,
    module: alder_ast::ModuleId<'a>,
) -> std::collections::BTreeMap<&'a str, &'a alder_ast::Annotation<'a>> {
    let Some((_, source)) = BUILTIN_VALUE_SOURCES
        .iter()
        .find(|(name, _)| module.path == [*name])
    else {
        return Default::default();
    };
    builtin_value_annotations_from_source(bump, module, source)
}

fn builtin_value_annotations_from_source<'a>(
    bump: &'a Bump,
    module: alder_ast::ModuleId<'a>,
    source: &'a str,
) -> std::collections::BTreeMap<&'a str, &'a alder_ast::Annotation<'a>> {
    let parsed =
        alder_parse::parse_module(bump, source).expect("packaged stdlib declarations must parse");
    // Use the builtin environment, never the importing module's shadowed names.
    let mut env = crate::environment::Env::new(bump, module);
    if parsed
        .items
        .iter()
        .any(|item| matches!(item.value.kind, alder_source::ItemKind::TypeAlias(_)))
    {
        let headers = crate::canonicalize_headers(
            bump,
            crate::Context {
                home: module,
                imports: &[],
                interfaces: &[],
            },
            &parsed,
        )
        .expect("packaged stdlib type declarations must canonicalize");
        for item in headers.module.items {
            let ItemKind::TypeAlias(alias) = &item.value.kind else {
                continue;
            };
            // Headers have already checked local name collisions and alias cycles.
            // Private aliases are available inside signatures too.
            env.types.insert(
                alias.name.name,
                crate::environment::Candidate::Unique(crate::environment::TypeBinding {
                    reference: alias.name,
                    arity: alias.params.len(),
                    region: item.region,
                }),
            );
            env.aliases.insert(
                alias.name,
                crate::aliases::Definition {
                    params: bump.alloc_slice_copy(
                        &alias
                            .params
                            .iter()
                            .map(|param| param.value)
                            .collect::<Vec<_>>(),
                    ),
                    body: alias.typ,
                },
            );
        }
    }
    parsed
        .items
        .iter()
        .filter_map(|item| {
            if !matches!(item.value.visibility, alder_source::Visibility::Pub(_)) {
                return None;
            }
            if let alder_source::ItemKind::Let(binding) = &item.value.kind {
                let alder_source::Pattern::Var(name) = binding.pattern.value else {
                    panic!("packaged stdlib values must have simple names");
                };
                let source = binding
                    .annotation
                    .expect("packaged stdlib values must be annotated");
                // Builtin values are shared, not factories: never quantify their
                // payloads. Function declarations use the separate path below.
                let typ = crate::types::canonicalize_type(bump, &env, &Default::default(), source)
                    .expect("packaged stdlib value types must canonicalize");
                let annotation = bump.alloc(alder_ast::Annotation {
                    params: &[],
                    trait_predicates: &[],
                    projection_equalities: &[],
                    error_row_inclusions: &[],
                    record_overlays: &[],
                    tuple_shapes: &[],
                    typ,
                });
                return Some((name, &*annotation));
            }
            let alder_source::ItemKind::Fn(function) = &item.value.kind else {
                return None;
            };
            let annotation = crate::canonicalize::trait_method_annotation(
                bump,
                &env,
                function,
                &Default::default(),
                &Default::default(),
            )
            .expect("packaged stdlib signatures must canonicalize");
            Some((function.name.value, annotation))
        })
        .collect()
}

/// Canonical first-party trait headers authored in Alder source.
pub fn builtin_trait_interface<'a>(bump: &'a Bump) -> Interface<'a> {
    let source = alder_parse::parse_module(bump, BUILTIN_TRAITS_SOURCE)
        .expect("the embedded first-party trait module must parse");
    let result = crate::canonicalize_headers(
        bump,
        crate::Context {
            home: alder_ast::ModuleId {
                package: alder_ast::PackageId::Builtin,
                path: &[],
            },
            imports: &[],
            interfaces: &[],
        },
        &source,
    )
    .expect("the embedded first-party trait module must canonicalize");
    headers_from_module(bump, result.module, &[])
}

/// Build the public, solved contract consumed by dependent modules.
pub fn from_module<'a>(
    bump: &'a Bump,
    module: &'a Module<'a>,
    annotations: &Annotations<'a>,
    imports: &[Interface<'a>],
) -> Interface<'a> {
    interface_from_module(bump, module, Some(annotations), imports)
}

/// Build the canonical declaration header used while collecting a package's
/// complete trait database. Value bindings are omitted until their inferred
/// schemes are available, while types, traits, and impl heads are complete.
/// `imports` must contain the dependency interfaces used for canonicalization,
/// so public named and wildcard imports can publish their original identities.
pub fn headers_from_module<'a>(
    bump: &'a Bump,
    module: &'a Module<'a>,
    imports: &[Interface<'a>],
) -> Interface<'a> {
    interface_from_module(bump, module, None, imports)
}

fn interface_from_module<'a>(
    bump: &'a Bump,
    module: &'a Module<'a>,
    annotations: Option<&Annotations<'a>>,
    imports: &[Interface<'a>],
) -> Interface<'a> {
    let mut values = Vec::new();
    let mut types = Vec::new();
    let mut enums = Vec::new();
    let mut traits = Vec::new();
    let mut instances = Vec::new();
    let mut modules = Vec::new();
    let mut private_names = Vec::new();

    for item in module.items {
        let public = matches!(item.value.visibility, Visibility::Public(_));
        match &item.value.kind {
            ItemKind::Fn(function) => {
                if let Some(annotations) = annotations {
                    value(
                        annotations,
                        public,
                        function.name,
                        ValueKind::Function,
                        &mut values,
                        &mut private_names,
                    );
                }
            }
            ItemKind::Let(decl) => {
                if let Some(annotations) = annotations {
                    for binding in decl.bindings {
                        value(
                            annotations,
                            public,
                            *binding,
                            ValueKind::Let,
                            &mut values,
                            &mut private_names,
                        );
                    }
                }
            }
            ItemKind::Component(component) => {
                if let Some(annotations) = annotations {
                    value(
                        annotations,
                        public,
                        component.name,
                        ValueKind::Component,
                        &mut values,
                        &mut private_names,
                    );
                }
            }
            ItemKind::Extern(alder_ast::ExternDecl::Fn { name, .. }) => {
                if let Some(annotations) = annotations {
                    value(
                        annotations,
                        public,
                        *name,
                        ValueKind::Extern,
                        &mut values,
                        &mut private_names,
                    );
                }
            }
            ItemKind::TypeAlias(alias) => {
                if public {
                    types.push(InterfaceType {
                        exported_as: alias.name.name,
                        reference: alias.name,
                        params: type_params(bump, alias.params),
                        result_kind: Kind::Type,
                        body: PublicTypeBody::Alias(alias.typ),
                    });
                } else {
                    private(&mut private_names, alias.name.name, Namespace::Type);
                }
            }
            ItemKind::Enum(enum_) => {
                if public {
                    enums.push(InterfaceEnum {
                        exported_as: enum_.name.name,
                        reference: enum_.name,
                        params: type_params(bump, enum_.params),
                        result_kind: Kind::Type,
                        variants: enum_.variants,
                    });
                } else {
                    private(&mut private_names, enum_.name.name, Namespace::Enum);
                }
            }
            ItemKind::Trait(trait_) => {
                if public {
                    let methods = trait_
                        .items
                        .iter()
                        .filter_map(|item| match item {
                            alder_ast::TraitItem::AssocType(_) => None,
                            alder_ast::TraitItem::Fn(method) => Some(InterfaceMethod {
                                id: method.id,
                                exported_as: method.name.value,
                                scheme: method.scheme,
                                has_default: method.body.is_some(),
                                default_symbol: method.body.is_some().then(|| {
                                    &*bump.alloc_str(&format!(
                                        "$default${}${}",
                                        method.id.trait_.0.name, method.id.name
                                    ))
                                }),
                            }),
                        })
                        .collect::<Vec<_>>();
                    let methods = bump.alloc_slice_copy(&methods);
                    for method in trait_.items.iter().filter_map(|item| match item {
                        alder_ast::TraitItem::AssocType(_) => None,
                        alder_ast::TraitItem::Fn(method) => Some(*method),
                    }) {
                        values.push(InterfaceValue {
                            exported_as: method.name.value,
                            identity: InterfaceValueIdentity::TraitMethod(method.id),
                            annotation: method.scheme,
                            kind: ValueKind::TraitMethod,
                        });
                    }
                    traits.push(InterfaceTrait {
                        exported_as: trait_.name.name,
                        id: trait_.id,
                        params: trait_.type_params,
                        superclasses: trait_.superclasses,
                        associated_types: trait_.associated_types,
                        methods,
                    });
                } else {
                    private(&mut private_names, trait_.name.name, Namespace::Trait);
                }
            }
            ItemKind::ErrorGroup(group) => {
                opaque_type(
                    public,
                    group.name,
                    PublicTypeBody::ErrorGroup(group.tags),
                    &mut types,
                    &mut private_names,
                );
            }
            ItemKind::Table(table) => opaque_type(
                public,
                table.name,
                PublicTypeBody::Opaque(OpaqueKind::Table),
                &mut types,
                &mut private_names,
            ),
            ItemKind::Schema(schema) => opaque_type(
                public,
                schema.name,
                PublicTypeBody::Opaque(OpaqueKind::Schema),
                &mut types,
                &mut private_names,
            ),
            ItemKind::Extern(alder_ast::ExternDecl::Type { name }) => opaque_type(
                public,
                *name,
                PublicTypeBody::Opaque(OpaqueKind::Extern),
                &mut types,
                &mut private_names,
            ),
            ItemKind::Impl(implementation) => {
                if annotations.is_some() && !impl_is_externally_nameable(module, implementation) {
                    continue;
                }
                let dictionary_symbol = bump.alloc_str(&format!(
                    "$dict${}${}",
                    implementation.trait_.name,
                    impl_origin_index(implementation.id.origin)
                ));
                let mut methods = Vec::new();
                if let Some(trait_) = module.items.iter().find_map(|item| match &item.value.kind {
                    ItemKind::Trait(trait_) if trait_.id == implementation.trait_ref.trait_ => {
                        Some(*trait_)
                    }
                    _ => None,
                }) {
                    for item in trait_.items {
                        let alder_ast::TraitItem::Fn(trait_method) = item else {
                            continue;
                        };
                        let provided = implementation.items.iter().find_map(|item| match item {
                            alder_ast::ImplItem::Fn(method) if method.method == trait_method.id => {
                                Some(*method)
                            }
                            _ => None,
                        });
                        let method = if provided.is_some() {
                            MethodImplementation::Provided {
                                symbol: bump.alloc_str(&format!(
                                    "$impl${}${}",
                                    impl_origin_index(implementation.id.origin),
                                    trait_method.id.name
                                )),
                            }
                        } else {
                            MethodImplementation::Default {
                                symbol: bump.alloc_str(&format!(
                                    "$default${}${}",
                                    trait_.id.0.name, trait_method.id.name
                                )),
                            }
                        };
                        methods.push((trait_method.id, method));
                    }
                } else {
                    for item in implementation.items {
                        match item {
                            alder_ast::ImplItem::Fn(method) => methods.push((
                                method.method,
                                MethodImplementation::Provided {
                                    symbol: bump.alloc_str(&format!(
                                        "$impl${}${}",
                                        impl_origin_index(implementation.id.origin),
                                        method.method.name
                                    )),
                                },
                            )),
                            alder_ast::ImplItem::Default { method, symbol, .. } => {
                                methods.push((*method, MethodImplementation::Default { symbol }));
                            }
                            alder_ast::ImplItem::AssocType { .. } => {}
                        }
                    }
                }
                instances.push(InterfaceImpl {
                    id: implementation.id,
                    source_uri: None,
                    region: Some(implementation.region),
                    params: implementation.params,
                    trait_ref: implementation.trait_ref,
                    trait_predicates: implementation.trait_predicates,
                    projection_equalities: implementation.projection_equalities,
                    assoc_bindings: implementation.assoc_bindings,
                    dictionary_symbol,
                    dictionary_kind: if implementation.trait_predicates.is_empty() {
                        DictionaryKind::Singleton
                    } else {
                        DictionaryKind::Factory
                    },
                    methods: bump.alloc_slice_copy(&methods),
                });
            }
            ItemKind::Test(_) | ItemKind::Tests(_) | ItemKind::Macro(_) | ItemKind::Comptime(_) => {
            }
        }
    }

    for import in module.imports {
        if !matches!(import.visibility, Visibility::Public(_)) {
            continue;
        }
        if let ResolvedImportKind::Module { binding } = import.kind {
            modules.push(InterfaceModule {
                exported_as: binding.value,
                module: import.module,
            });
            continue;
        }
        let Some(interface) = imports
            .iter()
            .find(|interface| interface.home == import.module)
        else {
            continue;
        };
        // Keep checked schemes and defining identities; a re-export is an alias,
        // not a fresh binding or another definition of a trait/instance.
        let selections = match import.kind {
            ResolvedImportKind::All => vec![None],
            ResolvedImportKind::Names(names) => names.iter().map(Some).collect(),
            ResolvedImportKind::Module { .. } => unreachable!("handled above"),
        };
        for selection in selections {
            let exported_name = |name: &'a str| match selection {
                None => Some(name),
                Some(entry) if entry.source.value == name => Some(entry.binding.value),
                Some(_) => None,
            };
            if annotations.is_some() {
                values.extend(interface.values.iter().filter_map(|value| {
                    Some(InterfaceValue {
                        exported_as: exported_name(value.exported_as)?,
                        ..*value
                    })
                }));
            }
            types.extend(interface.types.iter().filter_map(|typ| {
                Some(InterfaceType {
                    exported_as: exported_name(typ.exported_as)?,
                    ..*typ
                })
            }));
            enums.extend(interface.enums.iter().filter_map(|enum_| {
                Some(alder_ast::InterfaceEnum {
                    exported_as: exported_name(enum_.exported_as)?,
                    ..*enum_
                })
            }));
            traits.extend(interface.traits.iter().filter_map(|trait_| {
                Some(alder_ast::InterfaceTrait {
                    exported_as: exported_name(trait_.exported_as)?,
                    ..*trait_
                })
            }));
            modules.extend(interface.modules.iter().filter_map(|module| {
                Some(InterfaceModule {
                    exported_as: exported_name(module.exported_as)?,
                    ..*module
                })
            }));
        }
    }

    Interface {
        home: module.id,
        values: bump.alloc_slice_copy(&values),
        types: bump.alloc_slice_copy(&types),
        enums: bump.alloc_slice_copy(&enums),
        traits: bump.alloc_slice_copy(&traits),
        instances: bump.alloc_slice_copy(&instances),
        modules: bump.alloc_slice_copy(&modules),
        private_names: bump.alloc_slice_copy(&private_names),
    }
}

fn impl_is_externally_nameable(
    module: &Module<'_>,
    implementation: &alder_ast::ImplDecl<'_>,
) -> bool {
    trait_ref_is_externally_nameable(module, implementation.trait_ref)
        && implementation
            .trait_predicates
            .iter()
            .all(|predicate| trait_ref_is_externally_nameable(module, *predicate))
        && implementation.projection_equalities.iter().all(|equality| {
            trait_ref_is_externally_nameable(module, equality.projection.trait_ref)
                && type_is_externally_nameable(module, &equality.typ.value)
        })
        && implementation
            .assoc_bindings
            .iter()
            .all(|binding| type_is_externally_nameable(module, &binding.typ.value))
}

fn trait_ref_is_externally_nameable(
    module: &Module<'_>,
    trait_ref: alder_ast::TraitRef<'_>,
) -> bool {
    trait_is_externally_nameable(module, trait_ref.trait_)
        && trait_ref
            .args
            .iter()
            .all(|argument| type_is_externally_nameable(module, &argument.value))
}

fn trait_is_externally_nameable(module: &Module<'_>, trait_: alder_ast::TraitId<'_>) -> bool {
    trait_.0.module != module.id
        || module.items.iter().any(|item| {
            matches!(item.value.visibility, Visibility::Public(_))
                && matches!(
                    &item.value.kind,
                    ItemKind::Trait(declaration) if declaration.id == trait_
                )
        })
}

fn name_is_externally_nameable(module: &Module<'_>, name: alder_ast::QualifiedName<'_>) -> bool {
    name.module != module.id
        || module.items.iter().any(|item| {
            if !matches!(item.value.visibility, Visibility::Public(_)) {
                return false;
            }
            match &item.value.kind {
                ItemKind::TypeAlias(declaration) => declaration.name == name,
                ItemKind::Enum(declaration) => declaration.name == name,
                ItemKind::ErrorGroup(declaration) => declaration.name == name,
                ItemKind::Table(declaration) => declaration.name == name,
                ItemKind::Schema(declaration) => declaration.name == name,
                ItemKind::Extern(alder_ast::ExternDecl::Type { name: declaration }) => {
                    *declaration == name
                }
                _ => false,
            }
        })
}

fn type_is_externally_nameable(module: &Module<'_>, typ: &alder_ast::Type<'_>) -> bool {
    match typ {
        alder_ast::Type::Var { args, .. } => args
            .iter()
            .all(|argument| type_is_externally_nameable(module, &argument.value)),
        alder_ast::Type::Named { reference, args } => {
            name_is_externally_nameable(module, *reference)
                && args
                    .iter()
                    .all(|argument| type_is_externally_nameable(module, &argument.value))
        }
        alder_ast::Type::Partial { constructor, slots } => {
            name_is_externally_nameable(module, *constructor)
                && slots.iter().all(|slot| match slot {
                    alder_ast::TypeSlot::Hole(_) => true,
                    alder_ast::TypeSlot::Fixed(typ) => {
                        type_is_externally_nameable(module, &typ.value)
                    }
                })
        }
        alder_ast::Type::Projection(projection) => {
            trait_ref_is_externally_nameable(module, projection.trait_ref)
        }
        alder_ast::Type::Fn { params, ret } => {
            params
                .iter()
                .all(|param| type_is_externally_nameable(module, &param.value))
                && type_is_externally_nameable(module, &ret.value)
        }
        alder_ast::Type::Unit => true,
        alder_ast::Type::Tuple(types) => types
            .iter()
            .all(|typ| type_is_externally_nameable(module, &typ.value)),
        alder_ast::Type::Record { fields, .. } => fields
            .iter()
            .all(|field| type_is_externally_nameable(module, &field.typ.value)),
        alder_ast::Type::ErrorRow { tags, .. } => tags.iter().all(|tag| {
            tag.args
                .iter()
                .all(|argument| type_is_externally_nameable(module, &argument.value))
        }),
        alder_ast::Type::Alias {
            reference,
            arguments,
            target,
        } => {
            name_is_externally_nameable(module, *reference)
                && arguments
                    .iter()
                    .all(|argument| type_is_externally_nameable(module, &argument.typ.value))
                && match target {
                    alder_ast::AliasType::Open(target) | alder_ast::AliasType::Filled(target) => {
                        type_is_externally_nameable(module, &target.value)
                    }
                }
        }
    }
}

fn value<'a>(
    annotations: &Annotations<'a>,
    public: bool,
    name: alder_ast::QualifiedName<'a>,
    kind: ValueKind,
    values: &mut Vec<InterfaceValue<'a>>,
    private_names: &mut Vec<PrivateName<'a>>,
) {
    if public {
        values.push(InterfaceValue {
            exported_as: name.name,
            identity: InterfaceValueIdentity::Binding(name),
            annotation: annotations[&name],
            kind,
        });
    } else {
        private(private_names, name.name, Namespace::Value);
    }
}

fn opaque_type<'a>(
    public: bool,
    name: alder_ast::QualifiedName<'a>,
    body: PublicTypeBody<'a>,
    types: &mut Vec<InterfaceType<'a>>,
    private_names: &mut Vec<PrivateName<'a>>,
) {
    if public {
        types.push(InterfaceType {
            exported_as: name.name,
            reference: name,
            params: &[],
            result_kind: Kind::Type,
            body,
        });
    } else {
        private(private_names, name.name, Namespace::Type);
    }
}

fn type_params<'a>(bump: &'a Bump, params: &'a [alder_ast::Name<'a>]) -> &'a [TypeParam<'a>] {
    bump.alloc_slice_fill_iter(params.iter().map(|param| TypeParam {
        name: *param,
        kind: Kind::Type,
    }))
}

fn private<'a>(names: &mut Vec<PrivateName<'a>>, name: &'a str, namespace: Namespace) {
    names.push(PrivateName { name, namespace });
}

fn impl_origin_index(origin: alder_ast::ImplOrigin) -> u32 {
    match origin {
        alder_ast::ImplOrigin::Source { item_ordinal } => item_ordinal,
        alder_ast::ImplOrigin::Derived {
            type_ordinal,
            derive_index,
        } => type_ordinal.saturating_mul(1_000) + u32::from(derive_index),
        alder_ast::ImplOrigin::AutomaticEq { type_ordinal } => type_ordinal,
        alder_ast::ImplOrigin::Builtin { index } => u32::from(index),
    }
}

#[cfg(test)]
mod tests {
    use super::{BUILTIN_TRAITS_SOURCE, BUILTIN_VALUE_SOURCES};

    #[test]
    fn packaged_builtin_values_match_the_workspace_stdlib() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std");
        if !root.is_dir() {
            return; // Published packages carry their own audited source copies.
        }
        for (name, source) in BUILTIN_VALUE_SOURCES {
            assert_eq!(
                std::fs::read_to_string(root.join(format!("{name}.ald"))).unwrap(),
                *source,
                "{name}"
            );
        }
    }

    #[test]
    fn every_packaged_builtin_value_has_a_canonical_signature() {
        let bump = bumpalo::Bump::new();
        for (name, _) in BUILTIN_VALUE_SOURCES {
            let module = alder_ast::ModuleId {
                package: alder_ast::PackageId::Builtin,
                path: bump.alloc_slice_copy(&[*name]),
            };
            assert!(
                !super::builtin_value_annotations(&bump, module).is_empty(),
                "{name}"
            );
        }
    }

    #[test]
    fn builtin_unbounded_is_a_monomorphic_number_value() {
        let bump = bumpalo::Bump::new();
        let module = alder_ast::ModuleId {
            package: alder_ast::PackageId::Builtin,
            path: &["Fiber"],
        };
        let annotations = super::builtin_value_annotations(&bump, module);
        let annotation = annotations["unbounded"];
        assert!(annotation.params.is_empty());
        let alder_ast::Type::Named { reference, args } = annotation.typ.value else {
            panic!("unbounded must be a Number value, not a function or options record");
        };
        assert_eq!(reference.name, "Number");
        assert_eq!(reference.module.package, alder_ast::PackageId::Builtin);
        assert!(args.is_empty());
    }

    #[test]
    fn builtin_type_interfaces_require_builtin_package_identity() {
        let bump = bumpalo::Bump::new();
        let module = alder_ast::ModuleId {
            package: alder_ast::PackageId::Builtin,
            path: &["Fiber"],
        };
        let interface = super::builtin_type_interface(&bump, module).unwrap();
        let options = interface
            .types
            .iter()
            .find(|typ| typ.exported_as == "MapOptions")
            .unwrap();
        assert_eq!(options.reference.module, module);
        assert!(matches!(options.body, alder_ast::PublicTypeBody::Alias(_)));
        assert!(
            super::builtin_type_interface(
                &bump,
                alder_ast::ModuleId {
                    package: alder_ast::PackageId::Application,
                    path: module.path,
                }
            )
            .is_none()
        );
    }

    #[test]
    fn builtin_signatures_expand_source_local_aliases() {
        let bump = bumpalo::Bump::new();
        let module = alder_ast::ModuleId {
            package: alder_ast::PackageId::Builtin,
            path: &["Fixture"],
        };
        let source = indoc::indoc! {r#"
            pub type Options = Settings[Number]
            type Settings[a] = { concurrency?: a }

            #[extern("alder:kernel", "$fixture")]
            pub fn configure(options?: Options) Options
        "#};
        let annotations = super::builtin_value_annotations_from_source(&bump, module, source);
        let annotation = annotations["configure"];
        let alder_ast::Type::Fn { params, ret } = annotation.typ.value else {
            panic!("expected a function signature");
        };
        let alder_ast::Type::Named { reference, args } = params[0].value else {
            panic!("optional parameter must have Option type");
        };
        assert_eq!(reference.name, "Option");
        assert_eq!(reference.module.package, alder_ast::PackageId::Builtin);
        for typ in [args[0], ret] {
            let alder_ast::Type::Alias { reference, .. } = typ.value else {
                panic!("source alias must retain its canonical identity");
            };
            assert_eq!(reference.module, module);
            assert_eq!(reference.name, "Options");
            let mut expanded = typ;
            while let alder_ast::Type::Alias { target, .. } = expanded.value {
                expanded = match target {
                    alder_ast::AliasType::Open(body) | alder_ast::AliasType::Filled(body) => body,
                };
            }
            let alder_ast::Type::Record { fields, .. } = expanded.value else {
                panic!("aliases must expand to the underlying record");
            };
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0].name, "concurrency");
            let alder_ast::Type::Named { reference, args } = fields[0].typ.value else {
                panic!("optional record shorthand must expand to Option");
            };
            assert_eq!(reference.name, "Option");
            assert_eq!(reference.module.package, alder_ast::PackageId::Builtin);
            assert_eq!(args.len(), 1);
            let alder_ast::Type::Named {
                reference,
                args: [],
            } = args[0].value
            else {
                panic!("generic private alias must substitute its argument");
            };
            assert_eq!(reference.name, "Number");
            assert_eq!(reference.module.package, alder_ast::PackageId::Builtin);
        }
    }

    #[test]
    fn packaged_builtin_traits_match_the_workspace_stdlib() {
        let workspace_source =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std/Traits.ald");
        if workspace_source.is_file() {
            assert_eq!(
                std::fs::read_to_string(workspace_source).unwrap(),
                BUILTIN_TRAITS_SOURCE
            );
        }
    }
}
