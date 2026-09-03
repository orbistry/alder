use std::collections::{BTreeMap, BTreeSet};

use alder_ast::{
    AssocBinding, AssocTypeDecl, DictionaryKind, ErrorTagType, ImplDecl, ImplId, Interface,
    InterfaceImpl, InterfaceMethod, ItemKind, Kind, MethodId, Module, ModuleId, PackageId,
    PublicTypeBody, QualifiedName, TraitDecl, TraitId, TraitRef, Type, TypeParam, TypeSlot,
};
use alder_region::Located;
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

    pub fn params(self) -> &'a [TypeParam<'a>] {
        match self {
            Self::Local(implementation) => implementation.params,
            Self::Foreign(implementation) => implementation.params,
        }
    }

    pub fn assoc_bindings(self) -> &'a [AssocBinding<'a>] {
        match self {
            Self::Local(implementation) => implementation.assoc_bindings,
            Self::Foreign(implementation) => implementation.assoc_bindings,
        }
    }

    pub fn dictionary_symbol(self, bump: &'a Bump) -> &'a str {
        match self {
            Self::Local(implementation) => bump.alloc_str(&format!(
                "$dict${}${}",
                implementation.trait_ref.trait_.0.name,
                impl_origin_index(implementation.id.origin)
            )),
            Self::Foreign(implementation) => implementation.dictionary_symbol,
        }
    }

    pub fn dictionary_kind(self) -> DictionaryKind {
        match self {
            Self::Local(implementation) => {
                if implementation.trait_predicates.is_empty() {
                    DictionaryKind::Singleton
                } else {
                    DictionaryKind::Factory
                }
            }
            Self::Foreign(implementation) => implementation.dictionary_kind,
        }
    }
}

#[derive(Debug)]
pub struct TraitDatabase<'a> {
    traits: BTreeMap<TraitId<'a>, TraitHeader<'a>>,
    instances: BTreeMap<TraitId<'a>, Vec<InstanceHeader<'a>>>,
    error_groups: BTreeMap<QualifiedName<'a>, &'a [ErrorTagType<'a>]>,
}

#[derive(Clone, Debug)]
pub enum CoherenceError<'a> {
    SuperclassCycle {
        traits: &'a [TraitId<'a>],
    },
    OrphanImpl {
        implementation: ImplId<'a>,
        trait_: TraitId<'a>,
        subject: &'a str,
        trait_package: PackageId<'a>,
        type_package: Option<PackageId<'a>>,
    },
    OverlappingImpl {
        first: ImplId<'a>,
        second: ImplId<'a>,
        trait_: TraitId<'a>,
    },
    InvalidTermination {
        implementation: ImplId<'a>,
        prerequisite: TraitId<'a>,
    },
    KindMismatch {
        implementation: ImplId<'a>,
        parameter: u16,
        expected_arity: u16,
        actual_arity: u16,
    },
    ProjectionCycle {
        implementation: ImplId<'a>,
        chain: &'a [alder_ast::AssocTypeId<'a>],
    },
}

impl<'a> TraitDatabase<'a> {
    pub fn build(
        bump: &'a Bump,
        module: &'a Module<'a>,
        dependencies: &'a [Interface<'a>],
    ) -> Self {
        Self::build_with_package_instances(bump, module, dependencies, &[])
    }

    pub fn build_with_package_instances(
        bump: &'a Bump,
        module: &'a Module<'a>,
        dependencies: &'a [Interface<'a>],
        package_instances: &'a [InterfaceImpl<'a>],
    ) -> Self {
        let mut database = Self {
            traits: BTreeMap::new(),
            instances: BTreeMap::new(),
            error_groups: BTreeMap::new(),
        };
        let builtins = alder_can::builtin_trait_interface(bump);
        database.insert_interface(&builtins);
        for interface in dependencies {
            database.insert_interface(interface);
        }
        for implementation in package_instances {
            database
                .instances
                .entry(implementation.trait_ref.trait_)
                .or_default()
                .push(InstanceHeader::Foreign(implementation));
        }
        for item in module.items {
            match &item.value.kind {
                ItemKind::Trait(trait_) => database.insert_local_trait(bump, trait_),
                ItemKind::Impl(implementation) => database
                    .instances
                    .entry(implementation.trait_ref.trait_)
                    .or_default()
                    .push(InstanceHeader::Local(implementation)),
                ItemKind::ErrorGroup(group) => {
                    database.error_groups.insert(group.name, group.tags);
                }
                _ => {}
            }
        }
        for instances in database.instances.values_mut() {
            instances.sort_by_key(|implementation| implementation.id());
            instances.dedup_by_key(|implementation| implementation.id());
        }
        database
    }

    fn insert_interface(&mut self, interface: &Interface<'a>) {
        for typ in interface.types {
            if let PublicTypeBody::ErrorGroup(tags) = typ.body {
                self.error_groups.insert(typ.reference, tags);
            }
        }
        for trait_ in interface.traits {
            self.traits.insert(
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
            self.instances
                .entry(implementation.trait_ref.trait_)
                .or_default()
                .push(InstanceHeader::Foreign(implementation));
        }
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

    pub fn error_group(&self, name: QualifiedName<'a>) -> Option<&'a [ErrorTagType<'a>]> {
        self.error_groups.get(&name).copied()
    }

    pub fn validate(&self, bump: &'a Bump) -> Vec<CoherenceError<'a>> {
        let mut errors = self.superclass_cycle_errors(bump);
        for (trait_, instances) in &self.instances {
            for implementation in instances {
                let trait_ref = implementation.trait_ref();
                if let Some(header) = self.trait_(*trait_) {
                    for (index, (parameter, argument)) in
                        header.params.iter().zip(trait_ref.args).enumerate()
                    {
                        let expected = kind_arity(parameter.kind);
                        let actual = type_kind_arity(&argument.value, implementation.params());
                        if let Some(actual) = actual
                            && actual != expected
                        {
                            errors.push(CoherenceError::KindMismatch {
                                implementation: implementation.id(),
                                parameter: index as u16,
                                expected_arity: expected as u16,
                                actual_arity: actual as u16,
                            });
                        }
                    }
                }
                for chain in projection_cycles(implementation.assoc_bindings()) {
                    errors.push(CoherenceError::ProjectionCycle {
                        implementation: implementation.id(),
                        chain: bump.alloc_slice_copy(&chain),
                    });
                }
                let subject = trait_ref.args.first().copied();
                let type_package = subject.and_then(outer_nominal_package);
                let implementation_package = implementation.id().module.package;
                if trait_.0.module.package != implementation_package
                    && type_package != Some(implementation_package)
                {
                    let rendered = subject
                        .map_or_else(|| "()".to_owned(), |subject| render_type(&subject.value));
                    errors.push(CoherenceError::OrphanImpl {
                        implementation: implementation.id(),
                        trait_: *trait_,
                        subject: bump.alloc_str(&rendered),
                        trait_package: trait_.0.module.package,
                        type_package,
                    });
                }
                let head_size = trait_ref
                    .args
                    .iter()
                    .map(|argument| type_size(&argument.value))
                    .sum::<usize>();
                let head_counts = variable_counts(trait_ref.args);
                for prerequisite in implementation.predicates() {
                    let prerequisite_size = prerequisite
                        .args
                        .iter()
                        .map(|argument| type_size(&argument.value))
                        .sum::<usize>();
                    let prerequisite_counts = variable_counts(prerequisite.args);
                    if prerequisite_size >= head_size
                        || prerequisite_counts.iter().any(|(name, count)| {
                            *count > head_counts.get(name).copied().unwrap_or(0)
                        })
                    {
                        errors.push(CoherenceError::InvalidTermination {
                            implementation: implementation.id(),
                            prerequisite: prerequisite.trait_,
                        });
                    }
                }
            }
            for (index, first) in instances.iter().enumerate() {
                for second in &instances[index + 1..] {
                    if heads_overlap(first.trait_ref(), second.trait_ref()) {
                        errors.push(CoherenceError::OverlappingImpl {
                            first: first.id(),
                            second: second.id(),
                            trait_: *trait_,
                        });
                    }
                }
            }
        }
        errors
    }

    fn superclass_cycle_errors(&self, bump: &'a Bump) -> Vec<CoherenceError<'a>> {
        fn visit<'a>(
            database: &TraitDatabase<'a>,
            trait_: TraitId<'a>,
            states: &mut BTreeMap<TraitId<'a>, u8>,
            stack: &mut Vec<TraitId<'a>>,
            cycles: &mut BTreeSet<Vec<TraitId<'a>>>,
        ) {
            match states.get(&trait_).copied().unwrap_or(0) {
                2 => return,
                1 => {
                    let start = stack.iter().position(|item| *item == trait_).unwrap_or(0);
                    let mut cycle = stack[start..].to_vec();
                    if let Some((minimum, _)) = cycle.iter().enumerate().min_by_key(|(_, id)| *id) {
                        cycle.rotate_left(minimum);
                    }
                    cycles.insert(cycle);
                    return;
                }
                _ => {}
            }
            states.insert(trait_, 1);
            stack.push(trait_);
            if let Some(header) = database.trait_(trait_) {
                for superclass in header.superclasses {
                    visit(database, superclass.trait_, states, stack, cycles);
                }
            }
            stack.pop();
            states.insert(trait_, 2);
        }

        let mut states = BTreeMap::new();
        let mut stack = Vec::new();
        let mut cycles = BTreeSet::new();
        for trait_ in self.traits.keys().copied() {
            visit(self, trait_, &mut states, &mut stack, &mut cycles);
        }
        cycles
            .into_iter()
            .map(|cycle| CoherenceError::SuperclassCycle {
                traits: bump.alloc_slice_copy(&cycle),
            })
            .collect()
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
}

fn projection_cycles<'a>(bindings: &[AssocBinding<'a>]) -> Vec<Vec<alder_ast::AssocTypeId<'a>>> {
    fn visit<'a>(
        assoc: alder_ast::AssocTypeId<'a>,
        edges: &BTreeMap<alder_ast::AssocTypeId<'a>, BTreeSet<alder_ast::AssocTypeId<'a>>>,
        states: &mut BTreeMap<alder_ast::AssocTypeId<'a>, u8>,
        stack: &mut Vec<alder_ast::AssocTypeId<'a>>,
        cycles: &mut BTreeSet<Vec<alder_ast::AssocTypeId<'a>>>,
    ) {
        match states.get(&assoc).copied().unwrap_or(0) {
            2 => return,
            1 => {
                let start = stack.iter().position(|item| *item == assoc).unwrap_or(0);
                let mut cycle = stack[start..].to_vec();
                if let Some((minimum, _)) = cycle.iter().enumerate().min_by_key(|(_, id)| *id) {
                    cycle.rotate_left(minimum);
                }
                cycles.insert(cycle);
                return;
            }
            _ => {}
        }
        states.insert(assoc, 1);
        stack.push(assoc);
        if let Some(targets) = edges.get(&assoc) {
            for target in targets {
                visit(*target, edges, states, stack, cycles);
            }
        }
        stack.pop();
        states.insert(assoc, 2);
    }

    let declared = bindings
        .iter()
        .map(|binding| binding.assoc)
        .collect::<BTreeSet<_>>();
    let edges = bindings
        .iter()
        .map(|binding| {
            let mut referenced = BTreeSet::new();
            collect_associated_projections(&binding.typ.value, &mut referenced);
            referenced.retain(|assoc| declared.contains(assoc));
            (binding.assoc, referenced)
        })
        .collect::<BTreeMap<_, _>>();
    let mut states = BTreeMap::new();
    let mut stack = Vec::new();
    let mut cycles = BTreeSet::new();
    for assoc in declared {
        visit(assoc, &edges, &mut states, &mut stack, &mut cycles);
    }
    cycles.into_iter().collect()
}

fn collect_associated_projections<'a>(
    typ: &Type<'a>,
    found: &mut BTreeSet<alder_ast::AssocTypeId<'a>>,
) {
    match typ {
        Type::Projection(projection) => {
            found.insert(projection.assoc);
            for argument in projection.trait_ref.args {
                collect_associated_projections(&argument.value, found);
            }
        }
        Type::Var { args, .. } | Type::Named { args, .. } => {
            for argument in *args {
                collect_associated_projections(&argument.value, found);
            }
        }
        Type::Partial { slots, .. } => {
            for slot in *slots {
                if let TypeSlot::Fixed(typ) = slot {
                    collect_associated_projections(&typ.value, found);
                }
            }
        }
        Type::Fn { params, ret } => {
            for param in *params {
                collect_associated_projections(&param.value, found);
            }
            collect_associated_projections(&ret.value, found);
        }
        Type::Tuple(items) => {
            for item in *items {
                collect_associated_projections(&item.value, found);
            }
        }
        Type::Record { fields, .. } => {
            for field in *fields {
                collect_associated_projections(&field.typ.value, found);
            }
        }
        Type::ErrorRow { tags, .. } => {
            for tag in *tags {
                for argument in tag.args {
                    collect_associated_projections(&argument.value, found);
                }
            }
        }
        Type::Alias { target, .. } => match target {
            alder_ast::AliasType::Open(typ) | alder_ast::AliasType::Filled(typ) => {
                collect_associated_projections(&typ.value, found);
            }
        },
        Type::Unit => {}
    }
}

fn kind_arity(kind: Kind<'_>) -> usize {
    match kind {
        Kind::Type => 0,
        Kind::Arrow { result, .. } => 1 + kind_arity(*result),
    }
}

fn type_kind_arity(typ: &Type<'_>, params: &[TypeParam<'_>]) -> Option<usize> {
    match typ {
        Type::Partial { slots, .. } => Some(
            slots
                .iter()
                .filter(|slot| matches!(slot, TypeSlot::Hole(_)))
                .count(),
        ),
        Type::Var { name, args } if !args.is_empty() => params
            .iter()
            .find(|parameter| parameter.name.value == *name)
            .map(|parameter| kind_arity(parameter.kind).saturating_sub(args.len())),
        Type::Var { .. } => None,
        Type::Alias { target, .. } => match target {
            alder_ast::AliasType::Open(target) | alder_ast::AliasType::Filled(target) => {
                type_kind_arity(&target.value, params)
            }
        },
        Type::Named { .. }
        | Type::Projection(_)
        | Type::Fn { .. }
        | Type::Unit
        | Type::Tuple(_)
        | Type::Record { .. }
        | Type::ErrorRow { .. } => Some(0),
    }
}

fn outer_nominal_package<'a>(typ: &'a Located<Type<'a>>) -> Option<PackageId<'a>> {
    match &typ.value {
        Type::Named { reference, .. } => Some(reference.module.package),
        Type::Partial { constructor, .. } => Some(constructor.module.package),
        Type::Alias { target, .. } => match target {
            alder_ast::AliasType::Open(target) | alder_ast::AliasType::Filled(target) => {
                outer_nominal_package(target)
            }
        },
        Type::Var { .. }
        | Type::Projection(_)
        | Type::Fn { .. }
        | Type::Unit
        | Type::Tuple(_)
        | Type::Record { .. }
        | Type::ErrorRow { .. } => None,
    }
}

fn type_size(typ: &Type<'_>) -> usize {
    match typ {
        Type::Var { args, .. } => 1 + args.iter().map(|arg| type_size(&arg.value)).sum::<usize>(),
        Type::Named { args, .. } => 1 + args.iter().map(|arg| type_size(&arg.value)).sum::<usize>(),
        Type::Partial { slots, .. } => {
            1 + slots
                .iter()
                .map(|slot| match slot {
                    TypeSlot::Hole(_) => 1,
                    TypeSlot::Fixed(typ) => type_size(&typ.value),
                })
                .sum::<usize>()
        }
        Type::Projection(projection) => {
            1 + projection
                .trait_ref
                .args
                .iter()
                .map(|arg| type_size(&arg.value))
                .sum::<usize>()
        }
        Type::Fn { params, ret } => {
            1 + params
                .iter()
                .map(|param| type_size(&param.value))
                .sum::<usize>()
                + type_size(&ret.value)
        }
        Type::Unit => 1,
        Type::Tuple(items) => {
            1 + items
                .iter()
                .map(|item| type_size(&item.value))
                .sum::<usize>()
        }
        Type::Record { fields, .. } => {
            1 + fields
                .iter()
                .map(|field| type_size(&field.typ.value))
                .sum::<usize>()
        }
        Type::ErrorRow { tags, .. } => {
            1 + tags
                .iter()
                .flat_map(|tag| tag.args)
                .map(|arg| type_size(&arg.value))
                .sum::<usize>()
        }
        Type::Alias { target, .. } => match target {
            alder_ast::AliasType::Open(target) | alder_ast::AliasType::Filled(target) => {
                type_size(&target.value)
            }
        },
    }
}

fn variable_counts<'a>(types: &'a [&'a Located<Type<'a>>]) -> BTreeMap<&'a str, usize> {
    fn collect<'a>(typ: &'a Type<'a>, counts: &mut BTreeMap<&'a str, usize>) {
        match typ {
            Type::Var { name, args } => {
                *counts.entry(name).or_default() += 1;
                for argument in *args {
                    collect(&argument.value, counts);
                }
            }
            Type::Named { args, .. } => {
                for argument in *args {
                    collect(&argument.value, counts);
                }
            }
            Type::Partial { slots, .. } => {
                for slot in *slots {
                    if let TypeSlot::Fixed(typ) = slot {
                        collect(&typ.value, counts);
                    }
                }
            }
            Type::Projection(projection) => {
                for argument in projection.trait_ref.args {
                    collect(&argument.value, counts);
                }
            }
            Type::Fn { params, ret } => {
                for param in *params {
                    collect(&param.value, counts);
                }
                collect(&ret.value, counts);
            }
            Type::Tuple(items) => {
                for item in *items {
                    collect(&item.value, counts);
                }
            }
            Type::Record { fields, .. } => {
                for field in *fields {
                    collect(&field.typ.value, counts);
                }
            }
            Type::ErrorRow { tags, .. } => {
                for argument in tags.iter().flat_map(|tag| tag.args) {
                    collect(&argument.value, counts);
                }
            }
            Type::Alias { target, .. } => match target {
                alder_ast::AliasType::Open(target) | alder_ast::AliasType::Filled(target) => {
                    collect(&target.value, counts);
                }
            },
            Type::Unit => {}
        }
    }

    let mut counts = BTreeMap::new();
    for typ in types {
        collect(&typ.value, &mut counts);
    }
    counts
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum HeadType<'a> {
    Var(usize),
    Con(QualifiedName<'a>),
    App(Box<Self>, Vec<Self>),
    Projection(TraitId<'a>, &'a str, Vec<Self>),
    Fn(Vec<Self>, Box<Self>),
    Unit,
    Tuple(Vec<Self>),
    Record(Vec<(&'a str, Self)>),
    ErrorRow(Vec<(&'a str, Vec<Self>)>),
}

fn heads_overlap<'a>(first: TraitRef<'a>, second: TraitRef<'a>) -> bool {
    if first.args.len() != second.args.len() {
        return false;
    }
    let mut next = 0;
    let mut first_vars = BTreeMap::new();
    let mut second_vars = BTreeMap::new();
    let first = first
        .args
        .iter()
        .map(|typ| head_type(typ, &mut first_vars, &mut next))
        .collect::<Vec<_>>();
    let second = second
        .args
        .iter()
        .map(|typ| head_type(typ, &mut second_vars, &mut next))
        .collect::<Vec<_>>();
    let mut substitutions = BTreeMap::new();
    first
        .into_iter()
        .zip(second)
        .all(|(left, right)| unify_head(left, right, &mut substitutions))
}

fn head_type<'a>(
    typ: &'a Located<Type<'a>>,
    variables: &mut BTreeMap<&'a str, usize>,
    next: &mut usize,
) -> HeadType<'a> {
    let apply = |head, args: &'a [&'a Located<Type<'a>>], variables: &mut _, next: &mut _| {
        if args.is_empty() {
            head
        } else {
            HeadType::App(
                Box::new(head),
                args.iter()
                    .map(|argument| head_type(argument, variables, next))
                    .collect(),
            )
        }
    };
    match &typ.value {
        Type::Var { name, args } => {
            let variable = *variables.entry(name).or_insert_with(|| {
                let id = *next;
                *next += 1;
                id
            });
            apply(HeadType::Var(variable), args, variables, next)
        }
        Type::Named { reference, args } => apply(HeadType::Con(*reference), args, variables, next),
        Type::Partial { constructor, slots } => HeadType::App(
            Box::new(HeadType::Con(*constructor)),
            slots
                .iter()
                .map(|slot| match slot {
                    TypeSlot::Hole(_) => {
                        let id = *next;
                        *next += 1;
                        HeadType::Var(id)
                    }
                    TypeSlot::Fixed(typ) => head_type(typ, variables, next),
                })
                .collect(),
        ),
        Type::Projection(projection) => HeadType::Projection(
            projection.trait_ref.trait_,
            projection.assoc.name,
            projection
                .trait_ref
                .args
                .iter()
                .map(|argument| head_type(argument, variables, next))
                .collect(),
        ),
        Type::Fn { params, ret } => HeadType::Fn(
            params
                .iter()
                .map(|param| head_type(param, variables, next))
                .collect(),
            Box::new(head_type(ret, variables, next)),
        ),
        Type::Unit => HeadType::Unit,
        Type::Tuple(items) => HeadType::Tuple(
            items
                .iter()
                .map(|item| head_type(item, variables, next))
                .collect(),
        ),
        Type::Record { fields, .. } => HeadType::Record(
            fields
                .iter()
                .map(|field| (field.name, head_type(field.typ, variables, next)))
                .collect(),
        ),
        Type::ErrorRow { tags, .. } => HeadType::ErrorRow(
            tags.iter()
                .map(|tag| {
                    (
                        tag.name,
                        tag.args
                            .iter()
                            .map(|arg| head_type(arg, variables, next))
                            .collect(),
                    )
                })
                .collect(),
        ),
        Type::Alias { target, .. } => match target {
            alder_ast::AliasType::Open(target) | alder_ast::AliasType::Filled(target) => {
                head_type(target, variables, next)
            }
        },
    }
}

fn prune_head<'a>(
    typ: HeadType<'a>,
    substitutions: &BTreeMap<usize, HeadType<'a>>,
) -> HeadType<'a> {
    match typ {
        HeadType::Var(id) => substitutions
            .get(&id)
            .cloned()
            .map_or(HeadType::Var(id), |bound| prune_head(bound, substitutions)),
        other => other,
    }
}

fn occurs_head(
    id: usize,
    typ: &HeadType<'_>,
    substitutions: &BTreeMap<usize, HeadType<'_>>,
) -> bool {
    match prune_head(typ.clone(), substitutions) {
        HeadType::Var(other) => id == other,
        HeadType::App(head, args) => {
            occurs_head(id, &head, substitutions)
                || args.iter().any(|arg| occurs_head(id, arg, substitutions))
        }
        HeadType::Projection(_, _, args) | HeadType::Tuple(args) => {
            args.iter().any(|arg| occurs_head(id, arg, substitutions))
        }
        HeadType::Fn(args, ret) => {
            args.iter().any(|arg| occurs_head(id, arg, substitutions))
                || occurs_head(id, &ret, substitutions)
        }
        HeadType::Record(fields) => fields
            .iter()
            .any(|(_, typ)| occurs_head(id, typ, substitutions)),
        HeadType::ErrorRow(tags) => tags
            .iter()
            .any(|(_, args)| args.iter().any(|arg| occurs_head(id, arg, substitutions))),
        HeadType::Con(_) | HeadType::Unit => false,
    }
}

fn unify_head<'a>(
    left: HeadType<'a>,
    right: HeadType<'a>,
    substitutions: &mut BTreeMap<usize, HeadType<'a>>,
) -> bool {
    let left = prune_head(left, substitutions);
    let right = prune_head(right, substitutions);
    match (left, right) {
        (HeadType::Var(left), HeadType::Var(right)) if left == right => true,
        (HeadType::Var(id), typ) | (typ, HeadType::Var(id)) => {
            if occurs_head(id, &typ, substitutions) {
                false
            } else {
                substitutions.insert(id, typ);
                true
            }
        }
        (HeadType::Con(left), HeadType::Con(right)) => left == right,
        (HeadType::Unit, HeadType::Unit) => true,
        (HeadType::App(left_head, left_args), HeadType::App(right_head, right_args)) => {
            left_args.len() == right_args.len()
                && unify_head(*left_head, *right_head, substitutions)
                && left_args
                    .into_iter()
                    .zip(right_args)
                    .all(|(left, right)| unify_head(left, right, substitutions))
        }
        (
            HeadType::Projection(left_trait, left_name, left_args),
            HeadType::Projection(right_trait, right_name, right_args),
        ) => {
            left_trait == right_trait
                && left_name == right_name
                && left_args.len() == right_args.len()
                && left_args
                    .into_iter()
                    .zip(right_args)
                    .all(|(left, right)| unify_head(left, right, substitutions))
        }
        (HeadType::Fn(left_args, left_ret), HeadType::Fn(right_args, right_ret)) => {
            left_args.len() == right_args.len()
                && left_args
                    .into_iter()
                    .zip(right_args)
                    .all(|(left, right)| unify_head(left, right, substitutions))
                && unify_head(*left_ret, *right_ret, substitutions)
        }
        (HeadType::Tuple(left), HeadType::Tuple(right)) => {
            left.len() == right.len()
                && left
                    .into_iter()
                    .zip(right)
                    .all(|(left, right)| unify_head(left, right, substitutions))
        }
        (HeadType::Record(left), HeadType::Record(right)) => {
            left.len() == right.len()
                && left
                    .into_iter()
                    .zip(right)
                    .all(|((left_name, left), (right_name, right))| {
                        left_name == right_name && unify_head(left, right, substitutions)
                    })
        }
        (HeadType::ErrorRow(left), HeadType::ErrorRow(right)) => {
            left.len() == right.len()
                && left
                    .into_iter()
                    .zip(right)
                    .all(|((left_name, left), (right_name, right))| {
                        left_name == right_name
                            && left.len() == right.len()
                            && left
                                .into_iter()
                                .zip(right)
                                .all(|(left, right)| unify_head(left, right, substitutions))
                    })
        }
        _ => false,
    }
}

fn render_type(typ: &Type<'_>) -> String {
    match typ {
        Type::Var { name, args: [] } => (*name).to_owned(),
        Type::Var { name, args } => format!(
            "{name}[{}]",
            args.iter()
                .map(|arg| render_type(&arg.value))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Type::Named {
            reference,
            args: [],
        } => reference.name.to_owned(),
        Type::Named { reference, args } => format!(
            "{}[{}]",
            reference.name,
            args.iter()
                .map(|arg| render_type(&arg.value))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Type::Partial { constructor, .. } => constructor.name.to_owned(),
        Type::Projection(projection) => projection.assoc.name.to_owned(),
        Type::Fn { .. } => "fn".to_owned(),
        Type::Unit => "()".to_owned(),
        Type::Tuple(_) => "tuple".to_owned(),
        Type::Record { .. } => "record".to_owned(),
        Type::ErrorRow { .. } => "error row".to_owned(),
        Type::Alias { target, .. } => match target {
            alder_ast::AliasType::Open(target) | alder_ast::AliasType::Filled(target) => {
                render_type(&target.value)
            }
        },
    }
}

fn impl_origin_index(origin: alder_ast::ImplOrigin) -> u32 {
    match origin {
        alder_ast::ImplOrigin::Source { item_ordinal }
        | alder_ast::ImplOrigin::Derived {
            type_ordinal: item_ordinal,
            ..
        }
        | alder_ast::ImplOrigin::AutomaticEq {
            type_ordinal: item_ordinal,
        } => item_ordinal,
        alder_ast::ImplOrigin::Builtin { index } => u32::from(index),
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

#[cfg(test)]
mod tests {
    use alder_ast::{ModuleId, PackageId};
    use alder_can::Context;

    use super::*;

    #[test]
    fn database_collects_local_traits_impls_and_builtins() {
        let bump = Bump::new();
        let source = bump.alloc_str(
            "trait Show[a] { fn show(value: a) String }\nimpl Show[Number] { fn show(value: Number) String { \"number\" } }",
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
        let equality = database
            .trait_(builtin_trait_id("Eq"))
            .expect("audited stdlib Eq header is loaded");
        assert_eq!(equality.methods[0].id.name, "eq");
        let iterator = database
            .trait_(builtin_trait_id("Iterator"))
            .expect("audited stdlib Iterator header is loaded");
        assert_eq!(iterator.associated_types[0].id.name, "Item");
        assert_eq!(database.instances(iterator.id).len(), 1);
        for (trait_name, instances) in [
            ("Show", 8),
            ("Eq", 9),
            ("Ord", 3),
            ("Hash", 8),
            ("Json", 8),
            ("Num", 2),
            ("Functor", 3),
            ("Applicative", 3),
            ("Monad", 3),
            ("Traversable", 3),
            ("Iterator", 1),
        ] {
            assert_eq!(
                database.instances(builtin_trait_id(trait_name)).len(),
                instances,
                "all {trait_name} instance headers come from std/Traits.ald"
            );
        }
    }
}
