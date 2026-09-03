use alder_ast as ast;
use alder_region::{Located, Region};
use bumpalo::Bump;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum OwnedPackageId {
    Named { author: String, project: String },
    Application,
    ApplicationMember(String),
    Builtin,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OwnedModuleId {
    pub package: OwnedPackageId,
    pub path: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OwnedQualifiedName {
    pub module: OwnedModuleId,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OwnedTraitId(pub OwnedQualifiedName);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OwnedMethodId {
    pub trait_: OwnedTraitId,
    pub index: u16,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OwnedAssocTypeId {
    pub trait_: OwnedTraitId,
    pub index: u16,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum OwnedImplOrigin {
    Source {
        item_ordinal: u32,
    },
    Derived {
        type_ordinal: u32,
        derive_index: u16,
    },
    AutomaticEq {
        type_ordinal: u32,
    },
    Builtin {
        index: u16,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OwnedImplId {
    pub module: OwnedModuleId,
    pub origin: OwnedImplOrigin,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OwnedKind {
    Type,
    Arrow(Box<OwnedKind>, Box<OwnedKind>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedTypeParam {
    pub name: String,
    pub region: Region,
    pub kind: OwnedKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedTraitRef {
    pub trait_: OwnedTraitId,
    pub args: Vec<OwnedLocatedType>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedProjection {
    pub trait_ref: OwnedTraitRef,
    pub assoc: OwnedAssocTypeId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedProjectionEquality {
    pub projection: OwnedProjection,
    pub typ: OwnedLocatedType,
    pub region: Region,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedScheme {
    pub params: Vec<OwnedTypeParam>,
    pub trait_predicates: Vec<OwnedTraitRef>,
    pub projection_equalities: Vec<OwnedProjectionEquality>,
    pub typ: OwnedLocatedType,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedLocatedType {
    pub region: Region,
    pub typ: OwnedType,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OwnedType {
    Var {
        name: String,
        args: Vec<OwnedLocatedType>,
    },
    Named {
        reference: OwnedQualifiedName,
        args: Vec<OwnedLocatedType>,
    },
    Partial {
        constructor: OwnedQualifiedName,
        slots: Vec<OwnedTypeSlot>,
    },
    Projection(OwnedProjection),
    Fn {
        params: Vec<OwnedLocatedType>,
        ret: Box<OwnedLocatedType>,
    },
    Unit,
    Tuple(Vec<OwnedLocatedType>),
    Record {
        fields: Vec<OwnedRecordField>,
        ext: Option<String>,
    },
    ErrorRow {
        tags: Vec<OwnedErrorTag>,
        ext: Option<String>,
    },
    Alias {
        reference: OwnedQualifiedName,
        arguments: Vec<OwnedAliasArgument>,
        target: Box<OwnedLocatedType>,
        filled: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OwnedTypeSlot {
    Hole(u16),
    Fixed(Box<OwnedLocatedType>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedAliasArgument {
    pub name: String,
    pub typ: OwnedLocatedType,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedRecordField {
    pub index: u16,
    pub name: String,
    pub optional: bool,
    pub typ: OwnedLocatedType,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedErrorTag {
    pub index: u16,
    pub name: String,
    pub args: Vec<OwnedLocatedType>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OwnedValueKind {
    Function,
    Let,
    Component,
    Table,
    Schema,
    Extern,
    TraitMethod,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OwnedValueIdentity {
    Binding(OwnedQualifiedName),
    TraitMethod(OwnedMethodId),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedValue {
    pub exported_as: String,
    pub identity: OwnedValueIdentity,
    pub scheme: OwnedScheme,
    pub kind: OwnedValueKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedModuleExport {
    pub exported_as: String,
    pub module: OwnedModuleId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OwnedNamespace {
    Value,
    Type,
    Enum,
    Constructor,
    Trait,
    Module,
    Provider,
    AssociatedItem,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedPrivateName {
    pub name: String,
    pub namespace: OwnedNamespace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OwnedOpaqueKind {
    Extern,
    Table,
    Schema,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedTypeDecl {
    pub exported_as: String,
    pub reference: OwnedQualifiedName,
    pub params: Vec<OwnedTypeParam>,
    pub result_kind: OwnedKind,
    pub body: OwnedPublicTypeBody,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OwnedPublicTypeBody {
    Alias(Box<OwnedLocatedType>),
    Opaque(OwnedOpaqueKind),
    Enum(Vec<OwnedVariant>),
    ErrorGroup(Vec<OwnedErrorTag>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedVariant {
    pub name: String,
    pub index: u16,
    pub alternatives: u16,
    pub payload: OwnedVariantPayload,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OwnedVariantPayload {
    Unit,
    Tuple(Vec<OwnedLocatedType>),
    Record(Vec<OwnedRecordField>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedAssocType {
    pub id: OwnedAssocTypeId,
    pub kind: OwnedKind,
    pub region: Region,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedMethod {
    pub id: OwnedMethodId,
    pub exported_as: String,
    pub scheme: OwnedScheme,
    pub has_default: bool,
    pub default_symbol: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedTrait {
    pub exported_as: String,
    pub id: OwnedTraitId,
    pub params: Vec<OwnedTypeParam>,
    pub superclasses: Vec<OwnedTraitRef>,
    pub associated_types: Vec<OwnedAssocType>,
    pub methods: Vec<OwnedMethod>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OwnedDictionaryKind {
    Singleton,
    Factory,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OwnedMethodImplementation {
    Provided { symbol: String },
    Default { symbol: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedAssocBinding {
    pub assoc: OwnedAssocTypeId,
    pub typ: OwnedLocatedType,
    pub region: Region,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedImplHeader {
    pub id: OwnedImplId,
    pub source_uri: Option<String>,
    pub region: Option<Region>,
    pub params: Vec<OwnedTypeParam>,
    pub trait_ref: OwnedTraitRef,
    pub trait_predicates: Vec<OwnedTraitRef>,
    pub projection_equalities: Vec<OwnedProjectionEquality>,
    pub assoc_bindings: Vec<OwnedAssocBinding>,
    pub dictionary_symbol: String,
    pub dictionary_kind: OwnedDictionaryKind,
    pub methods: Vec<(OwnedMethodId, OwnedMethodImplementation)>,
}

pub(crate) fn own_interface(interface: &ast::Interface<'_>) -> super::InterfaceFile {
    let mut types = interface
        .types
        .iter()
        .map(own_type_decl)
        .collect::<Vec<_>>();
    types.extend(interface.enums.iter().map(own_enum_decl));
    super::InterfaceFile {
        format_version: super::INTERFACE_FORMAT_VERSION,
        compiler_version: env!("CARGO_PKG_VERSION").to_owned(),
        module: own_module_id(interface.home),
        values: interface.values.iter().map(own_value).collect(),
        types,
        traits: interface.traits.iter().map(own_trait).collect(),
        instances: interface.instances.iter().map(own_impl).collect(),
        modules: interface
            .modules
            .iter()
            .map(|module| OwnedModuleExport {
                exported_as: module.exported_as.to_owned(),
                module: own_module_id(module.module),
            })
            .collect(),
        private_names: interface
            .private_names
            .iter()
            .map(|name| OwnedPrivateName {
                name: name.name.to_owned(),
                namespace: own_namespace(name.namespace),
            })
            .collect(),
        fingerprint: [0; 32],
    }
}

fn own_value(value: &ast::InterfaceValue<'_>) -> OwnedValue {
    OwnedValue {
        exported_as: value.exported_as.to_owned(),
        identity: match value.identity {
            ast::InterfaceValueIdentity::Binding(name) => {
                OwnedValueIdentity::Binding(own_qualified_name(name))
            }
            ast::InterfaceValueIdentity::TraitMethod(method) => {
                OwnedValueIdentity::TraitMethod(own_method_id(method))
            }
        },
        scheme: own_scheme(value.annotation),
        kind: own_value_kind(value.kind),
    }
}

fn own_type_decl(typ: &ast::InterfaceType<'_>) -> OwnedTypeDecl {
    OwnedTypeDecl {
        exported_as: typ.exported_as.to_owned(),
        reference: own_qualified_name(typ.reference),
        params: typ.params.iter().map(own_type_param).collect(),
        result_kind: own_kind(typ.result_kind),
        body: match typ.body {
            ast::PublicTypeBody::Alias(alias) => {
                OwnedPublicTypeBody::Alias(Box::new(own_type_node(alias)))
            }
            ast::PublicTypeBody::Opaque(kind) => OwnedPublicTypeBody::Opaque(own_opaque(kind)),
            ast::PublicTypeBody::ErrorGroup(tags) => {
                OwnedPublicTypeBody::ErrorGroup(tags.iter().map(own_error_tag).collect())
            }
        },
    }
}

fn own_enum_decl(enum_: &ast::InterfaceEnum<'_>) -> OwnedTypeDecl {
    OwnedTypeDecl {
        exported_as: enum_.exported_as.to_owned(),
        reference: own_qualified_name(enum_.reference),
        params: enum_.params.iter().map(own_type_param).collect(),
        result_kind: own_kind(enum_.result_kind),
        body: OwnedPublicTypeBody::Enum(
            enum_
                .variants
                .iter()
                .map(|variant| OwnedVariant {
                    name: variant.name.variant.to_owned(),
                    index: variant.index,
                    alternatives: variant.alternatives,
                    payload: own_variant_payload(variant.payload),
                })
                .collect(),
        ),
    }
}

fn own_trait(trait_: &ast::InterfaceTrait<'_>) -> OwnedTrait {
    OwnedTrait {
        exported_as: trait_.exported_as.to_owned(),
        id: own_trait_id(trait_.id),
        params: trait_.params.iter().map(own_type_param).collect(),
        superclasses: trait_.superclasses.iter().map(own_trait_ref).collect(),
        associated_types: trait_
            .associated_types
            .iter()
            .map(|assoc| OwnedAssocType {
                id: own_assoc_id(assoc.id),
                kind: own_kind(assoc.kind),
                region: assoc.region,
            })
            .collect(),
        methods: trait_
            .methods
            .iter()
            .map(|method| OwnedMethod {
                id: own_method_id(method.id),
                exported_as: method.exported_as.to_owned(),
                scheme: own_scheme(method.scheme),
                has_default: method.has_default,
                default_symbol: method.default_symbol.map(str::to_owned),
            })
            .collect(),
    }
}

fn own_impl(implementation: &ast::InterfaceImpl<'_>) -> OwnedImplHeader {
    OwnedImplHeader {
        id: OwnedImplId {
            module: own_module_id(implementation.id.module),
            origin: own_impl_origin(implementation.id.origin),
        },
        source_uri: implementation.source_uri.map(str::to_owned),
        region: implementation.region,
        params: implementation.params.iter().map(own_type_param).collect(),
        trait_ref: own_trait_ref(&implementation.trait_ref),
        trait_predicates: implementation
            .trait_predicates
            .iter()
            .map(own_trait_ref)
            .collect(),
        projection_equalities: implementation
            .projection_equalities
            .iter()
            .map(own_projection_equality)
            .collect(),
        assoc_bindings: implementation
            .assoc_bindings
            .iter()
            .map(|binding| OwnedAssocBinding {
                assoc: own_assoc_id(binding.assoc),
                typ: own_type_node(binding.typ),
                region: binding.region,
            })
            .collect(),
        dictionary_symbol: implementation.dictionary_symbol.to_owned(),
        dictionary_kind: match implementation.dictionary_kind {
            ast::DictionaryKind::Singleton => OwnedDictionaryKind::Singleton,
            ast::DictionaryKind::Factory => OwnedDictionaryKind::Factory,
        },
        methods: implementation
            .methods
            .iter()
            .map(|(method, implementation)| {
                (
                    own_method_id(*method),
                    match implementation {
                        ast::MethodImplementation::Provided { symbol } => {
                            OwnedMethodImplementation::Provided {
                                symbol: (*symbol).to_owned(),
                            }
                        }
                        ast::MethodImplementation::Default { symbol } => {
                            OwnedMethodImplementation::Default {
                                symbol: (*symbol).to_owned(),
                            }
                        }
                    },
                )
            })
            .collect(),
    }
}

fn own_scheme(annotation: &ast::Annotation<'_>) -> OwnedScheme {
    OwnedScheme {
        params: annotation.params.iter().map(own_type_param).collect(),
        trait_predicates: annotation
            .trait_predicates
            .iter()
            .map(own_trait_ref)
            .collect(),
        projection_equalities: annotation
            .projection_equalities
            .iter()
            .map(own_projection_equality)
            .collect(),
        typ: own_type_node(annotation.typ),
    }
}

fn own_projection_equality(equality: &ast::ProjectionEquality<'_>) -> OwnedProjectionEquality {
    OwnedProjectionEquality {
        projection: own_projection(equality.projection),
        typ: own_type_node(equality.typ),
        region: equality.region,
    }
}

fn own_type_node(typ: ast::Node<'_, ast::Type<'_>>) -> OwnedLocatedType {
    OwnedLocatedType {
        region: typ.region,
        typ: own_type(&typ.value),
    }
}

fn own_type(typ: &ast::Type<'_>) -> OwnedType {
    match typ {
        ast::Type::Var { name, args } => OwnedType::Var {
            name: (*name).to_owned(),
            args: args.iter().map(|typ| own_type_node(typ)).collect(),
        },
        ast::Type::Named { reference, args } => OwnedType::Named {
            reference: own_qualified_name(*reference),
            args: args.iter().map(|typ| own_type_node(typ)).collect(),
        },
        ast::Type::Partial { constructor, slots } => OwnedType::Partial {
            constructor: own_qualified_name(*constructor),
            slots: slots
                .iter()
                .map(|slot| match slot {
                    ast::TypeSlot::Hole(index) => OwnedTypeSlot::Hole(*index),
                    ast::TypeSlot::Fixed(typ) => OwnedTypeSlot::Fixed(Box::new(own_type_node(typ))),
                })
                .collect(),
        },
        ast::Type::Projection(projection) => OwnedType::Projection(own_projection(*projection)),
        ast::Type::Fn { params, ret } => OwnedType::Fn {
            params: params.iter().map(|typ| own_type_node(typ)).collect(),
            ret: Box::new(own_type_node(ret)),
        },
        ast::Type::Unit => OwnedType::Unit,
        ast::Type::Tuple(items) => {
            OwnedType::Tuple(items.iter().map(|typ| own_type_node(typ)).collect())
        }
        ast::Type::Record { fields, ext } => OwnedType::Record {
            fields: fields.iter().map(own_record_field).collect(),
            ext: own_row_extension(*ext),
        },
        ast::Type::ErrorRow { tags, ext } => OwnedType::ErrorRow {
            tags: tags.iter().map(own_error_tag).collect(),
            ext: own_row_extension(*ext),
        },
        ast::Type::Alias {
            reference,
            arguments,
            target,
        } => {
            let (target, filled) = match target {
                ast::AliasType::Open(typ) => (typ, false),
                ast::AliasType::Filled(typ) => (typ, true),
            };
            OwnedType::Alias {
                reference: own_qualified_name(*reference),
                arguments: arguments
                    .iter()
                    .map(|argument| OwnedAliasArgument {
                        name: argument.name.to_owned(),
                        typ: own_type_node(argument.typ),
                    })
                    .collect(),
                target: Box::new(own_type_node(target)),
                filled,
            }
        }
    }
}

fn own_record_field(field: &ast::RecordTypeField<'_>) -> OwnedRecordField {
    OwnedRecordField {
        index: field.index,
        name: field.name.to_owned(),
        optional: field.presence == ast::FieldPresence::Optional,
        typ: own_type_node(field.typ),
    }
}

fn own_error_tag(tag: &ast::ErrorTagType<'_>) -> OwnedErrorTag {
    OwnedErrorTag {
        index: tag.index,
        name: tag.name.to_owned(),
        args: tag.args.iter().map(|typ| own_type_node(typ)).collect(),
    }
}

fn own_variant_payload(payload: ast::VariantPayload<'_>) -> OwnedVariantPayload {
    match payload {
        ast::VariantPayload::Unit => OwnedVariantPayload::Unit,
        ast::VariantPayload::Tuple(types) => {
            OwnedVariantPayload::Tuple(types.iter().map(|typ| own_type_node(typ)).collect())
        }
        ast::VariantPayload::Record(fields) => {
            OwnedVariantPayload::Record(fields.iter().map(own_record_field).collect())
        }
    }
}

fn own_trait_ref(trait_ref: &ast::TraitRef<'_>) -> OwnedTraitRef {
    OwnedTraitRef {
        trait_: own_trait_id(trait_ref.trait_),
        args: trait_ref
            .args
            .iter()
            .map(|typ| own_type_node(typ))
            .collect(),
    }
}

fn own_projection(projection: ast::ProjectionType<'_>) -> OwnedProjection {
    OwnedProjection {
        trait_ref: own_trait_ref(&projection.trait_ref),
        assoc: own_assoc_id(projection.assoc),
    }
}

fn own_type_param(param: &ast::TypeParam<'_>) -> OwnedTypeParam {
    OwnedTypeParam {
        name: param.name.value.to_owned(),
        region: param.name.region,
        kind: own_kind(param.kind),
    }
}

fn own_kind(kind: ast::Kind<'_>) -> OwnedKind {
    match kind {
        ast::Kind::Type => OwnedKind::Type,
        ast::Kind::Arrow { param, result } => {
            OwnedKind::Arrow(Box::new(own_kind(*param)), Box::new(own_kind(*result)))
        }
    }
}

fn own_module_id(module: ast::ModuleId<'_>) -> OwnedModuleId {
    OwnedModuleId {
        package: match module.package {
            ast::PackageId::Named(package) => OwnedPackageId::Named {
                author: package.author.to_owned(),
                project: package.project.to_owned(),
            },
            ast::PackageId::Application => OwnedPackageId::Application,
            ast::PackageId::ApplicationMember(member) => {
                OwnedPackageId::ApplicationMember(member.to_owned())
            }
            ast::PackageId::Builtin => OwnedPackageId::Builtin,
        },
        path: module.path.iter().map(|part| (*part).to_owned()).collect(),
    }
}

fn own_qualified_name(name: ast::QualifiedName<'_>) -> OwnedQualifiedName {
    OwnedQualifiedName {
        module: own_module_id(name.module),
        name: name.name.to_owned(),
    }
}

fn own_trait_id(id: ast::TraitId<'_>) -> OwnedTraitId {
    OwnedTraitId(own_qualified_name(id.0))
}

fn own_method_id(id: ast::MethodId<'_>) -> OwnedMethodId {
    OwnedMethodId {
        trait_: own_trait_id(id.trait_),
        index: id.index,
        name: id.name.to_owned(),
    }
}

fn own_assoc_id(id: ast::AssocTypeId<'_>) -> OwnedAssocTypeId {
    OwnedAssocTypeId {
        trait_: own_trait_id(id.trait_),
        index: id.index,
        name: id.name.to_owned(),
    }
}

fn own_impl_origin(origin: ast::ImplOrigin) -> OwnedImplOrigin {
    match origin {
        ast::ImplOrigin::Source { item_ordinal } => OwnedImplOrigin::Source { item_ordinal },
        ast::ImplOrigin::Derived {
            type_ordinal,
            derive_index,
        } => OwnedImplOrigin::Derived {
            type_ordinal,
            derive_index,
        },
        ast::ImplOrigin::AutomaticEq { type_ordinal } => {
            OwnedImplOrigin::AutomaticEq { type_ordinal }
        }
        ast::ImplOrigin::Builtin { index } => OwnedImplOrigin::Builtin { index },
    }
}

fn own_row_extension(ext: ast::RowExtension<'_>) -> Option<String> {
    match ext {
        ast::RowExtension::Closed => None,
        ast::RowExtension::Open(name) => Some(name.to_owned()),
    }
}

fn own_value_kind(kind: ast::ValueKind) -> OwnedValueKind {
    match kind {
        ast::ValueKind::Function => OwnedValueKind::Function,
        ast::ValueKind::Let => OwnedValueKind::Let,
        ast::ValueKind::Component => OwnedValueKind::Component,
        ast::ValueKind::Table => OwnedValueKind::Table,
        ast::ValueKind::Schema => OwnedValueKind::Schema,
        ast::ValueKind::Extern => OwnedValueKind::Extern,
        ast::ValueKind::TraitMethod => OwnedValueKind::TraitMethod,
    }
}

fn own_namespace(namespace: ast::Namespace) -> OwnedNamespace {
    match namespace {
        ast::Namespace::Value => OwnedNamespace::Value,
        ast::Namespace::Type => OwnedNamespace::Type,
        ast::Namespace::Enum => OwnedNamespace::Enum,
        ast::Namespace::Constructor => OwnedNamespace::Constructor,
        ast::Namespace::Trait => OwnedNamespace::Trait,
        ast::Namespace::Module => OwnedNamespace::Module,
        ast::Namespace::Provider => OwnedNamespace::Provider,
        ast::Namespace::AssociatedItem => OwnedNamespace::AssociatedItem,
    }
}

fn own_opaque(kind: ast::OpaqueKind) -> OwnedOpaqueKind {
    match kind {
        ast::OpaqueKind::Extern => OwnedOpaqueKind::Extern,
        ast::OpaqueKind::Table => OwnedOpaqueKind::Table,
        ast::OpaqueKind::Schema => OwnedOpaqueKind::Schema,
    }
}

pub(crate) fn hydrate_interface<'a>(
    bump: &'a Bump,
    interface: &super::InterfaceFile,
) -> ast::Interface<'a> {
    let ordinary_types = interface
        .types
        .iter()
        .filter(|typ| !matches!(typ.body, OwnedPublicTypeBody::Enum(_)))
        .collect::<Vec<_>>();
    let enums = interface
        .types
        .iter()
        .filter(|typ| matches!(typ.body, OwnedPublicTypeBody::Enum(_)))
        .collect::<Vec<_>>();
    ast::Interface {
        home: hydrate_module_id(bump, &interface.module),
        values: bump.alloc_slice_fill_iter(interface.values.iter().map(|value| {
            ast::InterfaceValue {
                exported_as: bump.alloc_str(&value.exported_as),
                identity: match &value.identity {
                    OwnedValueIdentity::Binding(name) => {
                        ast::InterfaceValueIdentity::Binding(hydrate_qualified_name(bump, name))
                    }
                    OwnedValueIdentity::TraitMethod(method) => {
                        ast::InterfaceValueIdentity::TraitMethod(hydrate_method_id(bump, method))
                    }
                },
                annotation: hydrate_scheme(bump, &value.scheme),
                kind: hydrate_value_kind(value.kind),
            }
        })),
        types: bump.alloc_slice_fill_iter(ordinary_types.into_iter().map(|typ| {
            ast::InterfaceType {
                exported_as: bump.alloc_str(&typ.exported_as),
                reference: hydrate_qualified_name(bump, &typ.reference),
                params: hydrate_type_params(bump, &typ.params),
                result_kind: hydrate_kind(bump, &typ.result_kind),
                body: match &typ.body {
                    OwnedPublicTypeBody::Alias(alias) => {
                        ast::PublicTypeBody::Alias(hydrate_type_node(bump, alias))
                    }
                    OwnedPublicTypeBody::Opaque(kind) => {
                        ast::PublicTypeBody::Opaque(hydrate_opaque(*kind))
                    }
                    OwnedPublicTypeBody::ErrorGroup(tags) => {
                        ast::PublicTypeBody::ErrorGroup(hydrate_error_tags(bump, tags))
                    }
                    OwnedPublicTypeBody::Enum(_) => unreachable!("filtered above"),
                },
            }
        })),
        enums: bump.alloc_slice_fill_iter(enums.into_iter().map(|enum_| {
            let OwnedPublicTypeBody::Enum(variants) = &enum_.body else {
                unreachable!("filtered above")
            };
            let reference = hydrate_qualified_name(bump, &enum_.reference);
            ast::InterfaceEnum {
                exported_as: bump.alloc_str(&enum_.exported_as),
                reference,
                params: hydrate_type_params(bump, &enum_.params),
                result_kind: hydrate_kind(bump, &enum_.result_kind),
                variants: bump.alloc_slice_fill_iter(variants.iter().map(|variant| ast::Variant {
                    name: ast::ConstructorName {
                        enum_: reference,
                        variant: bump.alloc_str(&variant.name),
                    },
                    index: variant.index,
                    alternatives: variant.alternatives,
                    payload: hydrate_variant_payload(bump, &variant.payload),
                })),
            }
        })),
        traits: bump.alloc_slice_fill_iter(interface.traits.iter().map(|trait_| {
            ast::InterfaceTrait {
                exported_as: bump.alloc_str(&trait_.exported_as),
                id: hydrate_trait_id(bump, &trait_.id),
                params: hydrate_type_params(bump, &trait_.params),
                superclasses: hydrate_trait_refs(bump, &trait_.superclasses),
                associated_types: bump.alloc_slice_fill_iter(trait_.associated_types.iter().map(
                    |assoc| ast::AssocTypeDecl {
                        id: hydrate_assoc_id(bump, &assoc.id),
                        kind: hydrate_kind(bump, &assoc.kind),
                        region: assoc.region,
                    },
                )),
                methods: bump.alloc_slice_fill_iter(trait_.methods.iter().map(|method| {
                    ast::InterfaceMethod {
                        id: hydrate_method_id(bump, &method.id),
                        exported_as: bump.alloc_str(&method.exported_as),
                        scheme: hydrate_scheme(bump, &method.scheme),
                        has_default: method.has_default,
                        default_symbol: method
                            .default_symbol
                            .as_ref()
                            .map(|symbol| bump.alloc_str(symbol) as &str),
                    }
                })),
            }
        })),
        instances: bump.alloc_slice_fill_iter(
            interface
                .instances
                .iter()
                .map(|implementation| hydrate_impl(bump, implementation)),
        ),
        modules: bump.alloc_slice_fill_iter(interface.modules.iter().map(|module| {
            ast::InterfaceModule {
                exported_as: bump.alloc_str(&module.exported_as),
                module: hydrate_module_id(bump, &module.module),
            }
        })),
        private_names: bump.alloc_slice_fill_iter(interface.private_names.iter().map(|name| {
            ast::PrivateName {
                name: bump.alloc_str(&name.name),
                namespace: hydrate_namespace(name.namespace),
            }
        })),
    }
}

pub(crate) fn hydrate_impl<'a>(
    bump: &'a Bump,
    implementation: &OwnedImplHeader,
) -> ast::InterfaceImpl<'a> {
    ast::InterfaceImpl {
        id: ast::ImplId {
            module: hydrate_module_id(bump, &implementation.id.module),
            origin: hydrate_impl_origin(implementation.id.origin),
        },
        source_uri: implementation
            .source_uri
            .as_ref()
            .map(|uri| bump.alloc_str(uri) as &str),
        region: implementation.region,
        params: hydrate_type_params(bump, &implementation.params),
        trait_ref: hydrate_trait_ref(bump, &implementation.trait_ref),
        trait_predicates: hydrate_trait_refs(bump, &implementation.trait_predicates),
        projection_equalities: bump.alloc_slice_fill_iter(
            implementation
                .projection_equalities
                .iter()
                .map(|equality| hydrate_projection_equality(bump, equality)),
        ),
        assoc_bindings: bump.alloc_slice_fill_iter(implementation.assoc_bindings.iter().map(
            |binding| ast::AssocBinding {
                assoc: hydrate_assoc_id(bump, &binding.assoc),
                typ: hydrate_type_node(bump, &binding.typ),
                region: binding.region,
            },
        )),
        dictionary_symbol: bump.alloc_str(&implementation.dictionary_symbol),
        dictionary_kind: match implementation.dictionary_kind {
            OwnedDictionaryKind::Singleton => ast::DictionaryKind::Singleton,
            OwnedDictionaryKind::Factory => ast::DictionaryKind::Factory,
        },
        methods: bump.alloc_slice_fill_iter(implementation.methods.iter().map(
            |(method, implementation)| {
                (
                    hydrate_method_id(bump, method),
                    match implementation {
                        OwnedMethodImplementation::Provided { symbol } => {
                            ast::MethodImplementation::Provided {
                                symbol: bump.alloc_str(symbol),
                            }
                        }
                        OwnedMethodImplementation::Default { symbol } => {
                            ast::MethodImplementation::Default {
                                symbol: bump.alloc_str(symbol),
                            }
                        }
                    },
                )
            },
        )),
    }
}

fn hydrate_type_node<'a>(bump: &'a Bump, typ: &OwnedLocatedType) -> ast::Node<'a, ast::Type<'a>> {
    bump.alloc(Located::at(typ.region, hydrate_type(bump, &typ.typ)))
}

fn hydrate_type<'a>(bump: &'a Bump, typ: &OwnedType) -> ast::Type<'a> {
    match typ {
        OwnedType::Var { name, args } => ast::Type::Var {
            name: bump.alloc_str(name),
            args: hydrate_type_nodes(bump, args),
        },
        OwnedType::Named { reference, args } => ast::Type::Named {
            reference: hydrate_qualified_name(bump, reference),
            args: hydrate_type_nodes(bump, args),
        },
        OwnedType::Partial { constructor, slots } => ast::Type::Partial {
            constructor: hydrate_qualified_name(bump, constructor),
            slots: bump.alloc_slice_fill_iter(slots.iter().map(|slot| match slot {
                OwnedTypeSlot::Hole(index) => ast::TypeSlot::Hole(*index),
                OwnedTypeSlot::Fixed(typ) => ast::TypeSlot::Fixed(hydrate_type_node(bump, typ)),
            })),
        },
        OwnedType::Projection(projection) => {
            ast::Type::Projection(hydrate_projection(bump, projection))
        }
        OwnedType::Fn { params, ret } => ast::Type::Fn {
            params: hydrate_type_nodes(bump, params),
            ret: hydrate_type_node(bump, ret),
        },
        OwnedType::Unit => ast::Type::Unit,
        OwnedType::Tuple(items) => ast::Type::Tuple(hydrate_type_nodes(bump, items)),
        OwnedType::Record { fields, ext } => ast::Type::Record {
            fields: hydrate_record_fields(bump, fields),
            ext: hydrate_row_extension(bump, ext),
        },
        OwnedType::ErrorRow { tags, ext } => ast::Type::ErrorRow {
            tags: hydrate_error_tags(bump, tags),
            ext: hydrate_row_extension(bump, ext),
        },
        OwnedType::Alias {
            reference,
            arguments,
            target,
            filled,
        } => ast::Type::Alias {
            reference: hydrate_qualified_name(bump, reference),
            arguments: bump.alloc_slice_fill_iter(arguments.iter().map(|argument| {
                ast::AliasArgument {
                    name: bump.alloc_str(&argument.name),
                    typ: hydrate_type_node(bump, &argument.typ),
                }
            })),
            target: if *filled {
                ast::AliasType::Filled(hydrate_type_node(bump, target))
            } else {
                ast::AliasType::Open(hydrate_type_node(bump, target))
            },
        },
    }
}

fn hydrate_type_nodes<'a>(
    bump: &'a Bump,
    types: &[OwnedLocatedType],
) -> &'a [ast::Node<'a, ast::Type<'a>>] {
    bump.alloc_slice_fill_iter(types.iter().map(|typ| hydrate_type_node(bump, typ)))
}

fn hydrate_record_fields<'a>(
    bump: &'a Bump,
    fields: &[OwnedRecordField],
) -> &'a [ast::RecordTypeField<'a>] {
    bump.alloc_slice_fill_iter(fields.iter().map(|field| ast::RecordTypeField {
        index: field.index,
        name: bump.alloc_str(&field.name),
        presence: if field.optional {
            ast::FieldPresence::Optional
        } else {
            ast::FieldPresence::Required
        },
        typ: hydrate_type_node(bump, &field.typ),
    }))
}

fn hydrate_error_tags<'a>(bump: &'a Bump, tags: &[OwnedErrorTag]) -> &'a [ast::ErrorTagType<'a>] {
    bump.alloc_slice_fill_iter(tags.iter().map(|tag| ast::ErrorTagType {
        index: tag.index,
        name: bump.alloc_str(&tag.name),
        args: hydrate_type_nodes(bump, &tag.args),
    }))
}

fn hydrate_variant_payload<'a>(
    bump: &'a Bump,
    payload: &OwnedVariantPayload,
) -> ast::VariantPayload<'a> {
    match payload {
        OwnedVariantPayload::Unit => ast::VariantPayload::Unit,
        OwnedVariantPayload::Tuple(types) => {
            ast::VariantPayload::Tuple(hydrate_type_nodes(bump, types))
        }
        OwnedVariantPayload::Record(fields) => {
            ast::VariantPayload::Record(hydrate_record_fields(bump, fields))
        }
    }
}

fn hydrate_scheme<'a>(bump: &'a Bump, scheme: &OwnedScheme) -> &'a ast::Annotation<'a> {
    bump.alloc(ast::Annotation {
        params: hydrate_type_params(bump, &scheme.params),
        trait_predicates: hydrate_trait_refs(bump, &scheme.trait_predicates),
        projection_equalities: bump.alloc_slice_fill_iter(
            scheme
                .projection_equalities
                .iter()
                .map(|equality| hydrate_projection_equality(bump, equality)),
        ),
        typ: hydrate_type_node(bump, &scheme.typ),
    })
}

fn hydrate_projection_equality<'a>(
    bump: &'a Bump,
    equality: &OwnedProjectionEquality,
) -> ast::ProjectionEquality<'a> {
    ast::ProjectionEquality {
        projection: hydrate_projection(bump, &equality.projection),
        typ: hydrate_type_node(bump, &equality.typ),
        region: equality.region,
    }
}

fn hydrate_trait_refs<'a>(bump: &'a Bump, refs: &[OwnedTraitRef]) -> &'a [ast::TraitRef<'a>] {
    bump.alloc_slice_fill_iter(
        refs.iter()
            .map(|trait_ref| hydrate_trait_ref(bump, trait_ref)),
    )
}

fn hydrate_trait_ref<'a>(bump: &'a Bump, trait_ref: &OwnedTraitRef) -> ast::TraitRef<'a> {
    ast::TraitRef {
        trait_: hydrate_trait_id(bump, &trait_ref.trait_),
        args: hydrate_type_nodes(bump, &trait_ref.args),
    }
}

fn hydrate_projection<'a>(bump: &'a Bump, projection: &OwnedProjection) -> ast::ProjectionType<'a> {
    ast::ProjectionType {
        trait_ref: hydrate_trait_ref(bump, &projection.trait_ref),
        assoc: hydrate_assoc_id(bump, &projection.assoc),
    }
}

fn hydrate_type_params<'a>(bump: &'a Bump, params: &[OwnedTypeParam]) -> &'a [ast::TypeParam<'a>] {
    bump.alloc_slice_fill_iter(params.iter().map(|param| ast::TypeParam {
        name: Located::at(param.region, bump.alloc_str(&param.name) as &str),
        kind: hydrate_kind(bump, &param.kind),
    }))
}

fn hydrate_kind<'a>(bump: &'a Bump, kind: &OwnedKind) -> ast::Kind<'a> {
    match kind {
        OwnedKind::Type => ast::Kind::Type,
        OwnedKind::Arrow(param, result) => ast::Kind::Arrow {
            param: bump.alloc(hydrate_kind(bump, param)),
            result: bump.alloc(hydrate_kind(bump, result)),
        },
    }
}

fn hydrate_module_id<'a>(bump: &'a Bump, module: &OwnedModuleId) -> ast::ModuleId<'a> {
    ast::ModuleId {
        package: match &module.package {
            OwnedPackageId::Named { author, project } => ast::PackageId::Named(ast::PackageName {
                author: bump.alloc_str(author),
                project: bump.alloc_str(project),
            }),
            OwnedPackageId::Application => ast::PackageId::Application,
            OwnedPackageId::ApplicationMember(member) => {
                ast::PackageId::ApplicationMember(bump.alloc_str(member))
            }
            OwnedPackageId::Builtin => ast::PackageId::Builtin,
        },
        path: bump
            .alloc_slice_fill_iter(module.path.iter().map(|part| bump.alloc_str(part) as &str)),
    }
}

fn hydrate_qualified_name<'a>(bump: &'a Bump, name: &OwnedQualifiedName) -> ast::QualifiedName<'a> {
    ast::QualifiedName {
        module: hydrate_module_id(bump, &name.module),
        name: bump.alloc_str(&name.name),
    }
}

fn hydrate_trait_id<'a>(bump: &'a Bump, id: &OwnedTraitId) -> ast::TraitId<'a> {
    ast::TraitId(hydrate_qualified_name(bump, &id.0))
}

fn hydrate_method_id<'a>(bump: &'a Bump, id: &OwnedMethodId) -> ast::MethodId<'a> {
    ast::MethodId {
        trait_: hydrate_trait_id(bump, &id.trait_),
        index: id.index,
        name: bump.alloc_str(&id.name),
    }
}

fn hydrate_assoc_id<'a>(bump: &'a Bump, id: &OwnedAssocTypeId) -> ast::AssocTypeId<'a> {
    ast::AssocTypeId {
        trait_: hydrate_trait_id(bump, &id.trait_),
        index: id.index,
        name: bump.alloc_str(&id.name),
    }
}

fn hydrate_impl_origin(origin: OwnedImplOrigin) -> ast::ImplOrigin {
    match origin {
        OwnedImplOrigin::Source { item_ordinal } => ast::ImplOrigin::Source { item_ordinal },
        OwnedImplOrigin::Derived {
            type_ordinal,
            derive_index,
        } => ast::ImplOrigin::Derived {
            type_ordinal,
            derive_index,
        },
        OwnedImplOrigin::AutomaticEq { type_ordinal } => {
            ast::ImplOrigin::AutomaticEq { type_ordinal }
        }
        OwnedImplOrigin::Builtin { index } => ast::ImplOrigin::Builtin { index },
    }
}

fn hydrate_row_extension<'a>(bump: &'a Bump, ext: &Option<String>) -> ast::RowExtension<'a> {
    ext.as_ref().map_or(ast::RowExtension::Closed, |name| {
        ast::RowExtension::Open(bump.alloc_str(name))
    })
}

fn hydrate_value_kind(kind: OwnedValueKind) -> ast::ValueKind {
    match kind {
        OwnedValueKind::Function => ast::ValueKind::Function,
        OwnedValueKind::Let => ast::ValueKind::Let,
        OwnedValueKind::Component => ast::ValueKind::Component,
        OwnedValueKind::Table => ast::ValueKind::Table,
        OwnedValueKind::Schema => ast::ValueKind::Schema,
        OwnedValueKind::Extern => ast::ValueKind::Extern,
        OwnedValueKind::TraitMethod => ast::ValueKind::TraitMethod,
    }
}

fn hydrate_namespace(namespace: OwnedNamespace) -> ast::Namespace {
    match namespace {
        OwnedNamespace::Value => ast::Namespace::Value,
        OwnedNamespace::Type => ast::Namespace::Type,
        OwnedNamespace::Enum => ast::Namespace::Enum,
        OwnedNamespace::Constructor => ast::Namespace::Constructor,
        OwnedNamespace::Trait => ast::Namespace::Trait,
        OwnedNamespace::Module => ast::Namespace::Module,
        OwnedNamespace::Provider => ast::Namespace::Provider,
        OwnedNamespace::AssociatedItem => ast::Namespace::AssociatedItem,
    }
}

fn hydrate_opaque(kind: OwnedOpaqueKind) -> ast::OpaqueKind {
    match kind {
        OwnedOpaqueKind::Extern => ast::OpaqueKind::Extern,
        OwnedOpaqueKind::Table => ast::OpaqueKind::Table,
        OwnedOpaqueKind::Schema => ast::OpaqueKind::Schema,
    }
}
