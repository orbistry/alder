use alder_ast::{
    DictionaryKind, Interface, InterfaceEnum, InterfaceImpl, InterfaceMethod, InterfaceModule,
    InterfaceTrait, InterfaceType, InterfaceValue, InterfaceValueIdentity, ItemKind, Kind,
    MethodImplementation, Module, Namespace, OpaqueKind, PrivateName, PublicTypeBody,
    ResolvedImportKind, TypeParam, ValueKind, Visibility,
};
use bumpalo::Bump;

use crate::Annotations;

const BUILTIN_TRAITS_SOURCE: &str = include_str!("../stdlib/Traits.ald");

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
    headers_from_module(bump, result.module)
}

/// Build the public, solved contract consumed by dependent modules.
pub fn from_module<'a>(
    bump: &'a Bump,
    module: &'a Module<'a>,
    annotations: &Annotations<'a>,
) -> Interface<'a> {
    interface_from_module(bump, module, Some(annotations))
}

/// Build the canonical declaration header used while collecting a package's
/// complete trait database. Value bindings are omitted until their inferred
/// schemes are available, while types, traits, and impl heads are complete.
pub fn headers_from_module<'a>(bump: &'a Bump, module: &'a Module<'a>) -> Interface<'a> {
    interface_from_module(bump, module, None)
}

fn interface_from_module<'a>(
    bump: &'a Bump,
    module: &'a Module<'a>,
    annotations: Option<&Annotations<'a>>,
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
                                default_symbol: method.body.is_some().then_some(method.name.value),
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
    use super::BUILTIN_TRAITS_SOURCE;

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
