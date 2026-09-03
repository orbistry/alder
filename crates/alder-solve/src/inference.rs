use std::collections::{BTreeMap, BTreeSet};

use alder_ast::{
    Annotation, BinOp, BindingName, Block, Child, ChildBlock, ChildItem, Expr, FieldPresence,
    ItemKind, MethodId, Module, ModuleId, PackageId, Pattern, QualifiedName, RecordField,
    RowExtension, Stmt, TraitId, Type, TypeSlot, UseId, ValueRef,
};
use alder_can::Annotations;
use alder_constrain::{Constraints, Error, ErrorKind, RequirementKind, RequirementSeed};
use alder_region::{Located, Region};
use bumpalo::Bump;

use crate::{
    BindingAbi, BindingEvidence, DerivedFieldKey, DirectTarget, Evidence, Intrinsic,
    IntrinsicContainer, SolveError, SolveOutput, SolveTraitError, StructuralEqShape, TraitDatabase,
    UseAction, builtin_trait_id,
};

#[derive(Clone, Debug, PartialEq)]
enum Ty<'a> {
    Var(usize),
    Con(QualifiedName<'a>),
    App(Box<Ty<'a>>, Vec<Ty<'a>>),
    Partial(QualifiedName<'a>, Vec<TySlot<'a>>),
    Projection(
        alder_ast::TraitId<'a>,
        Vec<Ty<'a>>,
        alder_ast::AssocTypeId<'a>,
    ),
    Fn(Vec<Ty<'a>>, Box<Ty<'a>>),
    Unit,
    Tuple(Vec<Ty<'a>>),
    Record(BTreeMap<&'a str, (FieldPresence, Ty<'a>)>, bool),
    ErrorRow,
    Any,
}

#[derive(Clone, Debug, PartialEq)]
enum TySlot<'a> {
    Hole(u16),
    Fixed(Ty<'a>),
}

fn resolve_obligations<'a>(
    bump: &'a Bump,
    module: &'a Module<'a>,
    database: &TraitDatabase<'a>,
    result: InferenceResult<'a>,
) -> Result<SolveOutput<'a>, Vec<SolveError<'a>>> {
    let mut uses = BTreeMap::new();
    let mut impl_superclasses = BTreeMap::new();
    let mut errors = Vec::new();
    for obligation in result.obligations {
        let mut stack = Vec::new();
        match resolve_predicate(
            bump,
            database,
            &obligation.predicate,
            &obligation.givens,
            obligation.region,
            &mut stack,
        ) {
            Ok(evidence) => match obligation.action {
                ObligationAction::Reference(method) => match uses.entry(
                    obligation
                        .use_id
                        .expect("reference obligations carry a use id"),
                ) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(UseAction::Reference {
                            dictionaries: vec![evidence],
                            method,
                        });
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        if let UseAction::Reference { dictionaries, .. } = entry.get_mut() {
                            dictionaries.push(evidence);
                        }
                    }
                },
                ObligationAction::Operator => {
                    uses.insert(
                        obligation
                            .use_id
                            .expect("operator obligations carry a use id"),
                        UseAction::Operator {
                            dictionary: evidence,
                        },
                    );
                }
                ObligationAction::Pin => {
                    uses.insert(
                        obligation.use_id.expect("pin obligations carry a use id"),
                        UseAction::Pin {
                            dictionary: evidence,
                        },
                    );
                }
                ObligationAction::CompoundAssign => {
                    uses.insert(
                        obligation
                            .use_id
                            .expect("compound assignment obligations carry a use id"),
                        UseAction::CompoundAssign {
                            dictionary: evidence,
                        },
                    );
                }
                ObligationAction::ImplSuperclass {
                    implementation,
                    slot,
                } => {
                    impl_superclasses.insert((implementation, slot), evidence);
                }
            },
            Err(error) => errors.push(SolveError::Trait(error)),
        }
    }
    for call in result.calls {
        let action = match (call.callee_use, call.target) {
            (Some(callee_use), target @ Some(_)) => {
                let dictionaries = match uses.get(&callee_use) {
                    Some(UseAction::Reference { dictionaries, .. }) => dictionaries.clone(),
                    _ => Vec::new(),
                };
                UseAction::DirectCall {
                    callee_use,
                    dictionaries,
                    target,
                }
            }
            _ => UseAction::IndirectCall,
        };
        uses.insert(call.use_id, action);
    }
    let derived_fields = match resolve_derived_fields(bump, module, database) {
        Ok(fields) => fields,
        Err(mut derived_errors) => {
            errors.append(&mut derived_errors);
            BTreeMap::new()
        }
    };
    if errors.is_empty() {
        let schemes = result.annotations.clone();
        Ok(SolveOutput {
            annotations: result.annotations,
            schemes,
            bindings: result.bindings,
            uses,
            impl_superclasses,
            derived_fields,
        })
    } else {
        Err(errors)
    }
}

fn resolve_derived_fields<'a>(
    bump: &'a Bump,
    module: &'a Module<'a>,
    database: &TraitDatabase<'a>,
) -> Result<BTreeMap<DerivedFieldKey<'a>, Evidence<'a>>, Vec<SolveError<'a>>> {
    let mut resolved = BTreeMap::new();
    let mut errors = Vec::new();
    for item in module.items {
        let ItemKind::Impl(implementation) = &item.value.kind else {
            continue;
        };
        if implementation.synthetic.is_none() {
            continue;
        }
        let mut vars = implementation
            .params
            .iter()
            .enumerate()
            .map(|(index, parameter)| (parameter.name.value, Ty::Var(index)))
            .collect::<BTreeMap<_, _>>();
        let self_predicate = predicate_from_ast_ref(implementation.trait_ref, &mut vars);
        let mut givens = implementation
            .trait_predicates
            .iter()
            .enumerate()
            .map(|(index, predicate)| Given {
                predicate: predicate_from_ast_ref(*predicate, &mut vars),
                evidence: Evidence::Param(index as u16),
            })
            .collect::<Vec<_>>();
        givens.push(Given {
            predicate: self_predicate.clone(),
            evidence: Evidence::SelfDictionary,
        });
        let Some(subject) = implementation
            .trait_ref
            .args
            .first()
            .and_then(|subject| match subject.value {
                Type::Named { reference, .. } => Some(reference),
                _ => None,
            })
        else {
            continue;
        };
        for item in module.items {
            match &item.value.kind {
                ItemKind::Enum(enum_) if enum_.name == subject => {
                    for variant in enum_.variants {
                        let fields: Vec<alder_ast::Node<'a, Type<'a>>> = match variant.payload {
                            alder_ast::VariantPayload::Unit => Vec::new(),
                            alder_ast::VariantPayload::Tuple(fields) => fields.to_vec(),
                            alder_ast::VariantPayload::Record(fields) => {
                                fields.iter().map(|field| field.typ).collect()
                            }
                        };
                        resolve_derived_variant_fields(
                            bump,
                            database,
                            implementation,
                            &self_predicate,
                            &givens,
                            variant.index,
                            fields.into_iter(),
                            &mut vars,
                            &mut resolved,
                            &mut errors,
                        );
                    }
                }
                ItemKind::ErrorGroup(group) if group.name == subject => {
                    for tag in group.tags {
                        resolve_derived_variant_fields(
                            bump,
                            database,
                            implementation,
                            &self_predicate,
                            &givens,
                            tag.index,
                            tag.args.iter().copied(),
                            &mut vars,
                            &mut resolved,
                            &mut errors,
                        );
                    }
                }
                _ => {}
            }
        }
    }
    if errors.is_empty() {
        Ok(resolved)
    } else {
        Err(errors)
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_derived_variant_fields<'a, 'field>(
    bump: &'a Bump,
    database: &TraitDatabase<'a>,
    implementation: &'a alder_ast::ImplDecl<'a>,
    self_predicate: &Predicate<'a>,
    givens: &[Given<'a>],
    variant: u16,
    fields: impl Iterator<Item = &'field Located<Type<'a>>>,
    vars: &mut BTreeMap<&'a str, Ty<'a>>,
    resolved: &mut BTreeMap<DerivedFieldKey<'a>, Evidence<'a>>,
    errors: &mut Vec<SolveError<'a>>,
) where
    'a: 'field,
{
    for (field, typ) in fields.enumerate() {
        let predicate = Predicate {
            trait_: self_predicate.trait_,
            args: vec![ty_from_ast(typ, vars)],
        };
        let mut stack = Vec::new();
        match resolve_predicate(bump, database, &predicate, givens, typ.region, &mut stack) {
            Ok(evidence) => {
                resolved.insert(
                    DerivedFieldKey {
                        implementation: implementation.id,
                        variant,
                        field: field as u16,
                    },
                    evidence,
                );
            }
            Err(error) => errors.push(SolveError::Trait(error)),
        }
    }
}

fn predicate_from_ast_ref<'a>(
    predicate: alder_ast::TraitRef<'a>,
    vars: &mut BTreeMap<&'a str, Ty<'a>>,
) -> Predicate<'a> {
    Predicate {
        trait_: predicate.trait_,
        args: predicate
            .args
            .iter()
            .map(|argument| ty_from_ast(argument, vars))
            .collect(),
    }
}

fn ty_from_ast<'a>(typ: &Located<Type<'a>>, vars: &mut BTreeMap<&'a str, Ty<'a>>) -> Ty<'a> {
    let apply = |head, args: Vec<_>| {
        if args.is_empty() {
            head
        } else {
            Ty::App(Box::new(head), args)
        }
    };
    match &typ.value {
        Type::Var { name, args } => {
            let next = vars.len();
            let head = vars.entry(name).or_insert(Ty::Var(next)).clone();
            apply(
                head,
                args.iter()
                    .map(|argument| ty_from_ast(argument, vars))
                    .collect(),
            )
        }
        Type::Named { reference, args } => apply(
            Ty::Con(*reference),
            args.iter()
                .map(|argument| ty_from_ast(argument, vars))
                .collect(),
        ),
        Type::Partial { constructor, slots } => Ty::Partial(
            *constructor,
            slots
                .iter()
                .map(|slot| match slot {
                    TypeSlot::Hole(index) => TySlot::Hole(*index),
                    TypeSlot::Fixed(typ) => TySlot::Fixed(ty_from_ast(typ, vars)),
                })
                .collect(),
        ),
        Type::Projection(projection) => Ty::Projection(
            projection.trait_ref.trait_,
            projection
                .trait_ref
                .args
                .iter()
                .map(|argument| ty_from_ast(argument, vars))
                .collect(),
            projection.assoc,
        ),
        Type::Fn { params, ret } => Ty::Fn(
            params
                .iter()
                .map(|param| ty_from_ast(param, vars))
                .collect(),
            Box::new(ty_from_ast(ret, vars)),
        ),
        Type::Unit => Ty::Unit,
        Type::Tuple(items) => Ty::Tuple(items.iter().map(|item| ty_from_ast(item, vars)).collect()),
        Type::Record { fields, ext } => Ty::Record(
            fields
                .iter()
                .map(|field| (field.name, (field.presence, ty_from_ast(field.typ, vars))))
                .collect(),
            matches!(ext, RowExtension::Open(_)),
        ),
        Type::ErrorRow { .. } => Ty::ErrorRow,
        Type::Alias { target, .. } => match target {
            alder_ast::AliasType::Open(real) | alder_ast::AliasType::Filled(real) => {
                ty_from_ast(real, vars)
            }
        },
    }
}

fn resolve_predicate<'a>(
    bump: &'a Bump,
    database: &TraitDatabase<'a>,
    predicate: &Predicate<'a>,
    givens: &[Given<'a>],
    origin: Region,
    stack: &mut Vec<(TraitId<'a>, String)>,
) -> Result<Evidence<'a>, SolveTraitError<'a>> {
    let subject = predicate.args.first().cloned().unwrap_or(Ty::Unit);
    let rendered = render_ty(&subject);
    if let Some(given) = givens.iter().find(|given| {
        given.predicate.trait_ == predicate.trait_ && given.predicate.args == predicate.args
    }) {
        return Ok(given.evidence.clone());
    }
    if stack
        .iter()
        .any(|(trait_, active)| *trait_ == predicate.trait_ && *active == rendered)
    {
        return Err(SolveTraitError::InstanceCycle {
            trait_: predicate.trait_,
            subject: bump.alloc_str(&rendered),
            origin,
        });
    }
    if let Some(evidence) = resolve_structural_eq(bump, database, predicate, givens, origin, stack)?
    {
        return Ok(evidence);
    }
    stack.push((predicate.trait_, rendered.clone()));
    let mut successes = Vec::new();
    let mut nested_error = None;
    for implementation in database.instances(predicate.trait_) {
        let template = implementation.trait_ref();
        if template.args.len() != predicate.args.len() {
            continue;
        }
        let mut bindings = BTreeMap::new();
        if !template
            .args
            .iter()
            .zip(&predicate.args)
            .all(|(template, goal)| match_type(template, goal, &mut bindings))
        {
            continue;
        }
        let mut arguments = Vec::new();
        let mut failed = None;
        for prerequisite in implementation.predicates() {
            let prerequisite = Predicate {
                trait_: prerequisite.trait_,
                args: prerequisite
                    .args
                    .iter()
                    .map(|argument| substitute_type(argument, &bindings))
                    .collect(),
            };
            match resolve_predicate(bump, database, &prerequisite, givens, origin, stack) {
                Ok(evidence) => arguments.push(evidence),
                Err(error) => {
                    failed = Some(error);
                    break;
                }
            }
        }
        if let Some(error) = failed {
            nested_error = Some(error);
        } else {
            let impl_id = implementation.id();
            let evidence = builtin_instance_evidence(impl_id, predicate, &arguments).unwrap_or(
                Evidence::Impl {
                    impl_id,
                    module: impl_id.module,
                    symbol: implementation.dictionary_symbol(bump),
                    kind: implementation.dictionary_kind(),
                    arguments,
                },
            );
            successes.push((impl_id, evidence));
        }
    }
    stack.pop();
    match successes.len() {
        1 => Ok(successes.pop().expect("one success").1),
        count if count > 1 => Err(SolveTraitError::AmbiguousInstance {
            trait_: predicate.trait_,
            subject: bump.alloc_str(&rendered),
            origin,
            candidates: bump
                .alloc_slice_fill_iter(successes.into_iter().map(|(impl_id, _)| impl_id)),
        }),
        _ if nested_error.is_some() => Err(nested_error.expect("checked above")),
        _ if contains_variable(&subject) => Err(SolveTraitError::UnsatisfiedBound {
            trait_: predicate.trait_,
            subject: bump.alloc_str(&rendered),
            origin,
        }),
        _ => Err(SolveTraitError::MissingInstance {
            trait_: predicate.trait_,
            subject: bump.alloc_str(&rendered),
            origin,
        }),
    }
}

fn resolve_structural_eq<'a>(
    bump: &'a Bump,
    database: &TraitDatabase<'a>,
    predicate: &Predicate<'a>,
    givens: &[Given<'a>],
    origin: Region,
    stack: &mut Vec<(TraitId<'a>, String)>,
) -> Result<Option<Evidence<'a>>, SolveTraitError<'a>> {
    if predicate.trait_ != builtin_trait_id("Eq") {
        return Ok(None);
    }
    let Some(subject) = predicate.args.first() else {
        return Ok(None);
    };
    let structural = match subject {
        Ty::Tuple(items) => Some((StructuralEqShape::Tuple, items.clone())),
        Ty::Record(fields, false) => Some((
            StructuralEqShape::Record(fields.keys().copied().collect()),
            fields.values().map(|(_, typ)| typ.clone()).collect(),
        )),
        _ => None,
    };
    if let Some((shape, children)) = structural {
        let mut evidence = Vec::new();
        for child in children {
            evidence.push(resolve_predicate(
                bump,
                database,
                &Predicate {
                    trait_: predicate.trait_,
                    args: vec![child],
                },
                givens,
                origin,
                stack,
            )?);
        }
        return Ok(Some(Evidence::StructuralEq {
            shape,
            fields: evidence,
        }));
    }
    Ok(None)
}

fn builtin_instance_evidence<'a>(
    implementation: alder_ast::ImplId<'a>,
    predicate: &Predicate<'a>,
    arguments: &[Evidence<'a>],
) -> Option<Evidence<'a>> {
    if implementation.module.package != PackageId::Builtin {
        return None;
    }
    let subject = predicate.args.first()?;
    let trait_name = predicate.trait_.0.name;
    let nominal = match subject {
        Ty::Partial(reference, _) => Some(reference.name),
        _ => nominal_name(subject),
    };
    let container = match nominal {
        Some("Array") => Some(IntrinsicContainer::Array),
        Some("Option") => Some(IntrinsicContainer::Option),
        Some("Result") => Some(IntrinsicContainer::Result),
        _ => None,
    };
    if let Some(container) = container {
        if let Some(intrinsic) = match trait_name {
            "Show" => Some(Intrinsic::ShowKernel),
            "Hash" => Some(Intrinsic::HashKernel),
            "Json" => Some(Intrinsic::JsonKernel),
            _ => None,
        } {
            return Some(Evidence::IntrinsicContainer {
                intrinsic,
                container,
                arguments: arguments.to_vec(),
            });
        }
        if trait_name == "Eq" {
            let shape = match container {
                IntrinsicContainer::Array => StructuralEqShape::Array,
                IntrinsicContainer::Option => StructuralEqShape::Option,
                IntrinsicContainer::Result => StructuralEqShape::Result,
            };
            return Some(Evidence::StructuralEq {
                shape,
                fields: arguments.to_vec(),
            });
        }
    }
    let intrinsic = match (trait_name, nominal) {
        ("Show", Some("Number" | "String" | "Bool" | "BigInt")) => Intrinsic::ShowKernel,
        ("Hash", Some("Number" | "String" | "Bool" | "BigInt")) => Intrinsic::HashKernel,
        ("Json", Some("Number" | "String" | "Bool" | "BigInt")) => Intrinsic::JsonKernel,
        ("Eq", Some("Number")) => Intrinsic::EqNumber,
        ("Eq", Some("String")) => Intrinsic::EqString,
        ("Eq", Some("Bool")) => Intrinsic::EqBool,
        ("Eq", Some("BigInt")) => Intrinsic::EqBigInt,
        ("Eq", Some("Ordering")) => Intrinsic::EqOrdering,
        ("Ord", Some("Number")) => Intrinsic::OrdNumber,
        ("Ord", Some("String")) => Intrinsic::OrdString,
        ("Ord", Some("BigInt")) => Intrinsic::OrdBigInt,
        ("Num", Some("Number")) => Intrinsic::NumNumber,
        ("Num", Some("BigInt")) => Intrinsic::NumBigInt,
        ("Functor", Some("Array")) => Intrinsic::FunctorArray,
        ("Functor", Some("Option")) => Intrinsic::FunctorOption,
        ("Functor", Some("Result")) => Intrinsic::FunctorResult,
        ("Applicative", Some("Array")) => Intrinsic::ApplicativeArray,
        ("Applicative", Some("Option")) => Intrinsic::ApplicativeOption,
        ("Applicative", Some("Result")) => Intrinsic::ApplicativeResult,
        ("Monad", Some("Array")) => Intrinsic::MonadArray,
        ("Monad", Some("Option")) => Intrinsic::MonadOption,
        ("Monad", Some("Result")) => Intrinsic::MonadResult,
        ("Traversable", Some("Array")) => Intrinsic::TraversableArray,
        ("Traversable", Some("Option")) => Intrinsic::TraversableOption,
        ("Traversable", Some("Result")) => Intrinsic::TraversableResult,
        ("Iterator", Some("Array")) => Intrinsic::IteratorArray,
        ("Show", None) if matches!(subject, Ty::Unit) => Intrinsic::ShowKernel,
        ("Hash", None) if matches!(subject, Ty::Unit) => Intrinsic::HashKernel,
        ("Json", None) if matches!(subject, Ty::Unit) => Intrinsic::JsonKernel,
        ("Eq", None) if matches!(subject, Ty::Unit) => Intrinsic::EqUnit,
        _ => return None,
    };
    Some(Evidence::Intrinsic(intrinsic))
}

fn match_type<'a>(
    template: &'a Located<Type<'a>>,
    goal: &Ty<'a>,
    bindings: &mut BTreeMap<&'a str, Ty<'a>>,
) -> bool {
    match &template.value {
        Type::Var { name, args: [] } => match bindings.get(name) {
            Some(bound) => bound == goal,
            None => {
                bindings.insert(name, goal.clone());
                true
            }
        },
        Type::Named { reference, args } => match nominal_parts(goal) {
            Some((actual, actual_args))
                if actual == *reference && actual_args.len() == args.len() =>
            {
                args.iter()
                    .zip(actual_args)
                    .all(|(template, actual)| match_type(template, actual, bindings))
            }
            _ => false,
        },
        Type::Partial { constructor, slots } => match goal {
            Ty::Partial(actual, actual_slots) => {
                constructor == actual
                    && slots.len() == actual_slots.len()
                    && slots
                        .iter()
                        .zip(actual_slots)
                        .all(|(left, right)| match (left, right) {
                            (TypeSlot::Hole(_), TySlot::Hole(_)) => true,
                            (TypeSlot::Fixed(left), TySlot::Fixed(right)) => {
                                match_type(left, right, bindings)
                            }
                            _ => false,
                        })
            }
            _ => false,
        },
        Type::Unit => matches!(goal, Ty::Unit),
        Type::Tuple(items) => match goal {
            Ty::Tuple(actual) if actual.len() == items.len() => items
                .iter()
                .zip(actual)
                .all(|(template, actual)| match_type(template, actual, bindings)),
            _ => false,
        },
        Type::Alias { target, .. } => match target {
            alder_ast::AliasType::Open(real) | alder_ast::AliasType::Filled(real) => {
                match_type(real, goal, bindings)
            }
        },
        Type::Fn { .. }
        | Type::Record { .. }
        | Type::ErrorRow { .. }
        | Type::Projection(_)
        | Type::Var { .. } => false,
    }
}

fn substitute_type<'a>(typ: &'a Located<Type<'a>>, bindings: &BTreeMap<&'a str, Ty<'a>>) -> Ty<'a> {
    match &typ.value {
        Type::Var { name, args: [] } => bindings.get(name).cloned().unwrap_or(Ty::Any),
        Type::Named { reference, args } => {
            let arguments = args
                .iter()
                .map(|argument| substitute_type(argument, bindings))
                .collect::<Vec<_>>();
            if arguments.is_empty() {
                Ty::Con(*reference)
            } else {
                Ty::App(Box::new(Ty::Con(*reference)), arguments)
            }
        }
        Type::Unit => Ty::Unit,
        Type::Tuple(items) => Ty::Tuple(
            items
                .iter()
                .map(|item| substitute_type(item, bindings))
                .collect(),
        ),
        Type::Alias { target, .. } => match target {
            alder_ast::AliasType::Open(real) | alder_ast::AliasType::Filled(real) => {
                substitute_type(real, bindings)
            }
        },
        _ => Ty::Any,
    }
}

fn nominal_parts<'t, 'a>(typ: &'t Ty<'a>) -> Option<(QualifiedName<'a>, &'t [Ty<'a>])> {
    match typ {
        Ty::Con(name) => Some((*name, &[])),
        Ty::App(head, args) => match head.as_ref() {
            Ty::Con(name) => Some((*name, args)),
            _ => None,
        },
        _ => None,
    }
}

fn nominal_name<'a>(typ: &Ty<'a>) -> Option<&'a str> {
    nominal_parts(typ).map(|(name, _)| name.name)
}

fn contains_variable(typ: &Ty<'_>) -> bool {
    match typ {
        Ty::Var(_) => true,
        Ty::App(head, args) => contains_variable(head) || args.iter().any(contains_variable),
        Ty::Partial(_, slots) => slots.iter().any(|slot| match slot {
            TySlot::Hole(_) => false,
            TySlot::Fixed(typ) => contains_variable(typ),
        }),
        Ty::Projection(_, args, _) | Ty::Tuple(args) => args.iter().any(contains_variable),
        Ty::Fn(params, ret) => params.iter().any(contains_variable) || contains_variable(ret),
        Ty::Record(fields, _) => fields.values().any(|(_, typ)| contains_variable(typ)),
        Ty::Con(_) | Ty::Unit | Ty::ErrorRow | Ty::Any => false,
    }
}

fn render_ty(typ: &Ty<'_>) -> String {
    match typ {
        Ty::Var(id) => format!("t{id}"),
        Ty::Con(name) => name.name.to_owned(),
        Ty::App(head, args) => format!(
            "{}[{}]",
            render_ty(head),
            args.iter().map(render_ty).collect::<Vec<_>>().join(", ")
        ),
        Ty::Partial(name, slots) => format!(
            "{}[{}]",
            name.name,
            slots
                .iter()
                .map(|slot| match slot {
                    TySlot::Hole(_) => "_".to_owned(),
                    TySlot::Fixed(typ) => render_ty(typ),
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Ty::Projection(trait_, args, assoc) => format!(
            "{}[{}]::{}",
            trait_.0.name,
            args.iter().map(render_ty).collect::<Vec<_>>().join(", "),
            assoc.name
        ),
        Ty::Fn(params, ret) => format!(
            "fn({}) -> {}",
            params.iter().map(render_ty).collect::<Vec<_>>().join(", "),
            render_ty(ret)
        ),
        Ty::Unit => "()".to_owned(),
        Ty::Tuple(items) => format!(
            "({})",
            items.iter().map(render_ty).collect::<Vec<_>>().join(", ")
        ),
        Ty::Record(_, _) => "{ .. }".to_owned(),
        Ty::ErrorRow => "[:_ | e]".to_owned(),
        Ty::Any => "_".to_owned(),
    }
}

#[derive(Clone, Debug)]
struct Scheme<'a> {
    quantified: Vec<usize>,
    predicates: Vec<Predicate<'a>>,
    projection_eqs: Vec<ProjectionEquation<'a>>,
    typ: Ty<'a>,
}

#[derive(Clone, Debug)]
struct Predicate<'a> {
    trait_: TraitId<'a>,
    args: Vec<Ty<'a>>,
}

#[derive(Clone, Debug)]
struct ProjectionEquation<'a> {
    projection: Ty<'a>,
    typ: Ty<'a>,
}

#[derive(Clone, Debug)]
struct Given<'a> {
    predicate: Predicate<'a>,
    evidence: Evidence<'a>,
}

#[derive(Clone, Copy, Debug)]
enum ObligationAction<'a> {
    Reference(Option<MethodId<'a>>),
    Operator,
    Pin,
    CompoundAssign,
    ImplSuperclass {
        implementation: alder_ast::ImplId<'a>,
        slot: u16,
    },
}

#[derive(Clone, Debug)]
struct Obligation<'a> {
    use_id: Option<UseId>,
    predicate: Predicate<'a>,
    region: Region,
    action: ObligationAction<'a>,
    givens: Vec<Given<'a>>,
}

struct InferenceResult<'a> {
    annotations: Annotations<'a>,
    bindings: BTreeMap<QualifiedName<'a>, BindingEvidence<'a>>,
    obligations: Vec<Obligation<'a>>,
    calls: Vec<CallSite<'a>>,
}

#[derive(Clone, Copy, Debug)]
struct CallSite<'a> {
    use_id: UseId,
    callee_use: Option<UseId>,
    target: Option<DirectTarget<'a>>,
}

#[derive(Clone, Copy)]
enum FunctionContext<'a> {
    Ordinary,
    Impl {
        implementation: &'a alder_ast::ImplDecl<'a>,
        method: &'a alder_ast::ImplFn<'a>,
    },
    Default(&'a alder_ast::TraitDecl<'a>),
}

#[derive(Clone, Copy)]
struct FunctionInput<'a> {
    params: &'a [alder_ast::Param<'a>],
    ret: Option<&'a Located<Type<'a>>>,
    constraints: &'a [alder_ast::TypeConstraint<'a>],
    context: FunctionContext<'a>,
    body: &'a Located<Block<'a>>,
    region: Region,
}

#[derive(Clone, Default)]
struct Env<'a> {
    locals: BTreeMap<u32, Scheme<'a>>,
    globals: BTreeMap<QualifiedName<'a>, Scheme<'a>>,
}

struct Infer<'a, 'db> {
    bump: &'a Bump,
    database: &'db TraitDatabase<'a>,
    substitutions: Vec<Option<Ty<'a>>>,
    obligations: Vec<Obligation<'a>>,
    givens: Vec<Given<'a>>,
    projection_equations: Vec<ProjectionEquation<'a>>,
    calls: Vec<CallSite<'a>>,
    requirement_seeds: BTreeMap<UseId, RequirementSeed<'a>>,
}

pub fn run<'a>(
    bump: &'a Bump,
    constraints: &Constraints<'a>,
) -> Result<Annotations<'a>, Vec<Error>> {
    let database = TraitDatabase::build(bump, constraints.module, &[]);
    Infer::new(bump, &database, constraints.requirement_seeds)
        .infer_module(constraints.module)
        .map(|result| result.annotations)
        .map_err(|error| vec![error])
}

pub fn solve<'a>(
    bump: &'a Bump,
    constraints: &Constraints<'a>,
    database: &TraitDatabase<'a>,
) -> Result<SolveOutput<'a>, Vec<SolveError<'a>>> {
    let coherence_errors = database
        .validate(bump)
        .into_iter()
        .map(SolveError::Coherence)
        .collect::<Vec<_>>();
    if !coherence_errors.is_empty() {
        return Err(coherence_errors);
    }
    let result = Infer::new(bump, database, constraints.requirement_seeds)
        .infer_module(constraints.module)
        .map_err(|error| vec![SolveError::Core(error)])?;
    resolve_obligations(bump, constraints.module, database, result)
}

impl<'a, 'db> Infer<'a, 'db> {
    fn new(
        bump: &'a Bump,
        database: &'db TraitDatabase<'a>,
        requirement_seeds: &'a [RequirementSeed<'a>],
    ) -> Self {
        Self {
            bump,
            database,
            substitutions: Vec::new(),
            obligations: Vec::new(),
            givens: Vec::new(),
            projection_equations: Vec::new(),
            calls: Vec::new(),
            requirement_seeds: requirement_seeds
                .iter()
                .map(|seed| (seed.use_id, *seed))
                .collect(),
        }
    }

    fn fresh(&mut self) -> Ty<'a> {
        let id = self.substitutions.len();
        self.substitutions.push(None);
        Ty::Var(id)
    }

    fn infer_module(&mut self, module: &'a Module<'a>) -> Result<InferenceResult<'a>, Error> {
        let mut env = Env::default();
        let mut value_items = BTreeMap::new();
        for item in module.items {
            match &item.value.kind {
                ItemKind::Fn(function) => {
                    self.predeclare(&mut env, function.name);
                    value_items.insert(function.name, *item);
                }
                ItemKind::Extern(alder_ast::ExternDecl::Fn { name, .. }) => {
                    self.predeclare(&mut env, *name);
                    value_items.insert(*name, *item);
                }
                ItemKind::Let(decl) => {
                    for binding in decl.bindings {
                        self.predeclare(&mut env, *binding);
                        value_items.insert(*binding, *item);
                    }
                }
                ItemKind::Component(component) => {
                    self.predeclare(&mut env, component.name);
                    value_items.insert(component.name, *item);
                }
                _ => {}
            }
        }

        for group in module.value_sccs {
            let mut seeded_items = BTreeSet::new();
            for member in group.members {
                let item = value_items
                    .get(member)
                    .expect("each value SCC member has a declaration");
                let identity: *const Located<alder_ast::Item<'a>> = *item;
                if seeded_items.insert(identity) {
                    self.seed_value_item(&mut env, &item.value.kind, item.region)?;
                }
            }
            let mut inferred_items = BTreeSet::new();
            for member in group.members {
                let item = value_items
                    .get(member)
                    .expect("each value SCC member has a declaration");
                let identity: *const Located<alder_ast::Item<'a>> = *item;
                if inferred_items.insert(identity) {
                    self.infer_value_item(&mut env, &item.value.kind, item.region)?;
                }
            }
            for member in group.members {
                self.generalize_global(&mut env, *member);
            }
        }

        for item in module.items {
            if !is_value_item(&item.value.kind) {
                self.infer_item(&mut env, &item.value.kind, item.region)?;
            }
        }

        let mut annotations = BTreeMap::new();
        let mut bindings = BTreeMap::new();
        for (name, scheme) in env.globals {
            let abi = match self.prune(scheme.typ.clone()) {
                Ty::Fn(_, _) => BindingAbi::DirectFunction,
                _ if scheme.predicates.is_empty() => BindingAbi::PlainValue,
                _ => BindingAbi::EvidenceFactory,
            };
            let annotation = self.annotation(&scheme);
            annotations.insert(name, annotation);
            bindings.insert(
                name,
                BindingEvidence {
                    dictionary_params: annotation.trait_predicates,
                    abi,
                },
            );
        }
        let mut obligations = std::mem::take(&mut self.obligations);
        for obligation in &mut obligations {
            for argument in &mut obligation.predicate.args {
                *argument = self.normalize_type(argument.clone());
            }
            for given in &mut obligation.givens {
                for argument in &mut given.predicate.args {
                    *argument = self.normalize_type(argument.clone());
                }
            }
        }
        Ok(InferenceResult {
            annotations,
            bindings,
            obligations,
            calls: std::mem::take(&mut self.calls),
        })
    }

    fn predeclare(&mut self, env: &mut Env<'a>, name: QualifiedName<'a>) {
        let typ = self.fresh();
        env.globals.insert(
            name,
            Scheme {
                quantified: Vec::new(),
                predicates: Vec::new(),
                projection_eqs: Vec::new(),
                typ,
            },
        );
    }

    fn seed_value_item(
        &mut self,
        env: &mut Env<'a>,
        item: &'a ItemKind<'a>,
        region: Region,
    ) -> Result<(), Error> {
        let (name, params, ret, constraints) = match item {
            ItemKind::Fn(function) => (
                function.name,
                function.params,
                function.ret,
                function.constraints,
            ),
            ItemKind::Extern(alder_ast::ExternDecl::Fn {
                name,
                params,
                ret,
                constraints,
                ..
            }) => (*name, *params, Some(*ret), *constraints),
            ItemKind::Let(_) | ItemKind::Component(_) => return Ok(()),
            _ => unreachable!("only value items belong to value SCCs"),
        };
        let mut vars = BTreeMap::new();
        let mut args = Vec::with_capacity(params.len());
        for parameter in params {
            args.push(match parameter.annotation {
                Some(typ) => self.from_ast(typ, &mut vars),
                None => self.fresh(),
            });
        }
        let ret = match ret {
            Some(typ) => self.from_ast(typ, &mut vars),
            None => self.fresh(),
        };
        let predicates = self.predicates_from_constraints(constraints, &vars);
        let projection_eqs = self.projection_equations_from_constraints(constraints, &vars)?;
        let placeholder = env
            .globals
            .get(&name)
            .expect("value was predeclared")
            .typ
            .clone();
        self.unify(placeholder, Ty::Fn(args, Box::new(ret)), region)?;
        let scheme = env.globals.get_mut(&name).expect("value was predeclared");
        scheme.predicates = predicates;
        scheme.projection_eqs = projection_eqs;
        Ok(())
    }

    fn infer_item(
        &mut self,
        env: &mut Env<'a>,
        item: &'a ItemKind<'a>,
        region: Region,
    ) -> Result<(), Error> {
        match item {
            ItemKind::Fn(function) => {
                self.infer_value_item(env, item, region)?;
                self.generalize_global(env, function.name);
            }
            ItemKind::Extern(alder_ast::ExternDecl::Fn { name, .. }) => {
                self.infer_value_item(env, item, region)?;
                self.generalize_global(env, *name);
            }
            ItemKind::Let(decl) => {
                self.infer_value_item(env, item, region)?;
                for binding in decl.bindings {
                    self.generalize_global(env, *binding);
                }
            }
            ItemKind::Component(component) => {
                self.infer_value_item(env, item, region)?;
                self.generalize_global(env, component.name);
            }
            ItemKind::Impl(impl_) => {
                self.require_impl_superclasses(impl_, region);
                for item in impl_.items {
                    if let alder_ast::ImplItem::Fn(function) = item {
                        self.infer_function(
                            env,
                            FunctionInput {
                                params: function.params,
                                ret: function.ret,
                                constraints: function.constraints,
                                context: FunctionContext::Impl {
                                    implementation: impl_,
                                    method: function,
                                },
                                body: function.body,
                                region,
                            },
                        )?;
                    }
                }
            }
            ItemKind::Trait(trait_) => {
                for item in trait_.items {
                    if let alder_ast::TraitItem::Fn(function) = item
                        && let Some(body) = function.body
                    {
                        self.infer_function(
                            env,
                            FunctionInput {
                                params: function.params,
                                ret: function.ret,
                                constraints: function.constraints,
                                context: FunctionContext::Default(trait_),
                                body,
                                region,
                            },
                        )?;
                    }
                }
            }
            ItemKind::Test(test) => {
                self.infer_block(&mut env.clone(), test.body, None)?;
            }
            ItemKind::Tests(items) => {
                let mut nested = env.clone();
                for item in *items {
                    self.infer_item(&mut nested, &item.value.kind, item.region)?;
                }
            }
            ItemKind::TypeAlias(_)
            | ItemKind::Enum(_)
            | ItemKind::ErrorGroup(_)
            | ItemKind::Table(_)
            | ItemKind::Schema(_)
            | ItemKind::Macro(_)
            | ItemKind::Comptime(_)
            | ItemKind::Extern(alder_ast::ExternDecl::Type { .. }) => {}
        }
        Ok(())
    }

    fn infer_value_item(
        &mut self,
        env: &mut Env<'a>,
        item: &'a ItemKind<'a>,
        region: Region,
    ) -> Result<(), Error> {
        match item {
            ItemKind::Fn(function) => {
                let (typ, predicates, projection_eqs) = self.infer_function(
                    env,
                    FunctionInput {
                        params: function.params,
                        ret: function.ret,
                        constraints: function.constraints,
                        context: FunctionContext::Ordinary,
                        body: function.body,
                        region,
                    },
                )?;
                let scheme = env
                    .globals
                    .get_mut(&function.name)
                    .expect("function was predeclared");
                scheme.predicates = predicates;
                scheme.projection_eqs = projection_eqs;
                self.unify_global(env, function.name, typ, region)
            }
            ItemKind::Extern(alder_ast::ExternDecl::Fn {
                name,
                params,
                ret,
                constraints,
                ..
            }) => {
                let mut vars = BTreeMap::new();
                let mut args = Vec::with_capacity(params.len());
                for param in *params {
                    args.push(match param.annotation {
                        Some(typ) => self.from_ast(typ, &mut vars),
                        None => self.fresh(),
                    });
                }
                let ret = self.from_ast(ret, &mut vars);
                let predicates = self.predicates_from_constraints(constraints, &vars);
                let projection_eqs =
                    self.projection_equations_from_constraints(constraints, &vars)?;
                let scheme = env
                    .globals
                    .get_mut(name)
                    .expect("extern function was predeclared");
                scheme.predicates = predicates;
                scheme.projection_eqs = projection_eqs;
                self.unify_global(env, *name, Ty::Fn(args, Box::new(ret)), region)
            }
            ItemKind::Let(decl) => {
                let mut value = self.infer_expr(env, decl.value, None)?;
                if let Some(annotation) = decl.annotation {
                    let annotated = self.from_ast(annotation, &mut BTreeMap::new());
                    self.unify(value.clone(), annotated, annotation.region)?;
                    value = self.prune(value);
                }
                self.infer_pattern(env, decl.pattern, value, true)
            }
            ItemKind::Component(component) => {
                let (typ, predicates, projection_eqs) = self.infer_function(
                    env,
                    FunctionInput {
                        params: component.params,
                        ret: None,
                        constraints: &[],
                        context: FunctionContext::Ordinary,
                        body: component.body,
                        region,
                    },
                )?;
                debug_assert!(predicates.is_empty());
                debug_assert!(projection_eqs.is_empty());
                let Ty::Fn(args, inferred) = typ else {
                    unreachable!()
                };
                self.unify(*inferred, self.named("Html", Vec::new()), region)?;
                self.unify_global(
                    env,
                    component.name,
                    Ty::Fn(args, Box::new(self.named("Html", Vec::new()))),
                    region,
                )
            }
            _ => unreachable!("only value items are inferred by value SCCs"),
        }
    }

    fn infer_function(
        &mut self,
        env: &Env<'a>,
        input: FunctionInput<'a>,
    ) -> Result<(Ty<'a>, Vec<Predicate<'a>>, Vec<ProjectionEquation<'a>>), Error> {
        let FunctionInput {
            params,
            ret,
            constraints,
            context,
            body,
            region,
        } = input;
        let mut local = env.clone();
        let mut vars = BTreeMap::new();
        let mut args = Vec::with_capacity(params.len());
        for param in params {
            let typ = match param.annotation {
                Some(annotation) => self.from_ast(annotation, &mut vars),
                None => self.fresh(),
            };
            self.infer_pattern(&mut local, param.pattern, typ.clone(), false)?;
            args.push(typ);
        }
        let result = match ret {
            Some(ret) => self.from_ast(ret, &mut vars),
            None => self.fresh(),
        };
        let predicates = self.predicates_from_constraints(constraints, &vars);
        let local_projection_equations =
            self.projection_equations_from_constraints(constraints, &vars)?;
        let outer_givens = std::mem::take(&mut self.givens);
        let outer_projection_equations = std::mem::take(&mut self.projection_equations);
        self.givens = outer_givens.clone();
        self.projection_equations = outer_projection_equations.clone();
        self.projection_equations
            .extend(local_projection_equations.clone());
        match context {
            FunctionContext::Ordinary => {
                self.add_parameter_givens(&predicates, 0);
                self.add_parameter_superclass_givens(&predicates, 0);
            }
            FunctionContext::Impl { implementation, .. } => {
                for binding in implementation.assoc_bindings {
                    let projection = alder_ast::ProjectionType {
                        trait_ref: implementation.trait_ref,
                        assoc: binding.assoc,
                    };
                    let projection = self.projection_from_ast(projection, &mut vars);
                    let typ = self.from_ast(binding.typ, &mut vars);
                    self.projection_equations
                        .push(ProjectionEquation { projection, typ });
                }
                let self_predicate =
                    self.predicate_from_trait_ref(implementation.trait_ref, &mut vars);
                self.givens.push(Given {
                    predicate: self_predicate.clone(),
                    evidence: Evidence::SelfDictionary,
                });
                self.add_superclass_givens(&self_predicate);
                let prerequisites = implementation
                    .trait_predicates
                    .iter()
                    .map(|predicate| self.predicate_from_trait_ref(*predicate, &mut vars))
                    .collect::<Vec<_>>();
                self.add_parameter_givens(&prerequisites, 0);
                self.add_parameter_superclass_givens(&prerequisites, 0);
                self.add_parameter_givens(&predicates, prerequisites.len());
                self.add_parameter_superclass_givens(&predicates, prerequisites.len());
            }
            FunctionContext::Default(trait_) => {
                let self_predicate = Predicate {
                    trait_: trait_.id,
                    args: trait_
                        .type_params
                        .iter()
                        .map(|parameter| {
                            vars.get(parameter.name.value)
                                .cloned()
                                .unwrap_or_else(|| self.fresh())
                        })
                        .collect(),
                };
                self.givens.push(Given {
                    predicate: self_predicate.clone(),
                    evidence: Evidence::SelfDictionary,
                });
                self.add_superclass_givens(&self_predicate);
                self.add_parameter_givens(&predicates, 0);
                self.add_parameter_superclass_givens(&predicates, 0);
            }
        }
        let body_result = match self.prune(result.clone()) {
            Ty::App(head, args)
                if matches!(*head, Ty::Con(reference) if reference.name == "Task")
                    && args.len() == 1 =>
            {
                args.into_iter().next().expect("length checked")
            }
            _ => result.clone(),
        };
        let inferred = (|| {
            let body_type = self.infer_block(&mut local, body, Some(body_result.clone()))?;
            if body.value.tail.is_some() || !block_contains_return(body) {
                self.unify(body_type, body_result, region)?;
            }
            let function_type = Ty::Fn(args, Box::new(self.prune(result)));
            if let FunctionContext::Impl {
                implementation,
                method,
            } = context
            {
                let mut expected_vars = BTreeMap::new();
                if let Some(header) = self.database.trait_(implementation.trait_ref.trait_) {
                    for (parameter, argument) in
                        header.params.iter().zip(implementation.trait_ref.args)
                    {
                        expected_vars
                            .insert(parameter.name.value, self.from_ast(argument, &mut vars));
                    }
                }
                let expected = self.from_ast(method.scheme.typ, &mut expected_vars);
                self.unify(function_type.clone(), expected, method.name.region)?;
            }
            Ok((function_type, predicates, local_projection_equations))
        })();
        self.givens = outer_givens;
        self.projection_equations = outer_projection_equations;
        inferred
    }

    fn infer_block(
        &mut self,
        env: &mut Env<'a>,
        block: &'a Located<Block<'a>>,
        return_type: Option<Ty<'a>>,
    ) -> Result<Ty<'a>, Error> {
        for statement in block.value.statements {
            self.infer_stmt(env, statement, return_type.clone())?;
        }
        match block.value.tail {
            Some(tail) => self.infer_expr(env, tail, return_type),
            None => Ok(Ty::Unit),
        }
    }

    fn infer_stmt(
        &mut self,
        env: &mut Env<'a>,
        statement: &'a Located<Stmt<'a>>,
        return_type: Option<Ty<'a>>,
    ) -> Result<(), Error> {
        match &statement.value {
            Stmt::Let(decl) => {
                let value = self.infer_expr(env, decl.value, return_type.clone())?;
                if let Some(annotation) = decl.annotation {
                    let annotated = self.from_ast(annotation, &mut BTreeMap::new());
                    self.unify(value.clone(), annotated, annotation.region)?;
                }
                self.infer_pattern(env, decl.pattern, value, false)?;
            }
            Stmt::Use { .. } => {}
            Stmt::Assign {
                use_id,
                place,
                value,
                ..
            } => {
                let expected = self.place_type(env, place, statement.region)?;
                let actual = self.infer_expr(env, value, return_type.clone())?;
                self.unify(actual, expected.clone(), statement.region)?;
                if let Some(use_id) = use_id {
                    self.record_builtin_obligation(
                        *use_id,
                        expected,
                        statement.region,
                        ObligationAction::CompoundAssign,
                    );
                }
            }
            Stmt::For {
                pattern,
                iter,
                body,
            } => {
                let item = self.fresh();
                let iter_type = self.infer_expr(env, iter, return_type.clone())?;
                self.unify(
                    iter_type,
                    self.named("Array", vec![item.clone()]),
                    iter.region,
                )?;
                let mut nested = env.clone();
                self.infer_pattern(&mut nested, pattern, item, false)?;
                self.infer_block(&mut nested, body, return_type)?;
            }
            Stmt::While { condition, body } => {
                let condition_type = self.infer_expr(env, condition, return_type.clone())?;
                self.unify(
                    condition_type,
                    self.named("Bool", Vec::new()),
                    condition.region,
                )?;
                self.infer_block(&mut env.clone(), body, return_type)?;
            }
            Stmt::Return(value) => {
                let expected = return_type.unwrap_or(Ty::Unit);
                let actual = match value {
                    Some(value) => self.infer_expr(env, value, Some(expected.clone()))?,
                    None => Ty::Unit,
                };
                self.unify(actual, expected, statement.region)?;
            }
            Stmt::Break(value) => {
                if let Some(value) = value {
                    self.infer_expr(env, value, return_type)?;
                }
            }
            Stmt::Continue => {}
            Stmt::Assert(expr) => {
                let actual = self.infer_expr(env, expr, return_type)?;
                self.unify(actual, self.named("Bool", Vec::new()), expr.region)?;
            }
            Stmt::Expr(expr) => {
                self.infer_expr(env, expr, return_type)?;
            }
        }
        Ok(())
    }

    fn infer_expr(
        &mut self,
        env: &Env<'a>,
        expression: &'a Located<Expr<'a>>,
        return_type: Option<Ty<'a>>,
    ) -> Result<Ty<'a>, Error> {
        let region = expression.region;
        match &expression.value {
            Expr::Number { .. } => Ok(self.named("Number", Vec::new())),
            Expr::BigInt(_) => Ok(self.named("BigInt", Vec::new())),
            Expr::Str(_) | Expr::Template(_) | Expr::TaggedTemplate { .. } => {
                if let Expr::Template(parts) | Expr::TaggedTemplate { parts, .. } =
                    &expression.value
                {
                    for part in *parts {
                        if let alder_ast::TemplatePart::Expr(expr) = part {
                            self.infer_expr(env, expr, return_type.clone())?;
                        }
                    }
                }
                Ok(self.named("String", Vec::new()))
            }
            Expr::Bool(_) => Ok(self.named("Bool", Vec::new())),
            Expr::Unit => Ok(Ty::Unit),
            Expr::Var { use_id, reference } => {
                self.infer_reference(env, *use_id, *reference, region)
            }
            Expr::Constructor(constructor) => {
                Ok(self.instantiate_annotation(constructor.annotation))
            }
            Expr::Tag { args, .. } => {
                for arg in *args {
                    self.infer_expr(env, arg, return_type.clone())?;
                }
                Ok(Ty::ErrorRow)
            }
            Expr::Array(items) => {
                let item_type = self.fresh();
                for item in *items {
                    let actual = self.infer_expr(env, item, return_type.clone())?;
                    self.unify(actual, item_type.clone(), item.region)?;
                }
                let item_type = self.prune(item_type);
                Ok(self.named("Array", vec![item_type]))
            }
            Expr::Tuple(items) => {
                let mut types = Vec::with_capacity(items.len());
                for item in *items {
                    types.push(self.infer_expr(env, item, return_type.clone())?);
                }
                Ok(Ty::Tuple(types))
            }
            Expr::Record(fields) => self.infer_record(env, fields, return_type),
            Expr::RecordConstructor {
                constructor,
                fields,
            } => {
                let actual = self.infer_record(env, fields, return_type)?;
                let Ty::Record(actual_fields, _) = actual else {
                    unreachable!("record inference always returns a record")
                };
                let constructor_type = self.instantiate_annotation(constructor.annotation);
                let alder_ast::VariantPayload::Record(expected_fields) = constructor.payload else {
                    unreachable!("record constructor carries a record payload")
                };
                match constructor_type {
                    Ty::Fn(expected_types, result)
                        if expected_types.len() == expected_fields.len() =>
                    {
                        for (field, expected) in expected_fields.iter().zip(expected_types) {
                            let Some((_, actual)) = actual_fields.get(field.name) else {
                                if field.presence == FieldPresence::Optional {
                                    continue;
                                }
                                return Err(Error {
                                    region,
                                    kind: ErrorKind::MissingField {
                                        field: field.name.to_owned(),
                                    },
                                });
                            };
                            self.unify(actual.clone(), expected, field.typ.region)?;
                        }
                        Ok(self.prune(*result))
                    }
                    result if expected_fields.is_empty() => Ok(result),
                    actual => {
                        Err(self.mismatch(region, actual, Ty::Fn(Vec::new(), Box::new(Ty::Any))))
                    }
                }
            }
            Expr::Call {
                use_id,
                function,
                arguments,
            } => {
                let (callee_use, target) = match function.value {
                    Expr::Var {
                        use_id,
                        reference: ValueRef::TopLevel(name),
                    }
                    | Expr::Var {
                        use_id,
                        reference:
                            ValueRef::Foreign {
                                reference: name, ..
                            },
                    }
                    | Expr::Var {
                        use_id,
                        reference: ValueRef::Builtin(name),
                    } => (Some(use_id), Some(DirectTarget::Binding(name))),
                    Expr::Var {
                        use_id,
                        reference: ValueRef::TraitMethod { method, .. },
                    } => (Some(use_id), Some(DirectTarget::TraitMethod(method))),
                    Expr::Var { use_id, .. } => (Some(use_id), None),
                    _ => (None, None),
                };
                let function_type = self.infer_expr(env, function, return_type.clone())?;
                let mut args = Vec::with_capacity(arguments.len());
                for argument in *arguments {
                    args.push(self.infer_expr(env, argument, return_type.clone())?);
                }
                let result = self.fresh();
                self.unify(
                    function_type,
                    Ty::Fn(args, Box::new(result.clone())),
                    region,
                )?;
                self.calls.push(CallSite {
                    use_id: *use_id,
                    callee_use,
                    target,
                });
                Ok(self.prune(result))
            }
            Expr::Access { record, field } => {
                let record_type = self.infer_expr(env, record, return_type)?;
                self.access_field(record_type, field.value, field.region)
            }
            Expr::TupleAccess { tuple, index } => {
                let tuple_type = self.infer_expr(env, tuple, return_type)?;
                let tuple_type = self.prune(tuple_type);
                match tuple_type {
                    Ty::Tuple(items) if (index.value as usize) < items.len() => {
                        Ok(items[index.value as usize].clone())
                    }
                    Ty::Var(id) => {
                        let mut items = Vec::with_capacity(index.value as usize + 1);
                        for _ in 0..=index.value {
                            items.push(self.fresh());
                        }
                        let result = items[index.value as usize].clone();
                        self.bind(id, Ty::Tuple(items), region)?;
                        Ok(result)
                    }
                    actual => Err(self.mismatch(region, actual, Ty::Tuple(Vec::new()))),
                }
            }
            Expr::Index { target, index } => {
                let item = self.fresh();
                let target_type = self.infer_expr(env, target, return_type.clone())?;
                self.unify(
                    target_type,
                    self.named("Array", vec![item.clone()]),
                    target.region,
                )?;
                let index_type = self.infer_expr(env, index, return_type)?;
                self.unify(index_type, self.named("Number", Vec::new()), index.region)?;
                Ok(self.prune(item))
            }
            Expr::Await(expr) => {
                let value = self.fresh();
                let actual = self.infer_expr(env, expr, return_type)?;
                self.unify(actual, self.named("Task", vec![value.clone()]), region)?;
                Ok(self.prune(value))
            }
            Expr::Try(expr) => {
                let value = self.fresh();
                let error = self.fresh();
                let actual = self.infer_expr(env, expr, return_type)?;
                self.unify(
                    actual,
                    self.named("Result", vec![value.clone(), error]),
                    region,
                )?;
                Ok(self.prune(value))
            }
            Expr::Pin(expr) | Expr::State(expr) => self.infer_expr(env, expr, return_type),
            Expr::Negate { use_id, expr } => {
                let actual = self.infer_expr(env, expr, return_type)?;
                self.record_builtin_obligation(
                    *use_id,
                    actual.clone(),
                    region,
                    ObligationAction::Operator,
                );
                Ok(self.prune(actual))
            }
            Expr::Not(expr) => {
                let actual = self.infer_expr(env, expr, return_type)?;
                self.unify(actual, self.named("Bool", Vec::new()), region)?;
                Ok(self.named("Bool", Vec::new()))
            }
            Expr::Binop {
                use_id,
                op,
                left,
                right,
            } => self.infer_binop(env, *use_id, op.value, left, right, return_type),
            Expr::Block(block) => self.infer_block(&mut env.clone(), block, return_type),
            Expr::Lambda { params, ret, body } => {
                let mut local = env.clone();
                let mut vars = BTreeMap::new();
                let mut args = Vec::with_capacity(params.len());
                for param in *params {
                    let typ = param
                        .annotation
                        .map(|annotation| self.from_ast(annotation, &mut vars))
                        .unwrap_or_else(|| self.fresh());
                    self.infer_pattern(&mut local, param.pattern, typ.clone(), false)?;
                    args.push(typ);
                }
                let result = ret
                    .map(|ret| self.from_ast(ret, &mut vars))
                    .unwrap_or_else(|| self.fresh());
                let body_type = self.infer_expr(&local, body, Some(result.clone()))?;
                self.unify(body_type, result.clone(), region)?;
                Ok(Ty::Fn(args, Box::new(self.prune(result))))
            }
            Expr::If {
                branches,
                final_else,
            } => {
                let result = self.fresh();
                for branch in *branches {
                    let condition = self.infer_expr(env, branch.condition, return_type.clone())?;
                    self.unify(
                        condition,
                        self.named("Bool", Vec::new()),
                        branch.condition.region,
                    )?;
                    let body =
                        self.infer_block(&mut env.clone(), branch.body, return_type.clone())?;
                    self.unify(body, result.clone(), branch.body.region)?;
                }
                if let Some(final_else) = final_else {
                    let body = self.infer_block(&mut env.clone(), final_else, return_type)?;
                    self.unify(body, result.clone(), final_else.region)?;
                } else {
                    self.unify(Ty::Unit, result.clone(), region)?;
                }
                Ok(self.prune(result))
            }
            Expr::Match { scrutinee, arms } => {
                let scrutinee_type = self.infer_expr(env, scrutinee, return_type.clone())?;
                let result = self.fresh();
                for arm in *arms {
                    let mut local = env.clone();
                    for pattern in arm.patterns {
                        self.infer_pattern(&mut local, pattern, scrutinee_type.clone(), false)?;
                    }
                    if let Some(guard) = arm.guard {
                        let guard_type = self.infer_expr(&local, guard, return_type.clone())?;
                        self.unify(guard_type, self.named("Bool", Vec::new()), guard.region)?;
                    }
                    let body = self.infer_expr(&local, arm.body, return_type.clone())?;
                    self.unify(body, result.clone(), arm.body.region)?;
                }
                Ok(self.prune(result))
            }
            Expr::Loop(block) => {
                self.infer_block(&mut env.clone(), block, return_type)?;
                Ok(Ty::Unit)
            }
            Expr::Provide { value, body, .. } => {
                self.infer_expr(env, value, return_type.clone())?;
                self.infer_block(&mut env.clone(), body, return_type)
            }
            Expr::Style(style) => {
                for entry in style.entries {
                    self.infer_style_value(env, entry.value, return_type.clone())?;
                }
                Ok(self.named("Style", Vec::new()))
            }
            Expr::Query(query) => {
                self.infer_query_pins(env, query, return_type)?;
                let result = self.fresh();
                Ok(self.named("Query", vec![result]))
            }
            Expr::Markup(markup) => {
                match markup {
                    alder_ast::Markup::Element(element) => {
                        self.infer_element(env, element, return_type)?
                    }
                    alder_ast::Markup::Fragment(children) => {
                        for child in *children {
                            self.infer_child(env, child, return_type.clone())?;
                        }
                    }
                }
                Ok(self.named("Html", Vec::new()))
            }
            Expr::MacroCall { .. } => Ok(Ty::Any),
        }
    }

    fn infer_reference(
        &mut self,
        env: &Env<'a>,
        use_id: UseId,
        reference: ValueRef<'a>,
        region: Region,
    ) -> Result<Ty<'a>, Error> {
        match reference {
            ValueRef::Local(local) => Ok(self.instantiate(&env.locals[&local.id.0])),
            ValueRef::TopLevel(name) => {
                let (typ, predicates) = self.instantiate_scheme(&env.globals[&name]);
                self.record_predicates(
                    use_id,
                    predicates,
                    region,
                    ObligationAction::Reference(None),
                );
                Ok(typ)
            }
            ValueRef::Foreign { annotation, .. } => {
                let (typ, vars) = self.instantiate_annotation_with_vars(annotation);
                self.record_annotation_predicates(
                    use_id,
                    annotation,
                    &vars,
                    region,
                    ObligationAction::Reference(None),
                );
                Ok(typ)
            }
            ValueRef::TraitMethod { method, annotation } => {
                let seed = self
                    .requirement_seeds
                    .get(&use_id)
                    .copied()
                    .expect("constraint generation must seed every trait method reference");
                assert_eq!(seed.kind, RequirementKind::TraitMethod(method));
                let origin = if seed.region == Region::zero() {
                    region
                } else {
                    seed.region
                };
                let (typ, vars) = self.instantiate_annotation_with_vars(annotation);
                if let Some(header) = self.database.trait_(method.trait_) {
                    let args = header
                        .params
                        .iter()
                        .map(|parameter| {
                            vars.get(parameter.name.value)
                                .cloned()
                                .unwrap_or_else(|| self.fresh())
                        })
                        .collect();
                    self.obligations.push(Obligation {
                        use_id: Some(use_id),
                        predicate: Predicate {
                            trait_: method.trait_,
                            args,
                        },
                        region: origin,
                        action: ObligationAction::Reference(Some(method)),
                        givens: self.givens.clone(),
                    });
                }
                self.record_annotation_predicates(
                    use_id,
                    annotation,
                    &vars,
                    origin,
                    ObligationAction::Reference(Some(method)),
                );
                Ok(typ)
            }
            ValueRef::Module(_)
            | ValueRef::Builtin(_)
            | ValueRef::Provider(_)
            | ValueRef::QueryName(_)
            | ValueRef::Opaque(_) => Ok(Ty::Any),
        }
    }

    fn infer_record(
        &mut self,
        env: &Env<'a>,
        fields: &'a [RecordField<'a>],
        return_type: Option<Ty<'a>>,
    ) -> Result<Ty<'a>, Error> {
        let mut result = BTreeMap::new();
        for field in fields {
            match field {
                RecordField::Field { name, value } => {
                    let typ = self.infer_expr(env, value, return_type.clone())?;
                    result.insert(name.value, (FieldPresence::Required, typ));
                }
                RecordField::Spread(expr) => {
                    let spread = self.infer_expr(env, expr, return_type.clone())?;
                    if let Ty::Record(fields, _) = self.prune(spread) {
                        result.extend(fields);
                    }
                }
            }
        }
        Ok(Ty::Record(result, false))
    }

    fn infer_pattern(
        &mut self,
        env: &mut Env<'a>,
        pattern: &'a Located<Pattern<'a>>,
        expected: Ty<'a>,
        top_level: bool,
    ) -> Result<(), Error> {
        match &pattern.value {
            Pattern::Anything => {}
            Pattern::Bind(binding) => match binding {
                BindingName::Local(local) => {
                    env.locals.insert(
                        local.id.0,
                        Scheme {
                            quantified: Vec::new(),
                            predicates: Vec::new(),
                            projection_eqs: Vec::new(),
                            typ: expected,
                        },
                    );
                }
                BindingName::TopLevel(name) => {
                    if top_level {
                        self.unify_global(env, *name, expected, pattern.region)?;
                    }
                }
            },
            Pattern::Pin {
                use_id,
                value: expr,
            } => {
                let actual = self.infer_expr(env, expr, None)?;
                self.unify(actual, expected.clone(), pattern.region)?;
                self.record_builtin_obligation(
                    *use_id,
                    expected,
                    pattern.region,
                    ObligationAction::Pin,
                );
            }
            Pattern::Number { .. } => {
                self.unify(expected, self.named("Number", Vec::new()), pattern.region)?;
            }
            Pattern::BigInt(_) => {
                self.unify(expected, self.named("BigInt", Vec::new()), pattern.region)?;
            }
            Pattern::Str(_) => {
                self.unify(expected, self.named("String", Vec::new()), pattern.region)?;
            }
            Pattern::Bool(_) => {
                self.unify(expected, self.named("Bool", Vec::new()), pattern.region)?;
            }
            Pattern::Unit => self.unify(expected, Ty::Unit, pattern.region)?,
            Pattern::Constructor { constructor, args } => {
                let constructor_type = self.instantiate_annotation(constructor.annotation);
                if args.is_empty() {
                    self.unify(constructor_type, expected, pattern.region)?;
                } else {
                    let mut arg_types = Vec::with_capacity(args.len());
                    for _ in *args {
                        arg_types.push(self.fresh());
                    }
                    self.unify(
                        constructor_type,
                        Ty::Fn(arg_types.clone(), Box::new(expected)),
                        pattern.region,
                    )?;
                    for (arg, typ) in args.iter().zip(arg_types) {
                        self.infer_pattern(env, arg, typ, false)?;
                    }
                }
            }
            Pattern::ConstructorRecord {
                constructor,
                fields,
                ..
            } => {
                let constructor_type = self.instantiate_annotation(constructor.annotation);
                let declared = match constructor.payload {
                    alder_ast::VariantPayload::Record(fields) => fields,
                    _ => &[],
                };
                let mut arg_types = Vec::with_capacity(declared.len());
                for _ in declared {
                    arg_types.push(self.fresh());
                }
                self.unify(
                    constructor_type,
                    Ty::Fn(arg_types.clone(), Box::new(expected)),
                    pattern.region,
                )?;
                for field in *fields {
                    if let Some(index) = declared
                        .iter()
                        .position(|declared| declared.name == field.name.value)
                    {
                        self.infer_pattern(env, field.pattern, arg_types[index].clone(), false)?;
                    }
                }
            }
            Pattern::Record { fields, .. } => {
                let mut record = BTreeMap::new();
                for field in *fields {
                    let typ = self.fresh();
                    self.infer_pattern(env, field.pattern, typ.clone(), false)?;
                    record.insert(field.name.value, (FieldPresence::Required, typ));
                }
                self.unify(expected, Ty::Record(record, true), pattern.region)?;
            }
            Pattern::Tag { args, .. } => {
                for arg in *args {
                    let typ = self.fresh();
                    self.infer_pattern(env, arg, typ, false)?;
                }
                self.unify(expected, Ty::ErrorRow, pattern.region)?;
            }
            Pattern::Tuple(items) => {
                let mut types = Vec::with_capacity(items.len());
                for item in *items {
                    let typ = self.fresh();
                    self.infer_pattern(env, item, typ.clone(), false)?;
                    types.push(typ);
                }
                self.unify(expected, Ty::Tuple(types), pattern.region)?;
            }
            Pattern::Array { elements, rest } => {
                let item = self.fresh();
                for element in *elements {
                    self.infer_pattern(env, element, item.clone(), false)?;
                }
                if let Some(rest) = rest.and_then(|rest| rest.name)
                    && let BindingName::Local(local) = rest
                {
                    env.locals.insert(
                        local.id.0,
                        Scheme {
                            quantified: Vec::new(),
                            predicates: Vec::new(),
                            projection_eqs: Vec::new(),
                            typ: self.named("Array", vec![item.clone()]),
                        },
                    );
                }
                self.unify(expected, self.named("Array", vec![item]), pattern.region)?;
            }
            Pattern::Alias { pattern, name } => {
                self.infer_pattern(env, pattern, expected.clone(), false)?;
                if let BindingName::Local(local) = name {
                    env.locals.insert(
                        local.id.0,
                        Scheme {
                            quantified: Vec::new(),
                            predicates: Vec::new(),
                            projection_eqs: Vec::new(),
                            typ: expected,
                        },
                    );
                }
            }
        }
        Ok(())
    }

    fn infer_binop(
        &mut self,
        env: &Env<'a>,
        use_id: UseId,
        op: BinOp,
        left: &'a Located<Expr<'a>>,
        right: &'a Located<Expr<'a>>,
        return_type: Option<Ty<'a>>,
    ) -> Result<Ty<'a>, Error> {
        let left_type = self.infer_expr(env, left, return_type.clone())?;
        let right_type = self.infer_expr(env, right, return_type)?;
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                self.unify(left_type.clone(), right_type, right.region)?;
                self.record_builtin_obligation(
                    use_id,
                    left_type.clone(),
                    left.region,
                    ObligationAction::Operator,
                );
                Ok(self.prune(left_type))
            }
            BinOp::Eq | BinOp::NotEq => {
                self.unify(left_type.clone(), right_type, right.region)?;
                self.record_builtin_obligation(
                    use_id,
                    left_type,
                    left.region,
                    ObligationAction::Operator,
                );
                Ok(self.named("Bool", Vec::new()))
            }
            BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => {
                self.unify(left_type.clone(), right_type, right.region)?;
                self.record_builtin_obligation(
                    use_id,
                    left_type,
                    left.region,
                    ObligationAction::Operator,
                );
                Ok(self.named("Bool", Vec::new()))
            }
            BinOp::And | BinOp::Or => {
                let bool_ = self.named("Bool", Vec::new());
                self.unify(left_type, bool_.clone(), left.region)?;
                self.unify(right_type, bool_.clone(), right.region)?;
                Ok(bool_)
            }
            BinOp::Coalesce => {
                self.unify(left_type.clone(), right_type, right.region)?;
                Ok(self.prune(left_type))
            }
            BinOp::Pipe => {
                let result = self.fresh();
                self.unify(
                    right_type,
                    Ty::Fn(vec![left_type], Box::new(result.clone())),
                    right.region,
                )?;
                Ok(self.prune(result))
            }
            BinOp::In => Ok(self.named("Bool", Vec::new())),
        }
    }

    fn record_builtin_obligation(
        &mut self,
        use_id: UseId,
        subject: Ty<'a>,
        fallback_region: Region,
        action: ObligationAction<'a>,
    ) {
        let seed = self
            .requirement_seeds
            .get(&use_id)
            .copied()
            .expect("constraint generation must seed every built-in evidence site");
        let trait_name = match seed.kind {
            RequirementKind::Eq => "Eq",
            RequirementKind::Ord => "Ord",
            RequirementKind::Num => "Num",
            RequirementKind::TraitMethod(_) => {
                panic!("trait method seed used for a built-in operator")
            }
        };
        self.obligations.push(Obligation {
            use_id: Some(use_id),
            predicate: Predicate {
                trait_: builtin_trait_id(trait_name),
                args: vec![subject],
            },
            region: if seed.region == Region::zero() {
                fallback_region
            } else {
                seed.region
            },
            action,
            givens: self.givens.clone(),
        });
    }

    fn place_type(
        &mut self,
        env: &Env<'a>,
        place: &'a alder_ast::Place<'a>,
        region: Region,
    ) -> Result<Ty<'a>, Error> {
        let mut typ = match place.root {
            BindingName::Local(local) => self.instantiate(&env.locals[&local.id.0]),
            BindingName::TopLevel(name) => self.instantiate(&env.globals[&name]),
        };
        for step in place.steps {
            typ = match step {
                alder_ast::PlaceStep::Field(field) => {
                    self.access_field(typ, field.value, field.region)?
                }
                alder_ast::PlaceStep::TupleIndex(index) => match self.prune(typ) {
                    Ty::Tuple(items) if (index.value as usize) < items.len() => {
                        items[index.value as usize].clone()
                    }
                    actual => return Err(self.mismatch(region, actual, Ty::Tuple(Vec::new()))),
                },
                alder_ast::PlaceStep::Index(index) => {
                    let item = self.fresh();
                    self.unify(typ, self.named("Array", vec![item.clone()]), region)?;
                    let index_type = self.infer_expr(env, index, None)?;
                    self.unify(index_type, self.named("Number", Vec::new()), index.region)?;
                    item
                }
            }
        }
        Ok(typ)
    }

    fn access_field(
        &mut self,
        record: Ty<'a>,
        field: &'a str,
        region: Region,
    ) -> Result<Ty<'a>, Error> {
        match self.prune(record) {
            Ty::Record(fields, _) => match fields.get(field) {
                Some((FieldPresence::Required, typ)) => Ok(typ.clone()),
                Some((FieldPresence::Optional, typ)) => Ok(self.named("Option", vec![typ.clone()])),
                None => Err(Error {
                    region,
                    kind: ErrorKind::MissingField {
                        field: field.to_owned(),
                    },
                }),
            },
            Ty::Var(id) => {
                let result = self.fresh();
                let fields = BTreeMap::from([(field, (FieldPresence::Required, result.clone()))]);
                self.bind(id, Ty::Record(fields, true), region)?;
                Ok(result)
            }
            Ty::Any => Ok(Ty::Any),
            actual => Err(self.mismatch(region, actual, Ty::Record(BTreeMap::new(), true))),
        }
    }

    fn infer_style_value(
        &mut self,
        env: &Env<'a>,
        value: alder_ast::StyleValue<'a>,
        return_type: Option<Ty<'a>>,
    ) -> Result<(), Error> {
        match value {
            alder_ast::StyleValue::Dimension { .. } => {}
            alder_ast::StyleValue::Expr(expr) => {
                self.infer_expr(env, expr, return_type)?;
            }
            alder_ast::StyleValue::Nested(style) => {
                for entry in style.entries {
                    self.infer_style_value(env, entry.value, return_type.clone())?;
                }
            }
        }
        Ok(())
    }

    fn infer_query_pins(
        &mut self,
        env: &Env<'a>,
        query: &'a alder_ast::Query<'a>,
        return_type: Option<Ty<'a>>,
    ) -> Result<(), Error> {
        let mut infer = |expr: &'a Located<Expr<'a>>| {
            self.infer_expr(env, expr, return_type.clone()).map(|_| ())
        };
        match query {
            alder_ast::Query::Select(select) => {
                if let alder_ast::Projection::Fields(fields) = select.projection {
                    for field in fields {
                        infer(field)?;
                    }
                }
                for join in select.joins {
                    infer(join.on)?;
                }
                if let Some(expr) = select.where_ {
                    infer(expr)?;
                }
                for expr in select.group_by {
                    infer(expr)?;
                }
                for order in select.order_by {
                    infer(order.expr)?;
                }
                if let Some(expr) = select.limit {
                    infer(expr)?;
                }
                if let Some(expr) = select.offset {
                    infer(expr)?;
                }
            }
            alder_ast::Query::Insert { values, .. } => infer(values)?,
            alder_ast::Query::Update { set, where_, .. } => {
                for field in *set {
                    match field {
                        RecordField::Field { value, .. } | RecordField::Spread(value) => {
                            infer(value)?
                        }
                    }
                }
                if let Some(expr) = where_ {
                    infer(expr)?;
                }
            }
            alder_ast::Query::Delete { where_, .. } => {
                if let Some(expr) = where_ {
                    infer(expr)?;
                }
            }
        }
        Ok(())
    }

    fn infer_element(
        &mut self,
        env: &Env<'a>,
        element: &'a alder_ast::Element<'a>,
        return_type: Option<Ty<'a>>,
    ) -> Result<(), Error> {
        for attr in element.attrs {
            if let Some(alder_ast::AttrValue::Expr(expr)) = attr.value {
                self.infer_expr(env, expr, return_type.clone())?;
            }
        }
        for child in element.children {
            self.infer_child(env, child, return_type.clone())?;
        }
        Ok(())
    }

    fn infer_child(
        &mut self,
        env: &Env<'a>,
        child: &'a Located<Child<'a>>,
        return_type: Option<Ty<'a>>,
    ) -> Result<(), Error> {
        match &child.value {
            Child::Element(element) => self.infer_element(env, element, return_type)?,
            Child::Fragment(children) => {
                for child in *children {
                    self.infer_child(env, child, return_type.clone())?;
                }
            }
            Child::Text(_) => {}
            Child::Hole(expr) => {
                self.infer_expr(env, expr, return_type)?;
            }
            Child::If {
                branches,
                final_else,
            } => {
                for branch in *branches {
                    let condition = self.infer_expr(env, branch.condition, return_type.clone())?;
                    self.unify(
                        condition,
                        self.named("Bool", Vec::new()),
                        branch.condition.region,
                    )?;
                    self.infer_child_block(env, branch.body, return_type.clone())?;
                }
                if let Some(block) = final_else {
                    self.infer_child_block(env, block, return_type)?;
                }
            }
            Child::For {
                pattern,
                iter,
                key,
                body,
                empty,
            } => {
                let item = self.fresh();
                let iter_type = self.infer_expr(env, iter, return_type.clone())?;
                self.unify(
                    iter_type,
                    self.named("Array", vec![item.clone()]),
                    iter.region,
                )?;
                let mut local = env.clone();
                self.infer_pattern(&mut local, pattern, item, false)?;
                if let Some(key) = key {
                    self.infer_expr(&local, key, return_type.clone())?;
                }
                self.infer_child_block(&local, body, return_type.clone())?;
                if let Some(empty) = empty {
                    self.infer_child_block(env, empty, return_type)?;
                }
            }
            Child::Match { scrutinee, arms } => {
                let typ = self.infer_expr(env, scrutinee, return_type.clone())?;
                for arm in *arms {
                    let mut local = env.clone();
                    for pattern in arm.patterns {
                        self.infer_pattern(&mut local, pattern, typ.clone(), false)?;
                    }
                    if let Some(guard) = arm.guard {
                        let guard_type = self.infer_expr(&local, guard, return_type.clone())?;
                        self.unify(guard_type, self.named("Bool", Vec::new()), guard.region)?;
                    }
                    self.infer_child_block(&local, arm.body, return_type.clone())?;
                }
            }
        }
        Ok(())
    }

    fn infer_child_block(
        &mut self,
        env: &Env<'a>,
        block: &'a Located<ChildBlock<'a>>,
        return_type: Option<Ty<'a>>,
    ) -> Result<(), Error> {
        let mut local = env.clone();
        for item in block.value.items {
            match item {
                ChildItem::Stmt(stmt) => self.infer_stmt(&mut local, stmt, return_type.clone())?,
                ChildItem::Child(child) => self.infer_child(&local, child, return_type.clone())?,
            }
        }
        Ok(())
    }

    fn unify_global(
        &mut self,
        env: &Env<'a>,
        name: QualifiedName<'a>,
        typ: Ty<'a>,
        region: Region,
    ) -> Result<(), Error> {
        self.unify(env.globals[&name].typ.clone(), typ, region)
    }

    fn generalize_global(&mut self, env: &mut Env<'a>, name: QualifiedName<'a>) {
        let typ = self.prune(env.globals[&name].typ.clone());
        let mut predicates = env.globals[&name].predicates.clone();
        for predicate in &mut predicates {
            for argument in &mut predicate.args {
                *argument = self.prune(argument.clone());
            }
        }
        let mut projection_eqs = env.globals[&name].projection_eqs.clone();
        for equation in &mut projection_eqs {
            equation.projection = self.prune(equation.projection.clone());
            equation.typ = self.prune(equation.typ.clone());
        }
        let mut vars = BTreeSet::new();
        self.free_vars(&typ, &mut vars);
        for equation in &projection_eqs {
            self.free_vars(&equation.projection, &mut vars);
            self.free_vars(&equation.typ, &mut vars);
        }
        env.globals.insert(
            name,
            Scheme {
                quantified: vars.into_iter().collect(),
                predicates,
                projection_eqs,
                typ,
            },
        );
    }

    fn instantiate(&mut self, scheme: &Scheme<'a>) -> Ty<'a> {
        self.instantiate_scheme(scheme).0
    }

    fn instantiate_scheme(&mut self, scheme: &Scheme<'a>) -> (Ty<'a>, Vec<Predicate<'a>>) {
        let replacements: BTreeMap<_, _> = scheme
            .quantified
            .iter()
            .map(|id| (*id, self.fresh()))
            .collect();
        let typ = self.replace_vars(&scheme.typ, &replacements);
        let predicates = scheme
            .predicates
            .iter()
            .map(|predicate| Predicate {
                trait_: predicate.trait_,
                args: predicate
                    .args
                    .iter()
                    .map(|argument| self.replace_vars(argument, &replacements))
                    .collect(),
            })
            .collect();
        let projection_eqs = scheme
            .projection_eqs
            .iter()
            .map(|equation| ProjectionEquation {
                projection: self.replace_vars(&equation.projection, &replacements),
                typ: self.replace_vars(&equation.typ, &replacements),
            })
            .collect::<Vec<_>>();
        self.projection_equations.extend(projection_eqs);
        (typ, predicates)
    }

    fn instantiate_annotation(&mut self, annotation: &'a Annotation<'a>) -> Ty<'a> {
        self.instantiate_annotation_with_vars(annotation).0
    }

    fn instantiate_annotation_with_vars(
        &mut self,
        annotation: &'a Annotation<'a>,
    ) -> (Ty<'a>, BTreeMap<&'a str, Ty<'a>>) {
        let mut vars = BTreeMap::new();
        let typ = self.from_ast(annotation.typ, &mut vars);
        for equality in annotation.projection_equalities {
            let projection = self.projection_from_ast(equality.projection, &mut vars);
            let typ = self.from_ast(equality.typ, &mut vars);
            self.projection_equations
                .push(ProjectionEquation { projection, typ });
        }
        (typ, vars)
    }

    fn record_annotation_predicates(
        &mut self,
        use_id: UseId,
        annotation: &'a Annotation<'a>,
        vars: &BTreeMap<&'a str, Ty<'a>>,
        region: Region,
        action: ObligationAction<'a>,
    ) {
        let predicates = annotation
            .trait_predicates
            .iter()
            .map(|predicate| {
                let mut vars = vars.clone();
                Predicate {
                    trait_: predicate.trait_,
                    args: predicate
                        .args
                        .iter()
                        .map(|argument| self.from_ast(argument, &mut vars))
                        .collect(),
                }
            })
            .collect();
        self.record_predicates(use_id, predicates, region, action);
    }

    fn record_predicates(
        &mut self,
        use_id: UseId,
        predicates: Vec<Predicate<'a>>,
        region: Region,
        action: ObligationAction<'a>,
    ) {
        for predicate in predicates {
            self.obligations.push(Obligation {
                use_id: Some(use_id),
                predicate,
                region,
                action,
                givens: self.givens.clone(),
            });
        }
    }

    fn predicates_from_constraints(
        &mut self,
        constraints: &'a [alder_ast::TypeConstraint<'a>],
        vars: &BTreeMap<&'a str, Ty<'a>>,
    ) -> Vec<Predicate<'a>> {
        let mut predicates = Vec::new();
        for constraint in constraints {
            if let alder_ast::TypeConstraint::Bound {
                var,
                traits: trait_names,
            } = constraint
                && let Some(subject) = vars.get(var.value).cloned()
            {
                for trait_name in *trait_names {
                    predicates.push(Predicate {
                        trait_: TraitId(*trait_name),
                        args: vec![subject.clone()],
                    });
                }
            }
        }
        predicates
    }

    fn projection_equations_from_constraints(
        &mut self,
        constraints: &'a [alder_ast::TypeConstraint<'a>],
        vars: &BTreeMap<&'a str, Ty<'a>>,
    ) -> Result<Vec<ProjectionEquation<'a>>, Error> {
        let mut equations: Vec<ProjectionEquation<'a>> = Vec::new();
        for constraint in constraints {
            let alder_ast::TypeConstraint::AssocEq {
                projection,
                typ,
                region,
            } = constraint
            else {
                continue;
            };
            let mut vars = vars.clone();
            let equation = ProjectionEquation {
                projection: self.projection_from_ast(*projection, &mut vars),
                typ: self.from_ast(typ, &mut vars),
            };
            for previous in &equations {
                if previous.projection == equation.projection {
                    let expected = render_ty(&previous.typ);
                    let actual = render_ty(&equation.typ);
                    if self
                        .unify(previous.typ.clone(), equation.typ.clone(), *region)
                        .is_err()
                    {
                        return Err(Error {
                            region: *region,
                            kind: ErrorKind::AssocTypeMismatch {
                                assoc: projection.assoc.name.to_owned(),
                                expected,
                                actual,
                            },
                        });
                    }
                }
            }
            equations.push(equation);
        }
        Ok(equations)
    }

    fn projection_from_ast(
        &mut self,
        projection: alder_ast::ProjectionType<'a>,
        vars: &mut BTreeMap<&'a str, Ty<'a>>,
    ) -> Ty<'a> {
        Ty::Projection(
            projection.trait_ref.trait_,
            projection
                .trait_ref
                .args
                .iter()
                .map(|argument| self.from_ast(argument, vars))
                .collect(),
            projection.assoc,
        )
    }

    fn predicate_from_trait_ref(
        &mut self,
        predicate: alder_ast::TraitRef<'a>,
        vars: &mut BTreeMap<&'a str, Ty<'a>>,
    ) -> Predicate<'a> {
        Predicate {
            trait_: predicate.trait_,
            args: predicate
                .args
                .iter()
                .map(|argument| self.from_ast(argument, vars))
                .collect(),
        }
    }

    fn add_parameter_givens(&mut self, predicates: &[Predicate<'a>], offset: usize) {
        self.givens.extend(
            predicates
                .iter()
                .cloned()
                .enumerate()
                .map(|(index, predicate)| Given {
                    predicate,
                    evidence: Evidence::Param((offset + index) as u16),
                }),
        );
    }

    fn add_superclass_givens(&mut self, predicate: &Predicate<'a>) {
        let mut superclasses = Vec::new();
        self.collect_superclasses(
            predicate,
            &mut Vec::new(),
            &mut BTreeSet::new(),
            &mut superclasses,
        );
        for (path, predicate) in superclasses {
            let evidence = if path.len() == 1 {
                Evidence::Super(path[0])
            } else {
                Evidence::SuperPath(path)
            };
            self.givens.push(Given {
                predicate,
                evidence,
            });
        }
    }

    fn add_parameter_superclass_givens(&mut self, predicates: &[Predicate<'a>], offset: usize) {
        for (parameter_index, predicate) in predicates.iter().enumerate() {
            let mut superclasses = Vec::new();
            self.collect_superclasses(
                predicate,
                &mut Vec::new(),
                &mut BTreeSet::new(),
                &mut superclasses,
            );
            for (path, predicate) in superclasses {
                let param = (offset + parameter_index) as u16;
                let evidence = if path.len() == 1 {
                    Evidence::ParamSuper {
                        param,
                        slot: path[0],
                    }
                } else {
                    Evidence::ParamSuperPath { param, path }
                };
                self.givens.push(Given {
                    predicate,
                    evidence,
                });
            }
        }
    }

    fn collect_superclasses(
        &mut self,
        predicate: &Predicate<'a>,
        path: &mut Vec<u16>,
        active: &mut BTreeSet<TraitId<'a>>,
        output: &mut Vec<(Vec<u16>, Predicate<'a>)>,
    ) {
        if !active.insert(predicate.trait_) {
            return;
        }
        let Some(header) = self.database.trait_(predicate.trait_) else {
            active.remove(&predicate.trait_);
            return;
        };
        let mut vars = header
            .params
            .iter()
            .zip(&predicate.args)
            .map(|(parameter, argument)| (parameter.name.value, argument.clone()))
            .collect::<BTreeMap<_, _>>();
        for (slot, superclass) in header.superclasses.iter().enumerate() {
            let superclass = self.predicate_from_trait_ref(*superclass, &mut vars);
            path.push(slot as u16);
            output.push((path.clone(), superclass.clone()));
            self.collect_superclasses(&superclass, path, active, output);
            path.pop();
        }
        active.remove(&predicate.trait_);
    }

    fn require_impl_superclasses(
        &mut self,
        implementation: &'a alder_ast::ImplDecl<'a>,
        region: Region,
    ) {
        let Some(header) = self.database.trait_(implementation.trait_ref.trait_) else {
            return;
        };
        let mut vars = BTreeMap::new();
        let self_predicate = self.predicate_from_trait_ref(implementation.trait_ref, &mut vars);
        let prerequisites = implementation
            .trait_predicates
            .iter()
            .map(|predicate| self.predicate_from_trait_ref(*predicate, &mut vars))
            .collect::<Vec<_>>();
        let mut givens = prerequisites
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, predicate)| Given {
                predicate,
                evidence: Evidence::Param(index as u16),
            })
            .collect::<Vec<_>>();
        for (parameter_index, prerequisite) in prerequisites.iter().enumerate() {
            let mut superclasses = Vec::new();
            self.collect_superclasses(
                prerequisite,
                &mut Vec::new(),
                &mut BTreeSet::new(),
                &mut superclasses,
            );
            for (path, predicate) in superclasses {
                let param = parameter_index as u16;
                let evidence = if path.len() == 1 {
                    Evidence::ParamSuper {
                        param,
                        slot: path[0],
                    }
                } else {
                    Evidence::ParamSuperPath { param, path }
                };
                givens.push(Given {
                    predicate,
                    evidence,
                });
            }
        }
        let mut superclass_vars = header
            .params
            .iter()
            .zip(&self_predicate.args)
            .map(|(parameter, argument)| (parameter.name.value, argument.clone()))
            .collect::<BTreeMap<_, _>>();
        for (slot, superclass) in header.superclasses.iter().enumerate() {
            let predicate = self.predicate_from_trait_ref(*superclass, &mut superclass_vars);
            self.obligations.push(Obligation {
                use_id: None,
                predicate,
                region,
                action: ObligationAction::ImplSuperclass {
                    implementation: implementation.id,
                    slot: slot as u16,
                },
                givens: givens.clone(),
            });
        }
    }

    #[allow(clippy::wrong_self_convention)]
    fn from_ast(
        &mut self,
        typ: &'a Located<Type<'a>>,
        vars: &mut BTreeMap<&'a str, Ty<'a>>,
    ) -> Ty<'a> {
        match &typ.value {
            Type::Var { name, args } => {
                let base = vars.entry(name).or_insert_with(|| self.fresh()).clone();
                let args = args.iter().map(|arg| self.from_ast(arg, vars)).collect();
                self.apply(base, args)
            }
            Type::Named { reference, args } => {
                let args = args.iter().map(|arg| self.from_ast(arg, vars)).collect();
                self.apply(Ty::Con(*reference), args)
            }
            Type::Partial { constructor, slots } => Ty::Partial(
                *constructor,
                slots
                    .iter()
                    .map(|slot| match slot {
                        TypeSlot::Hole(index) => TySlot::Hole(*index),
                        TypeSlot::Fixed(typ) => TySlot::Fixed(self.from_ast(typ, vars)),
                    })
                    .collect(),
            ),
            Type::Projection(projection) => Ty::Projection(
                projection.trait_ref.trait_,
                projection
                    .trait_ref
                    .args
                    .iter()
                    .map(|arg| self.from_ast(arg, vars))
                    .collect(),
                projection.assoc,
            ),
            Type::Fn { params, ret } => Ty::Fn(
                params
                    .iter()
                    .map(|param| self.from_ast(param, vars))
                    .collect(),
                Box::new(self.from_ast(ret, vars)),
            ),
            Type::Unit => Ty::Unit,
            Type::Tuple(items) => {
                Ty::Tuple(items.iter().map(|item| self.from_ast(item, vars)).collect())
            }
            Type::Record { fields, ext } => Ty::Record(
                fields
                    .iter()
                    .map(|field| (field.name, (field.presence, self.from_ast(field.typ, vars))))
                    .collect(),
                matches!(ext, RowExtension::Open(_)),
            ),
            Type::ErrorRow { .. } => Ty::ErrorRow,
            Type::Alias { target, .. } => match target {
                alder_ast::AliasType::Open(real) | alder_ast::AliasType::Filled(real) => {
                    self.from_ast(real, vars)
                }
            },
        }
    }

    fn unify(&mut self, left: Ty<'a>, right: Ty<'a>, region: Region) -> Result<(), Error> {
        let left = self.normalize_projection_root(left);
        let right = self.normalize_projection_root(right);
        if let Some(result) = self.unify_higher_kinded_pattern(&left, &right, region) {
            return result;
        }
        if let Some(result) = self.unify_higher_kinded_pattern(&right, &left, region) {
            return result;
        }
        match (left, right) {
            (Ty::Any, _) | (_, Ty::Any) | (Ty::ErrorRow, Ty::ErrorRow) => Ok(()),
            (Ty::Var(left), Ty::Var(right)) if left == right => Ok(()),
            (Ty::Var(id), typ) | (typ, Ty::Var(id)) => self.bind(id, typ, region),
            (Ty::Unit, Ty::Unit) => Ok(()),
            (Ty::Con(left), Ty::Con(right)) if left == right => Ok(()),
            (Ty::App(left_head, left_args), Ty::App(right_head, right_args))
                if left_args.len() == right_args.len() =>
            {
                self.unify(*left_head, *right_head, region)?;
                for (left, right) in left_args.into_iter().zip(right_args) {
                    self.unify(left, right, region)?;
                }
                Ok(())
            }
            (Ty::Partial(left, left_slots), Ty::Partial(right, right_slots))
                if left == right && left_slots.len() == right_slots.len() =>
            {
                let actual = Ty::Partial(left, left_slots.clone());
                let expected = Ty::Partial(right, right_slots.clone());
                for (left_slot, right_slot) in left_slots.into_iter().zip(right_slots) {
                    match (left_slot, right_slot) {
                        (TySlot::Hole(left), TySlot::Hole(right)) if left == right => {}
                        (TySlot::Fixed(left), TySlot::Fixed(right)) => {
                            self.unify(left, right, region)?;
                        }
                        _ => return Err(self.mismatch(region, actual, expected)),
                    }
                }
                Ok(())
            }
            (
                Ty::Projection(left_trait, left_args, left_assoc),
                Ty::Projection(right_trait, right_args, right_assoc),
            ) if left_trait == right_trait
                && left_assoc == right_assoc
                && left_args.len() == right_args.len() =>
            {
                for (left, right) in left_args.into_iter().zip(right_args) {
                    self.unify(left, right, region)?;
                }
                Ok(())
            }
            (Ty::Fn(left_args, left_ret), Ty::Fn(right_args, right_ret))
                if left_args.len() == right_args.len() =>
            {
                for (left, right) in left_args.into_iter().zip(right_args) {
                    self.unify(left, right, region)?;
                }
                self.unify(*left_ret, *right_ret, region)
            }
            (Ty::Tuple(left), Ty::Tuple(right)) if left.len() == right.len() => {
                for (left, right) in left.into_iter().zip(right) {
                    self.unify(left, right, region)?;
                }
                Ok(())
            }
            (Ty::Record(left, left_open), Ty::Record(right, right_open)) => {
                self.unify_records(left, left_open, right, right_open, region)
            }
            (left, right) => Err(self.mismatch(region, left, right)),
        }
    }

    fn normalize_projection_root(&mut self, typ: Ty<'a>) -> Ty<'a> {
        let mut current = self.prune(typ);
        let mut seen = Vec::new();
        loop {
            let Ty::Projection(trait_, args, assoc) = current.clone() else {
                return current;
            };
            let args = args
                .into_iter()
                .map(|argument| self.prune(argument))
                .collect::<Vec<_>>();
            current = Ty::Projection(trait_, args.clone(), assoc);
            if seen.contains(&current) {
                return current;
            }
            seen.push(current.clone());

            let equations = self.projection_equations.clone();
            let mut assumed = None;
            for equation in equations {
                let projection = match self.prune(equation.projection) {
                    Ty::Projection(trait_, args, assoc) => Ty::Projection(
                        trait_,
                        args.into_iter()
                            .map(|argument| self.prune(argument))
                            .collect(),
                        assoc,
                    ),
                    other => other,
                };
                if projection == current {
                    assumed = Some(self.prune(equation.typ));
                    break;
                }
            }
            if let Some(typ) = assumed {
                current = typ;
                continue;
            }

            let mut matches = Vec::new();
            for implementation in self.database.instances(trait_) {
                let template = implementation.trait_ref();
                if template.args.len() != args.len() {
                    continue;
                }
                let mut bindings = BTreeMap::new();
                if !template
                    .args
                    .iter()
                    .zip(&args)
                    .all(|(template, goal)| match_type(template, goal, &mut bindings))
                {
                    continue;
                }
                if let Some(binding) = implementation
                    .assoc_bindings()
                    .iter()
                    .find(|binding| binding.assoc == assoc)
                {
                    matches.push((binding.typ, bindings));
                }
            }
            if matches.len() != 1 {
                return current;
            }
            let (typ, mut bindings) = matches.pop().expect("one match");
            current = self.from_ast(typ, &mut bindings);
        }
    }

    fn normalize_type(&mut self, typ: Ty<'a>) -> Ty<'a> {
        match self.normalize_projection_root(typ) {
            Ty::App(head, args) => {
                let head = self.normalize_type(*head);
                let args = args
                    .into_iter()
                    .map(|argument| self.normalize_type(argument))
                    .collect();
                self.apply(head, args)
            }
            Ty::Partial(constructor, slots) => Ty::Partial(
                constructor,
                slots
                    .into_iter()
                    .map(|slot| match slot {
                        TySlot::Hole(index) => TySlot::Hole(index),
                        TySlot::Fixed(typ) => TySlot::Fixed(self.normalize_type(typ)),
                    })
                    .collect(),
            ),
            Ty::Projection(trait_, args, assoc) => {
                let args = args
                    .into_iter()
                    .map(|argument| self.normalize_type(argument))
                    .collect::<Vec<_>>();
                let projection = Ty::Projection(trait_, args, assoc);
                let normalized = self.normalize_projection_root(projection.clone());
                if normalized == projection {
                    projection
                } else {
                    self.normalize_type(normalized)
                }
            }
            Ty::Fn(params, ret) => Ty::Fn(
                params
                    .into_iter()
                    .map(|param| self.normalize_type(param))
                    .collect(),
                Box::new(self.normalize_type(*ret)),
            ),
            Ty::Tuple(items) => Ty::Tuple(
                items
                    .into_iter()
                    .map(|item| self.normalize_type(item))
                    .collect(),
            ),
            Ty::Record(fields, open) => Ty::Record(
                fields
                    .into_iter()
                    .map(|(name, (presence, typ))| (name, (presence, self.normalize_type(typ))))
                    .collect(),
                open,
            ),
            other => other,
        }
    }

    fn unify_higher_kinded_pattern(
        &mut self,
        pattern: &Ty<'a>,
        rigid: &Ty<'a>,
        region: Region,
    ) -> Option<Result<(), Error>> {
        let Ty::App(head, pattern_args) = pattern else {
            return None;
        };
        let Ty::Var(head_var) = self.prune((**head).clone()) else {
            return None;
        };
        let (constructor, rigid_args) = match self.prune(rigid.clone()) {
            Ty::Con(constructor) => (constructor, Vec::new()),
            Ty::App(head, args) => match *head {
                Ty::Con(constructor) => (constructor, args),
                _ => return None,
            },
            _ => return None,
        };
        if pattern_args.is_empty() || rigid_args.len() < pattern_args.len() {
            return None;
        }

        let concrete_arguments = pattern_args
            .iter()
            .all(|argument| !matches!(self.prune(argument.clone()), Ty::Var(_)));
        if concrete_arguments {
            for (pattern_arg, rigid_arg) in pattern_args.iter().zip(&rigid_args) {
                if let Err(error) = self.unify(pattern_arg.clone(), rigid_arg.clone(), region) {
                    return Some(Err(error));
                }
            }
            let slots = rigid_args
                .into_iter()
                .enumerate()
                .map(|(index, typ)| {
                    if index < pattern_args.len() {
                        TySlot::Hole(index as u16)
                    } else {
                        TySlot::Fixed(typ)
                    }
                })
                .collect();
            return Some(self.bind(head_var, Ty::Partial(constructor, slots), region));
        }

        let mut variables = Vec::with_capacity(pattern_args.len());
        let mut seen = BTreeSet::new();
        for argument in pattern_args {
            let Ty::Var(variable) = self.prune(argument.clone()) else {
                return Some(Err(Error {
                    region,
                    kind: ErrorKind::UnsupportedHigherKindedUnification,
                }));
            };
            if variable == head_var || !seen.insert(variable) {
                return Some(Err(Error {
                    region,
                    kind: ErrorKind::UnsupportedHigherKindedUnification,
                }));
            }
            variables.push(variable);
        }

        for (variable, rigid_arg) in variables.iter().zip(&rigid_args) {
            if let Err(error) = self.unify(Ty::Var(*variable), rigid_arg.clone(), region) {
                return Some(Err(error));
            }
        }
        let slots = rigid_args
            .into_iter()
            .enumerate()
            .map(|(index, typ)| {
                if index < variables.len() {
                    TySlot::Hole(index as u16)
                } else {
                    TySlot::Fixed(typ)
                }
            })
            .collect();
        Some(self.bind(head_var, Ty::Partial(constructor, slots), region))
    }

    fn unify_records(
        &mut self,
        left: BTreeMap<&'a str, (FieldPresence, Ty<'a>)>,
        left_open: bool,
        right: BTreeMap<&'a str, (FieldPresence, Ty<'a>)>,
        right_open: bool,
        region: Region,
    ) -> Result<(), Error> {
        for (name, (left_presence, left_type)) in &left {
            match right.get(name) {
                Some((right_presence, right_type)) => {
                    if left_presence != right_presence
                        && !matches!(
                            (left_presence, right_presence),
                            (FieldPresence::Required, FieldPresence::Optional)
                                | (FieldPresence::Optional, FieldPresence::Required)
                        )
                    {
                        return Err(self.mismatch(
                            region,
                            Ty::Record(left, left_open),
                            Ty::Record(right, right_open),
                        ));
                    }
                    self.unify(left_type.clone(), right_type.clone(), region)?;
                }
                None if !right_open && *left_presence == FieldPresence::Required => {
                    return Err(Error {
                        region,
                        kind: ErrorKind::MissingField {
                            field: (*name).to_owned(),
                        },
                    });
                }
                None => {}
            }
        }
        for (name, (presence, _)) in &right {
            if !left.contains_key(name) && !left_open && *presence == FieldPresence::Required {
                return Err(Error {
                    region,
                    kind: ErrorKind::MissingField {
                        field: (*name).to_owned(),
                    },
                });
            }
        }
        Ok(())
    }

    fn bind(&mut self, id: usize, typ: Ty<'a>, region: Region) -> Result<(), Error> {
        if self.occurs(id, &typ) {
            return Err(Error {
                region,
                kind: ErrorKind::InfiniteType,
            });
        }
        self.substitutions[id] = Some(typ);
        Ok(())
    }

    fn prune(&mut self, typ: Ty<'a>) -> Ty<'a> {
        match typ {
            Ty::Var(id) => match self.substitutions[id].clone() {
                Some(bound) => {
                    let pruned = self.prune(bound);
                    self.substitutions[id] = Some(pruned.clone());
                    pruned
                }
                None => Ty::Var(id),
            },
            Ty::App(head, args) => {
                let head = self.prune(*head);
                let args = args.into_iter().map(|arg| self.prune(arg)).collect();
                self.apply(head, args)
            }
            other => other,
        }
    }

    fn apply(&self, head: Ty<'a>, mut arguments: Vec<Ty<'a>>) -> Ty<'a> {
        if arguments.is_empty() {
            return head;
        }
        match head {
            Ty::App(head, mut existing) => {
                existing.append(&mut arguments);
                Ty::App(head, existing)
            }
            Ty::Partial(constructor, slots) => {
                let mut supplied = arguments.into_iter();
                let mut remaining_hole = 0;
                let mut filled = Vec::with_capacity(slots.len());
                let mut complete = true;
                for slot in slots {
                    match slot {
                        TySlot::Fixed(typ) => filled.push(TySlot::Fixed(typ)),
                        TySlot::Hole(_) => match supplied.next() {
                            Some(typ) => filled.push(TySlot::Fixed(typ)),
                            None => {
                                filled.push(TySlot::Hole(remaining_hole));
                                remaining_hole += 1;
                                complete = false;
                            }
                        },
                    }
                }
                let rest = supplied.collect::<Vec<_>>();
                if complete {
                    let base = Ty::App(
                        Box::new(Ty::Con(constructor)),
                        filled
                            .into_iter()
                            .map(|slot| match slot {
                                TySlot::Fixed(typ) => typ,
                                TySlot::Hole(_) => unreachable!("complete partial has no holes"),
                            })
                            .collect(),
                    );
                    if rest.is_empty() {
                        base
                    } else {
                        Ty::App(Box::new(base), rest)
                    }
                } else {
                    debug_assert!(rest.is_empty());
                    Ty::Partial(constructor, filled)
                }
            }
            other => Ty::App(Box::new(other), arguments),
        }
    }

    fn occurs(&mut self, needle: usize, typ: &Ty<'a>) -> bool {
        match self.prune(typ.clone()) {
            Ty::Var(id) => id == needle,
            Ty::Con(_) => false,
            Ty::App(head, args) => {
                self.occurs(needle, &head) || args.iter().any(|arg| self.occurs(needle, arg))
            }
            Ty::Tuple(args) => args.iter().any(|arg| self.occurs(needle, arg)),
            Ty::Partial(_, slots) => slots.iter().any(|slot| match slot {
                TySlot::Hole(_) => false,
                TySlot::Fixed(typ) => self.occurs(needle, typ),
            }),
            Ty::Projection(_, args, _) => args.iter().any(|arg| self.occurs(needle, arg)),
            Ty::Fn(args, ret) => {
                args.iter().any(|arg| self.occurs(needle, arg)) || self.occurs(needle, &ret)
            }
            Ty::Record(fields, _) => fields.values().any(|(_, typ)| self.occurs(needle, typ)),
            Ty::Unit | Ty::ErrorRow | Ty::Any => false,
        }
    }

    fn free_vars(&mut self, typ: &Ty<'a>, result: &mut BTreeSet<usize>) {
        match self.prune(typ.clone()) {
            Ty::Var(id) => {
                result.insert(id);
            }
            Ty::Con(_) => {}
            Ty::App(head, args) => {
                self.free_vars(&head, result);
                for arg in &args {
                    self.free_vars(arg, result);
                }
            }
            Ty::Tuple(args) => {
                for arg in &args {
                    self.free_vars(arg, result);
                }
            }
            Ty::Partial(_, slots) => {
                for slot in &slots {
                    if let TySlot::Fixed(typ) = slot {
                        self.free_vars(typ, result);
                    }
                }
            }
            Ty::Projection(_, args, _) => {
                for arg in &args {
                    self.free_vars(arg, result);
                }
            }
            Ty::Fn(args, ret) => {
                for arg in &args {
                    self.free_vars(arg, result);
                }
                self.free_vars(&ret, result);
            }
            Ty::Record(fields, _) => {
                for (_, typ) in fields.values() {
                    self.free_vars(typ, result);
                }
            }
            Ty::Unit | Ty::ErrorRow | Ty::Any => {}
        }
    }

    fn replace_vars(&mut self, typ: &Ty<'a>, replacements: &BTreeMap<usize, Ty<'a>>) -> Ty<'a> {
        match self.prune(typ.clone()) {
            Ty::Var(id) => replacements.get(&id).cloned().unwrap_or(Ty::Var(id)),
            Ty::Con(name) => Ty::Con(name),
            Ty::App(head, args) => Ty::App(
                Box::new(self.replace_vars(&head, replacements)),
                args.iter()
                    .map(|arg| self.replace_vars(arg, replacements))
                    .collect(),
            ),
            Ty::Partial(name, slots) => Ty::Partial(
                name,
                slots
                    .iter()
                    .map(|slot| match slot {
                        TySlot::Hole(index) => TySlot::Hole(*index),
                        TySlot::Fixed(typ) => TySlot::Fixed(self.replace_vars(typ, replacements)),
                    })
                    .collect(),
            ),
            Ty::Projection(trait_, args, assoc) => Ty::Projection(
                trait_,
                args.iter()
                    .map(|arg| self.replace_vars(arg, replacements))
                    .collect(),
                assoc,
            ),
            Ty::Fn(args, ret) => Ty::Fn(
                args.iter()
                    .map(|arg| self.replace_vars(arg, replacements))
                    .collect(),
                Box::new(self.replace_vars(&ret, replacements)),
            ),
            Ty::Tuple(items) => Ty::Tuple(
                items
                    .iter()
                    .map(|item| self.replace_vars(item, replacements))
                    .collect(),
            ),
            Ty::Record(fields, open) => Ty::Record(
                fields
                    .iter()
                    .map(|(name, (presence, typ))| {
                        (*name, (*presence, self.replace_vars(typ, replacements)))
                    })
                    .collect(),
                open,
            ),
            other => other,
        }
    }

    fn annotation(&mut self, scheme: &Scheme<'a>) -> &'a Annotation<'a> {
        let typ = self.prune(scheme.typ.clone());
        let mut arities = BTreeMap::new();
        self.collect_kind_arities(&typ, &mut arities);
        for predicate in &scheme.predicates {
            for argument in &predicate.args {
                self.collect_kind_arities(argument, &mut arities);
            }
        }
        for equation in &scheme.projection_eqs {
            self.collect_kind_arities(&equation.projection, &mut arities);
            self.collect_kind_arities(&equation.typ, &mut arities);
        }
        let mut names = BTreeMap::new();
        let typ = self.to_ast(&typ, &mut names);
        let trait_predicates = self
            .bump
            .alloc_slice_fill_iter(scheme.predicates.iter().map(|predicate| {
                alder_ast::TraitRef {
                    trait_: predicate.trait_,
                    args: self.bump.alloc_slice_fill_iter(
                        predicate
                            .args
                            .iter()
                            .map(|argument| self.to_ast(argument, &mut names)),
                    ),
                }
            }));
        let mut projection_equalities = Vec::with_capacity(scheme.projection_eqs.len());
        for equation in &scheme.projection_eqs {
            let projection_type = self.to_ast(&equation.projection, &mut names);
            let Type::Projection(projection) = projection_type.value else {
                unreachable!("scheme projection equations have projection left sides")
            };
            projection_equalities.push(alder_ast::ProjectionEquality {
                projection,
                typ: self.to_ast(&equation.typ, &mut names),
                region: Region::zero(),
            });
        }
        let mut params = names.into_iter().collect::<Vec<_>>();
        params.sort_by_key(|(_, name)| generated_type_name_rank(name));
        self.bump.alloc(Annotation {
            params: self
                .bump
                .alloc_slice_fill_iter(params.into_iter().map(|(id, name)| alder_ast::TypeParam {
                    name: Located::at(Region::zero(), name),
                    kind: self.kind_from_arity(arities.get(&id).copied().unwrap_or(0)),
                })),
            trait_predicates,
            projection_equalities: self.bump.alloc_slice_copy(&projection_equalities),
            typ,
        })
    }

    fn collect_kind_arities(&mut self, typ: &Ty<'a>, arities: &mut BTreeMap<usize, usize>) {
        match self.prune(typ.clone()) {
            Ty::Var(id) => {
                arities.entry(id).or_insert(0);
            }
            Ty::Con(_) | Ty::Unit | Ty::ErrorRow | Ty::Any => {}
            Ty::App(head, args) => {
                match self.prune(*head) {
                    Ty::Var(id) => {
                        arities
                            .entry(id)
                            .and_modify(|arity| *arity = (*arity).max(args.len()))
                            .or_insert(args.len());
                    }
                    other => self.collect_kind_arities(&other, arities),
                }
                for arg in &args {
                    self.collect_kind_arities(arg, arities);
                }
            }
            Ty::Partial(_, slots) => {
                for slot in &slots {
                    if let TySlot::Fixed(typ) = slot {
                        self.collect_kind_arities(typ, arities);
                    }
                }
            }
            Ty::Projection(_, args, _) | Ty::Tuple(args) => {
                for arg in &args {
                    self.collect_kind_arities(arg, arities);
                }
            }
            Ty::Fn(args, ret) => {
                for arg in &args {
                    self.collect_kind_arities(arg, arities);
                }
                self.collect_kind_arities(&ret, arities);
            }
            Ty::Record(fields, _) => {
                for (_, typ) in fields.values() {
                    self.collect_kind_arities(typ, arities);
                }
            }
        }
    }

    fn kind_from_arity(&self, arity: usize) -> alder_ast::Kind<'a> {
        let mut kind = alder_ast::Kind::Type;
        for _ in 0..arity {
            kind = alder_ast::Kind::Arrow {
                param: self.bump.alloc(alder_ast::Kind::Type),
                result: self.bump.alloc(kind),
            };
        }
        kind
    }

    #[allow(clippy::wrong_self_convention)]
    fn to_ast(
        &mut self,
        typ: &Ty<'a>,
        names: &mut BTreeMap<usize, &'a str>,
    ) -> &'a Located<Type<'a>> {
        let typ = match self.prune(typ.clone()) {
            Ty::Var(id) => {
                let name = self.type_var_name(id, names);
                Type::Var { name, args: &[] }
            }
            Ty::Con(reference) => Type::Named {
                reference,
                args: &[],
            },
            Ty::App(head, args) => match self.prune(*head) {
                Ty::Con(reference) => Type::Named {
                    reference,
                    args: self
                        .bump
                        .alloc_slice_fill_iter(args.iter().map(|arg| self.to_ast(arg, names))),
                },
                Ty::Var(id) => {
                    let name = self.type_var_name(id, names);
                    Type::Var {
                        name,
                        args: self
                            .bump
                            .alloc_slice_fill_iter(args.iter().map(|arg| self.to_ast(arg, names))),
                    }
                }
                other => panic!("unsupported public type application head: {other:?}"),
            },
            Ty::Partial(constructor, slots) => Type::Partial {
                constructor,
                slots: self
                    .bump
                    .alloc_slice_fill_iter(slots.iter().map(|slot| match slot {
                        TySlot::Hole(index) => TypeSlot::Hole(*index),
                        TySlot::Fixed(typ) => TypeSlot::Fixed(self.to_ast(typ, names)),
                    })),
            },
            Ty::Projection(trait_, args, assoc) => Type::Projection(alder_ast::ProjectionType {
                trait_ref: alder_ast::TraitRef {
                    trait_,
                    args: self
                        .bump
                        .alloc_slice_fill_iter(args.iter().map(|arg| self.to_ast(arg, names))),
                },
                assoc,
            }),
            Ty::Fn(params, ret) => Type::Fn {
                params: self
                    .bump
                    .alloc_slice_fill_iter(params.iter().map(|param| self.to_ast(param, names))),
                ret: self.to_ast(&ret, names),
            },
            Ty::Unit | Ty::Any => Type::Unit,
            Ty::Tuple(items) => Type::Tuple(
                self.bump
                    .alloc_slice_fill_iter(items.iter().map(|item| self.to_ast(item, names))),
            ),
            Ty::Record(fields, open) => Type::Record {
                fields: self
                    .bump
                    .alloc_slice_fill_iter(fields.iter().enumerate().map(
                        |(index, (name, (presence, typ)))| alder_ast::RecordTypeField {
                            index: index as u16,
                            name,
                            presence: *presence,
                            typ: self.to_ast(typ, names),
                        },
                    )),
                ext: if open {
                    RowExtension::Open("r")
                } else {
                    RowExtension::Closed
                },
            },
            Ty::ErrorRow => Type::ErrorRow {
                tags: &[],
                ext: RowExtension::Open("e"),
            },
        };
        self.bump.alloc(Located::at_zero(typ))
    }

    fn type_var_name(&self, id: usize, names: &mut BTreeMap<usize, &'a str>) -> &'a str {
        let next = names.len();
        names.entry(id).or_insert_with(|| {
            let generated = if next < 26 {
                ((b'a' + next as u8) as char).to_string()
            } else {
                format!("t{next}")
            };
            self.bump.alloc_str(&generated)
        })
    }

    fn named(&self, name: &'a str, args: Vec<Ty<'a>>) -> Ty<'a> {
        self.apply(
            Ty::Con(QualifiedName {
                module: ModuleId {
                    package: PackageId::Builtin,
                    path: &[],
                },
                name,
            }),
            args,
        )
    }

    fn mismatch(&mut self, region: Region, actual: Ty<'a>, expected: Ty<'a>) -> Error {
        Error {
            region,
            kind: ErrorKind::Mismatch {
                actual: self.render(actual),
                expected: self.render(expected),
            },
        }
    }

    fn render(&mut self, typ: Ty<'a>) -> String {
        match self.prune(typ) {
            Ty::Var(_) => "a".to_owned(),
            Ty::Con(name) => name.name.to_owned(),
            Ty::App(head, args) => format!(
                "{}[{}]",
                self.render(*head),
                args.into_iter()
                    .map(|arg| self.render(arg))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ty::Fn(args, ret) => format!(
                "fn({}) -> {}",
                args.into_iter()
                    .map(|arg| self.render(arg))
                    .collect::<Vec<_>>()
                    .join(", "),
                self.render(*ret)
            ),
            Ty::Unit => "()".to_owned(),
            Ty::Tuple(items) => format!(
                "({})",
                items
                    .into_iter()
                    .map(|item| self.render(item))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ty::Record(fields, _) => format!(
                "{{ {} }}",
                fields
                    .into_iter()
                    .map(|(name, (presence, typ))| format!(
                        "{}{}: {}",
                        name,
                        if presence == FieldPresence::Optional {
                            "?"
                        } else {
                            ""
                        },
                        self.render(typ)
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ty::Partial(reference, slots) => format!(
                "{}[{}]",
                reference.name,
                slots
                    .into_iter()
                    .map(|slot| match slot {
                        TySlot::Hole(_) => "_".to_owned(),
                        TySlot::Fixed(typ) => self.render(typ),
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ty::Projection(trait_, args, assoc) => format!(
                "{}[{}]::{}",
                trait_.0.name,
                args.into_iter()
                    .map(|arg| self.render(arg))
                    .collect::<Vec<_>>()
                    .join(", "),
                assoc.name
            ),
            Ty::ErrorRow => "[:_ | e]".to_owned(),
            Ty::Any => "_".to_owned(),
        }
    }
}

fn block_contains_return(block: &Located<Block<'_>>) -> bool {
    block
        .value
        .statements
        .iter()
        .any(|statement| match &statement.value {
            Stmt::Return(_) => true,
            Stmt::For { body, .. } | Stmt::While { body, .. } => block_contains_return(body),
            Stmt::Let(_)
            | Stmt::Use { .. }
            | Stmt::Assign { .. }
            | Stmt::Break(_)
            | Stmt::Continue
            | Stmt::Assert(_)
            | Stmt::Expr(_) => false,
        })
}

fn is_value_item(item: &ItemKind<'_>) -> bool {
    matches!(
        item,
        ItemKind::Fn(_)
            | ItemKind::Let(_)
            | ItemKind::Component(_)
            | ItemKind::Extern(alder_ast::ExternDecl::Fn { .. })
    )
}

fn generated_type_name_rank(name: &str) -> usize {
    match name.as_bytes() {
        [letter @ b'a'..=b'z'] => usize::from(*letter - b'a'),
        _ => name
            .strip_prefix('t')
            .and_then(|index| index.parse().ok())
            .unwrap_or(usize::MAX),
    }
}
