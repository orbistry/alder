//! Deep-copy solved interfaces across module arena boundaries.

use alder_region::Located;
use bumpalo::Bump;

use crate::*;

pub fn copy_interface<'a>(bump: &'a Bump, interface: &Interface<'_>) -> Interface<'a> {
    Interface {
        home: copy_module_id(bump, interface.home),
        values: bump.alloc_slice_fill_iter(interface.values.iter().map(|value| InterfaceValue {
            exported_as: copy_str(bump, value.exported_as),
            identity: match value.identity {
                InterfaceValueIdentity::Binding(name) => {
                    InterfaceValueIdentity::Binding(copy_qualified_name(bump, name))
                }
                InterfaceValueIdentity::TraitMethod(method) => {
                    InterfaceValueIdentity::TraitMethod(copy_method_id(bump, method))
                }
            },
            annotation: copy_annotation(bump, value.annotation),
            kind: value.kind,
        })),
        types: bump.alloc_slice_fill_iter(interface.types.iter().map(|typ| InterfaceType {
            exported_as: copy_str(bump, typ.exported_as),
            reference: copy_qualified_name(bump, typ.reference),
            params: copy_type_params(bump, typ.params),
            result_kind: copy_kind(bump, typ.result_kind),
            body: match typ.body {
                PublicTypeBody::Alias(alias) => PublicTypeBody::Alias(copy_type_node(bump, alias)),
                PublicTypeBody::Opaque(kind) => PublicTypeBody::Opaque(kind),
                PublicTypeBody::ErrorGroup(tags) => {
                    PublicTypeBody::ErrorGroup(copy_error_tags(bump, tags))
                }
            },
        })),
        enums: bump.alloc_slice_fill_iter(interface.enums.iter().map(|enum_| InterfaceEnum {
            exported_as: copy_str(bump, enum_.exported_as),
            reference: copy_qualified_name(bump, enum_.reference),
            params: copy_type_params(bump, enum_.params),
            result_kind: copy_kind(bump, enum_.result_kind),
            variants: bump.alloc_slice_fill_iter(enum_.variants.iter().map(|variant| Variant {
                name: ConstructorName {
                    enum_: copy_qualified_name(bump, variant.name.enum_),
                    variant: copy_str(bump, variant.name.variant),
                },
                index: variant.index,
                alternatives: variant.alternatives,
                payload: copy_variant_payload(bump, variant.payload),
            })),
        })),
        traits: bump.alloc_slice_fill_iter(interface.traits.iter().map(|trait_| InterfaceTrait {
            exported_as: copy_str(bump, trait_.exported_as),
            id: copy_trait_id(bump, trait_.id),
            params: copy_type_params(bump, trait_.params),
            superclasses: copy_trait_refs(bump, trait_.superclasses),
            associated_types: bump.alloc_slice_fill_iter(trait_.associated_types.iter().map(
                |assoc| AssocTypeDecl {
                    id: copy_assoc_type_id(bump, assoc.id),
                    kind: copy_kind(bump, assoc.kind),
                    region: assoc.region,
                },
            )),
            methods: bump.alloc_slice_fill_iter(trait_.methods.iter().map(|method| {
                InterfaceMethod {
                    id: copy_method_id(bump, method.id),
                    exported_as: copy_str(bump, method.exported_as),
                    scheme: copy_annotation(bump, method.scheme),
                    has_default: method.has_default,
                    default_symbol: method.default_symbol.map(|name| copy_str(bump, name)),
                }
            })),
        })),
        instances: bump.alloc_slice_fill_iter(interface.instances.iter().map(|implementation| {
            InterfaceImpl {
                id: ImplId {
                    module: copy_module_id(bump, implementation.id.module),
                    origin: implementation.id.origin,
                },
                source_uri: implementation.source_uri.map(|uri| copy_str(bump, uri)),
                region: implementation.region,
                params: copy_type_params(bump, implementation.params),
                trait_ref: copy_trait_ref(bump, implementation.trait_ref),
                trait_predicates: copy_trait_refs(bump, implementation.trait_predicates),
                projection_equalities: bump.alloc_slice_fill_iter(
                    implementation
                        .projection_equalities
                        .iter()
                        .map(|equality| copy_projection_equality(bump, equality)),
                ),
                assoc_bindings: bump.alloc_slice_fill_iter(
                    implementation
                        .assoc_bindings
                        .iter()
                        .map(|binding| AssocBinding {
                            assoc: copy_assoc_type_id(bump, binding.assoc),
                            typ: copy_type_node(bump, binding.typ),
                            region: binding.region,
                        }),
                ),
                dictionary_symbol: copy_str(bump, implementation.dictionary_symbol),
                dictionary_kind: implementation.dictionary_kind,
                methods: bump.alloc_slice_fill_iter(implementation.methods.iter().map(
                    |(method, implementation)| {
                        (
                            copy_method_id(bump, *method),
                            match implementation {
                                MethodImplementation::Provided { symbol } => {
                                    MethodImplementation::Provided {
                                        symbol: copy_str(bump, symbol),
                                    }
                                }
                                MethodImplementation::Default { symbol } => {
                                    MethodImplementation::Default {
                                        symbol: copy_str(bump, symbol),
                                    }
                                }
                            },
                        )
                    },
                )),
            }
        })),
        modules: bump.alloc_slice_fill_iter(interface.modules.iter().map(|module| {
            InterfaceModule {
                exported_as: copy_str(bump, module.exported_as),
                module: copy_module_id(bump, module.module),
            }
        })),
        private_names: bump.alloc_slice_fill_iter(interface.private_names.iter().map(|name| {
            PrivateName {
                name: copy_str(bump, name.name),
                namespace: name.namespace,
            }
        })),
    }
}

fn copy_str<'a>(bump: &'a Bump, value: &str) -> &'a str {
    bump.alloc_str(value)
}

fn copy_package_id<'a>(bump: &'a Bump, package: PackageId<'_>) -> PackageId<'a> {
    match package {
        PackageId::Named(name) => PackageId::Named(PackageName {
            author: copy_str(bump, name.author),
            project: copy_str(bump, name.project),
        }),
        PackageId::Application => PackageId::Application,
        PackageId::ApplicationMember(member) => {
            PackageId::ApplicationMember(copy_str(bump, member))
        }
        PackageId::Builtin => PackageId::Builtin,
    }
}

fn copy_module_id<'a>(bump: &'a Bump, module: ModuleId<'_>) -> ModuleId<'a> {
    ModuleId {
        package: copy_package_id(bump, module.package),
        path: bump.alloc_slice_fill_iter(module.path.iter().map(|part| copy_str(bump, part))),
    }
}

fn copy_qualified_name<'a>(bump: &'a Bump, name: QualifiedName<'_>) -> QualifiedName<'a> {
    QualifiedName {
        module: copy_module_id(bump, name.module),
        name: copy_str(bump, name.name),
    }
}

fn copy_trait_id<'a>(bump: &'a Bump, id: TraitId<'_>) -> TraitId<'a> {
    TraitId(copy_qualified_name(bump, id.0))
}

fn copy_method_id<'a>(bump: &'a Bump, id: MethodId<'_>) -> MethodId<'a> {
    MethodId {
        trait_: copy_trait_id(bump, id.trait_),
        index: id.index,
        name: copy_str(bump, id.name),
    }
}

fn copy_assoc_type_id<'a>(bump: &'a Bump, id: AssocTypeId<'_>) -> AssocTypeId<'a> {
    AssocTypeId {
        trait_: copy_trait_id(bump, id.trait_),
        index: id.index,
        name: copy_str(bump, id.name),
    }
}

fn copy_kind<'a>(bump: &'a Bump, kind: Kind<'_>) -> Kind<'a> {
    match kind {
        Kind::Type => Kind::Type,
        Kind::Arrow { param, result } => Kind::Arrow {
            param: bump.alloc(copy_kind(bump, *param)),
            result: bump.alloc(copy_kind(bump, *result)),
        },
    }
}

fn copy_type_params<'a>(bump: &'a Bump, params: &[TypeParam<'_>]) -> &'a [TypeParam<'a>] {
    bump.alloc_slice_fill_iter(params.iter().map(|param| TypeParam {
        name: Located::at(param.name.region, copy_str(bump, param.name.value)),
        kind: copy_kind(bump, param.kind),
    }))
}

fn copy_trait_ref<'a>(bump: &'a Bump, trait_ref: TraitRef<'_>) -> TraitRef<'a> {
    TraitRef {
        trait_: copy_trait_id(bump, trait_ref.trait_),
        args: copy_type_nodes(bump, trait_ref.args),
    }
}

fn copy_trait_refs<'a>(bump: &'a Bump, refs: &[TraitRef<'_>]) -> &'a [TraitRef<'a>] {
    bump.alloc_slice_fill_iter(
        refs.iter()
            .map(|trait_ref| copy_trait_ref(bump, *trait_ref)),
    )
}

fn copy_projection<'a>(bump: &'a Bump, projection: ProjectionType<'_>) -> ProjectionType<'a> {
    ProjectionType {
        trait_ref: copy_trait_ref(bump, projection.trait_ref),
        assoc: copy_assoc_type_id(bump, projection.assoc),
    }
}

fn copy_projection_equality<'a>(
    bump: &'a Bump,
    equality: &ProjectionEquality<'_>,
) -> ProjectionEquality<'a> {
    ProjectionEquality {
        projection: copy_projection(bump, equality.projection),
        typ: copy_type_node(bump, equality.typ),
        region: equality.region,
    }
}

fn copy_annotation<'a>(bump: &'a Bump, annotation: &Annotation<'_>) -> &'a Annotation<'a> {
    bump.alloc(Annotation {
        params: copy_type_params(bump, annotation.params),
        trait_predicates: copy_trait_refs(bump, annotation.trait_predicates),
        projection_equalities: bump.alloc_slice_fill_iter(
            annotation
                .projection_equalities
                .iter()
                .map(|equality| copy_projection_equality(bump, equality)),
        ),
        typ: copy_type_node(bump, annotation.typ),
    })
}

fn copy_type_nodes<'a>(bump: &'a Bump, nodes: &[Node<'_, Type<'_>>]) -> &'a [Node<'a, Type<'a>>] {
    bump.alloc_slice_fill_iter(nodes.iter().map(|node| copy_type_node(bump, node)))
}

fn copy_type_node<'a>(bump: &'a Bump, node: Node<'_, Type<'_>>) -> Node<'a, Type<'a>> {
    bump.alloc(Located::at(node.region, copy_type(bump, &node.value)))
}

fn copy_type<'a>(bump: &'a Bump, typ: &Type<'_>) -> Type<'a> {
    match typ {
        Type::Var { name, args } => Type::Var {
            name: copy_str(bump, name),
            args: copy_type_nodes(bump, args),
        },
        Type::Named { reference, args } => Type::Named {
            reference: copy_qualified_name(bump, *reference),
            args: copy_type_nodes(bump, args),
        },
        Type::Partial { constructor, slots } => Type::Partial {
            constructor: copy_qualified_name(bump, *constructor),
            slots: bump.alloc_slice_fill_iter(slots.iter().map(|slot| match slot {
                TypeSlot::Hole(index) => TypeSlot::Hole(*index),
                TypeSlot::Fixed(typ) => TypeSlot::Fixed(copy_type_node(bump, typ)),
            })),
        },
        Type::Projection(projection) => Type::Projection(copy_projection(bump, *projection)),
        Type::Fn { params, ret } => Type::Fn {
            params: copy_type_nodes(bump, params),
            ret: copy_type_node(bump, ret),
        },
        Type::Unit => Type::Unit,
        Type::Tuple(items) => Type::Tuple(copy_type_nodes(bump, items)),
        Type::Record { fields, ext } => Type::Record {
            fields: bump.alloc_slice_fill_iter(fields.iter().map(|field| RecordTypeField {
                index: field.index,
                name: copy_str(bump, field.name),
                presence: field.presence,
                typ: copy_type_node(bump, field.typ),
            })),
            ext: copy_row_extension(bump, *ext),
        },
        Type::ErrorRow { tags, ext } => Type::ErrorRow {
            tags: copy_error_tags(bump, tags),
            ext: copy_row_extension(bump, *ext),
        },
        Type::Alias {
            reference,
            arguments,
            target,
        } => Type::Alias {
            reference: copy_qualified_name(bump, *reference),
            arguments: bump.alloc_slice_fill_iter(arguments.iter().map(|argument| AliasArgument {
                name: copy_str(bump, argument.name),
                typ: copy_type_node(bump, argument.typ),
            })),
            target: match target {
                AliasType::Open(typ) => AliasType::Open(copy_type_node(bump, typ)),
                AliasType::Filled(typ) => AliasType::Filled(copy_type_node(bump, typ)),
            },
        },
    }
}

fn copy_row_extension<'a>(bump: &'a Bump, ext: RowExtension<'_>) -> RowExtension<'a> {
    match ext {
        RowExtension::Closed => RowExtension::Closed,
        RowExtension::Open(name) => RowExtension::Open(copy_str(bump, name)),
    }
}

fn copy_error_tags<'a>(bump: &'a Bump, tags: &[ErrorTagType<'_>]) -> &'a [ErrorTagType<'a>] {
    bump.alloc_slice_fill_iter(tags.iter().map(|tag| ErrorTagType {
        index: tag.index,
        name: copy_str(bump, tag.name),
        args: copy_type_nodes(bump, tag.args),
    }))
}

fn copy_variant_payload<'a>(bump: &'a Bump, payload: VariantPayload<'_>) -> VariantPayload<'a> {
    match payload {
        VariantPayload::Unit => VariantPayload::Unit,
        VariantPayload::Tuple(types) => VariantPayload::Tuple(copy_type_nodes(bump, types)),
        VariantPayload::Record(fields) => {
            VariantPayload::Record(bump.alloc_slice_fill_iter(fields.iter().map(|field| {
                RecordTypeField {
                    index: field.index,
                    name: copy_str(bump, field.name),
                    presence: field.presence,
                    typ: copy_type_node(bump, field.typ),
                }
            })))
        }
    }
}
