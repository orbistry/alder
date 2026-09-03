use std::collections::BTreeMap;

use alder_ast::{
    AssocTypeDecl, ImplDecl, Interface, InterfaceImpl, InterfaceMethod, ItemKind, Kind, MethodId,
    Module, ModuleId, Name, PackageId, TraitDecl, TraitId, TraitRef, TypeParam,
};
use alder_region::{Located, Region};
use bumpalo::Bump;

#[derive(Clone, Copy, Debug)]
pub struct TraitHeader<'a> {
    pub id: TraitId<'a>,
    pub params: &'a [TypeParam<'a>],
    pub superclasses: &'a [TraitRef<'a>],
    pub associated_types: &'a [AssocTypeDecl<'a>],
    pub methods: &'a [InterfaceMethod<'a>],
}

#[derive(Clone, Copy, Debug)]
pub enum InstanceHeader<'a> {
    Local(&'a ImplDecl<'a>),
    Foreign(&'a InterfaceImpl<'a>),
}

impl<'a> InstanceHeader<'a> {
    pub fn trait_ref(self) -> TraitRef<'a> {
        match self {
            Self::Local(implementation) => implementation.trait_ref,
            Self::Foreign(implementation) => implementation.trait_ref,
        }
    }

    pub fn id(self) -> alder_ast::ImplId<'a> {
        match self {
            Self::Local(implementation) => implementation.id,
            Self::Foreign(implementation) => implementation.id,
        }
    }

    pub fn predicates(self) -> &'a [TraitRef<'a>] {
        match self {
            Self::Local(implementation) => implementation.trait_predicates,
            Self::Foreign(implementation) => implementation.trait_predicates,
        }
    }
}

#[derive(Debug)]
pub struct TraitDatabase<'a> {
    traits: BTreeMap<TraitId<'a>, TraitHeader<'a>>,
    instances: BTreeMap<TraitId<'a>, Vec<InstanceHeader<'a>>>,
}

impl<'a> TraitDatabase<'a> {
    pub fn build(
        bump: &'a Bump,
        module: &'a Module<'a>,
        dependencies: &'a [Interface<'a>],
    ) -> Self {
        let mut database = Self {
            traits: BTreeMap::new(),
            instances: BTreeMap::new(),
        };
        database.insert_builtins(bump);
        for interface in dependencies {
            for trait_ in interface.traits {
                database.traits.insert(
                    trait_.id,
                    TraitHeader {
                        id: trait_.id,
                        params: trait_.params,
                        superclasses: trait_.superclasses,
                        associated_types: trait_.associated_types,
                        methods: trait_.methods,
                    },
                );
            }
            for implementation in interface.instances {
                database
                    .instances
                    .entry(implementation.trait_ref.trait_)
                    .or_default()
                    .push(InstanceHeader::Foreign(implementation));
            }
        }
        for item in module.items {
            match &item.value.kind {
                ItemKind::Trait(trait_) => database.insert_local_trait(bump, trait_),
                ItemKind::Impl(implementation) => database
                    .instances
                    .entry(implementation.trait_ref.trait_)
                    .or_default()
                    .push(InstanceHeader::Local(implementation)),
                _ => {}
            }
        }
        for instances in database.instances.values_mut() {
            instances.sort_by_key(|implementation| implementation.id());
        }
        database
    }

    pub fn trait_(&self, id: TraitId<'a>) -> Option<TraitHeader<'a>> {
        self.traits.get(&id).copied()
    }

    pub fn instances(&self, trait_: TraitId<'a>) -> &[InstanceHeader<'a>] {
        self.instances.get(&trait_).map_or(&[], Vec::as_slice)
    }

    pub fn method(&self, id: MethodId<'a>) -> Option<InterfaceMethod<'a>> {
        self.trait_(id.trait_)?
            .methods
            .iter()
            .find(|method| method.id == id)
            .copied()
    }

    fn insert_local_trait(&mut self, bump: &'a Bump, trait_: &'a TraitDecl<'a>) {
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
        self.traits.insert(
            trait_.id,
            TraitHeader {
                id: trait_.id,
                params: trait_.type_params,
                superclasses: trait_.superclasses,
                associated_types: trait_.associated_types,
                methods: bump.alloc_slice_copy(&methods),
            },
        );
    }

    fn insert_builtins(&mut self, bump: &'a Bump) {
        for name in ["Show", "Eq", "Ord", "Hash", "Json", "Num"] {
            let id = builtin_trait_id(name);
            let params = bump.alloc_slice_copy(&[TypeParam {
                name: builtin_name(name),
                kind: Kind::Type,
            }]);
            self.traits.insert(
                id,
                TraitHeader {
                    id,
                    params,
                    superclasses: &[],
                    associated_types: &[],
                    methods: &[],
                },
            );
        }
    }
}

pub fn builtin_trait_id(name: &'static str) -> TraitId<'static> {
    TraitId(alder_ast::QualifiedName {
        module: ModuleId {
            package: PackageId::Builtin,
            path: &[],
        },
        name,
    })
}

fn builtin_name(name: &'static str) -> Name<'static> {
    Located::at(Region::zero(), name)
}

#[cfg(test)]
mod tests {
    use alder_ast::{ModuleId, PackageId};
    use alder_can::Context;

    use super::*;

    #[test]
    fn database_collects_local_traits_impls_and_builtins() {
        let bump = Bump::new();
        let source = bump.alloc_str(
            "trait Show[a] { fn show(value: a) -> String }\nimpl Show[Number] { fn show(value: Number) -> String { \"number\" } }",
        );
        let parsed = alder_parse::parse_module(&bump, source).expect("source parses");
        let module = alder_can::canonicalize(
            &bump,
            Context {
                home: ModuleId {
                    package: PackageId::Application,
                    path: &["Main"],
                },
                imports: &[],
                interfaces: &[],
            },
            &parsed,
        )
        .expect("source canonicalizes")
        .module;
        let database = TraitDatabase::build(&bump, module, &[]);
        let local = module.items.iter().find_map(|item| match &item.value.kind {
            ItemKind::Trait(trait_) => Some(trait_.id),
            _ => None,
        });
        let local = local.expect("local trait");
        assert!(database.trait_(local).is_some());
        assert_eq!(database.instances(local).len(), 1);
        assert!(database.trait_(builtin_trait_id("Eq")).is_some());
    }
}
