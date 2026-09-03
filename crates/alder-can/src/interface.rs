use alder_ast::{
    Interface, InterfaceEnum, InterfaceMethod, InterfaceModule, InterfaceTrait, InterfaceType,
    InterfaceValue, InterfaceValueIdentity, ItemKind, Kind, Module, Namespace, OpaqueKind,
    PrivateName, PublicTypeBody, ResolvedImportKind, TypeParam, ValueKind, Visibility,
};
use bumpalo::Bump;

use crate::Annotations;

/// Build the public, solved contract consumed by dependent modules.
pub fn from_module<'a>(
    bump: &'a Bump,
    module: &'a Module<'a>,
    annotations: &Annotations<'a>,
) -> Interface<'a> {
    let mut values = Vec::new();
    let mut types = Vec::new();
    let mut enums = Vec::new();
    let mut traits = Vec::new();
    let mut modules = Vec::new();
    let mut private_names = Vec::new();

    for item in module.items {
        let public = matches!(item.value.visibility, Visibility::Public(_));
        match &item.value.kind {
            ItemKind::Fn(function) => value(
                annotations,
                public,
                function.name,
                ValueKind::Function,
                &mut values,
                &mut private_names,
            ),
            ItemKind::Let(decl) => {
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
            ItemKind::Component(component) => value(
                annotations,
                public,
                component.name,
                ValueKind::Component,
                &mut values,
                &mut private_names,
            ),
            ItemKind::Extern(alder_ast::ExternDecl::Fn { name, .. }) => value(
                annotations,
                public,
                *name,
                ValueKind::Extern,
                &mut values,
                &mut private_names,
            ),
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
            ItemKind::Impl(_)
            | ItemKind::Test(_)
            | ItemKind::Tests(_)
            | ItemKind::Macro(_)
            | ItemKind::Comptime(_) => {}
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
        instances: &[],
        modules: bump.alloc_slice_copy(&modules),
        private_names: bump.alloc_slice_copy(&private_names),
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
