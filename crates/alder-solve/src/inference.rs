use std::collections::{BTreeMap, BTreeSet};

use alder_ast::{
    Annotation, BinOp, BindingName, Block, Child, ChildBlock, ChildItem, Expr, FieldPresence,
    ImplId, ItemKind, MethodId, Module, ModuleId, PackageId, Pattern, QualifiedName, RecordField,
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
    Record(
        BTreeMap<&'a str, (FieldPresence, Ty<'a>)>,
        Option<Box<Ty<'a>>>,
    ),
    RecordRow(Box<Ty<'a>>),
    ErrorRow {
        tags: BTreeMap<&'a str, Vec<Ty<'a>>>,
        tail: Option<Box<Ty<'a>>>,
    },
    Any,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VariableKind {
    Unknown,
    Type,
    RecordRow,
    ErrorRow,
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
    let variable_names = result.variable_names;
    let generalized_variables = result.generalized_variables;
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
            ResolutionStep {
                variable_names: &variable_names,
                generalized_variables: &generalized_variables,
                required_by: None,
            },
        ) {
            Ok(evidence) => match obligation.action {
                ObligationAction::ContractCheck => {}
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
            optional_accesses: result.optional_accesses,
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
        let mut next_var = vars.len();
        let self_predicate =
            predicate_from_ast_ref(implementation.trait_ref, &mut vars, &mut next_var);
        let mut givens = implementation
            .trait_predicates
            .iter()
            .enumerate()
            .map(|(index, predicate)| Given {
                predicate: predicate_from_ast_ref(*predicate, &mut vars, &mut next_var),
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
                            &mut next_var,
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
                            &mut next_var,
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
    next_var: &mut usize,
    resolved: &mut BTreeMap<DerivedFieldKey<'a>, Evidence<'a>>,
    errors: &mut Vec<SolveError<'a>>,
) where
    'a: 'field,
{
    for (field, typ) in fields.enumerate() {
        let predicate = Predicate {
            trait_: self_predicate.trait_,
            args: vec![ty_from_ast(typ, vars, next_var)],
        };
        let mut stack = Vec::new();
        let variable_names = vars
            .iter()
            .filter_map(|(name, typ)| match typ {
                Ty::Var(id) => Some((*id, *name)),
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        let generalized_variables = variable_names.keys().copied().collect();
        match resolve_predicate(
            bump,
            database,
            &predicate,
            givens,
            typ.region,
            &mut stack,
            ResolutionStep {
                variable_names: &variable_names,
                generalized_variables: &generalized_variables,
                required_by: None,
            },
        ) {
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
    next_var: &mut usize,
) -> Predicate<'a> {
    Predicate {
        trait_: predicate.trait_,
        args: predicate
            .args
            .iter()
            .map(|argument| ty_from_ast(argument, vars, next_var))
            .collect(),
    }
}

fn ty_from_ast<'a>(
    typ: &Located<Type<'a>>,
    vars: &mut BTreeMap<&'a str, Ty<'a>>,
    next_var: &mut usize,
) -> Ty<'a> {
    let apply = |head, args: Vec<_>| {
        if args.is_empty() {
            head
        } else {
            Ty::App(Box::new(head), args)
        }
    };
    match &typ.value {
        Type::Var { name, args } => {
            let next = *next_var;
            *next_var += usize::from(!vars.contains_key(name));
            let head = vars.entry(name).or_insert(Ty::Var(next)).clone();
            apply(
                head,
                args.iter()
                    .map(|argument| ty_from_ast(argument, vars, next_var))
                    .collect(),
            )
        }
        Type::Named { reference, args } => {
            let mut args = args
                .iter()
                .map(|argument| ty_from_ast(argument, vars, next_var))
                .collect::<Vec<_>>();
            if reference.name == "Result" && args.len() == 1 {
                let id = *next_var;
                *next_var += 1;
                args.push(Ty::ErrorRow {
                    tags: BTreeMap::new(),
                    tail: Some(Box::new(Ty::Var(id))),
                });
            }
            apply(Ty::Con(*reference), args)
        }
        Type::Partial { constructor, slots } => Ty::Partial(
            *constructor,
            slots
                .iter()
                .map(|slot| match slot {
                    TypeSlot::Hole(index) => TySlot::Hole(*index),
                    TypeSlot::Fixed(typ) => TySlot::Fixed(ty_from_ast(typ, vars, next_var)),
                })
                .collect(),
        ),
        Type::Projection(projection) => Ty::Projection(
            projection.trait_ref.trait_,
            projection
                .trait_ref
                .args
                .iter()
                .map(|argument| ty_from_ast(argument, vars, next_var))
                .collect(),
            projection.assoc,
        ),
        Type::Fn { params, ret } => Ty::Fn(
            params
                .iter()
                .map(|param| ty_from_ast(param, vars, next_var))
                .collect(),
            Box::new(ty_from_ast(ret, vars, next_var)),
        ),
        Type::Unit => Ty::Unit,
        Type::Tuple(items) => Ty::Tuple(
            items
                .iter()
                .map(|item| ty_from_ast(item, vars, next_var))
                .collect(),
        ),
        Type::Record { fields, ext } => Ty::Record(
            fields
                .iter()
                .map(|field| {
                    (
                        field.name,
                        (field.presence, ty_from_ast(field.typ, vars, next_var)),
                    )
                })
                .collect(),
            match ext {
                RowExtension::Closed => None,
                RowExtension::Open(name) => {
                    let next = *next_var;
                    *next_var += usize::from(!vars.contains_key(name));
                    Some(Box::new(vars.entry(name).or_insert(Ty::Var(next)).clone()))
                }
            },
        ),
        Type::ErrorRow { tags, ext } => Ty::ErrorRow {
            tags: tags
                .iter()
                .map(|tag| {
                    (
                        tag.name,
                        tag.args
                            .iter()
                            .map(|argument| ty_from_ast(argument, vars, next_var))
                            .collect(),
                    )
                })
                .collect(),
            tail: match ext {
                RowExtension::Closed => None,
                RowExtension::Open(name) => {
                    let next = *next_var;
                    *next_var += usize::from(!vars.contains_key(name));
                    Some(Box::new(vars.entry(name).or_insert(Ty::Var(next)).clone()))
                }
            },
        },
        Type::Alias { target, .. } => match target {
            alder_ast::AliasType::Open(real) | alder_ast::AliasType::Filled(real) => {
                ty_from_ast(real, vars, next_var)
            }
        },
    }
}

#[derive(Clone, Copy)]
struct ResolutionStep<'a, 'names> {
    variable_names: &'names BTreeMap<usize, &'a str>,
    generalized_variables: &'names BTreeSet<usize>,
    required_by: Option<ImplId<'a>>,
}

fn resolve_predicate<'a>(
    bump: &'a Bump,
    database: &TraitDatabase<'a>,
    predicate: &Predicate<'a>,
    givens: &[Given<'a>],
    origin: Region,
    stack: &mut Vec<crate::ObligationFrame<'a>>,
    step: ResolutionStep<'a, '_>,
) -> Result<Evidence<'a>, SolveTraitError<'a>> {
    let subject = predicate.args.first().cloned().unwrap_or(Ty::Unit);
    let rendered = render_ty(&subject, step.variable_names);
    if let Some(given) = givens.iter().find(|given| {
        given.predicate.trait_ == predicate.trait_ && given.predicate.args == predicate.args
    }) {
        return Ok(given.evidence.clone());
    }
    if let Some(cycle_start) = stack
        .iter()
        .position(|frame| frame.trait_ == predicate.trait_ && frame.subject == rendered)
    {
        let current = crate::ObligationFrame {
            trait_: predicate.trait_,
            subject: bump.alloc_str(&rendered),
            required_by: step.required_by,
        };
        let mut cycle = stack[cycle_start..].to_vec();
        cycle.push(current);
        let chain = bump.alloc_slice_copy(&cycle);
        return Err(SolveTraitError::InstanceCycle {
            trait_: predicate.trait_,
            subject: bump.alloc_str(&rendered),
            origin,
            chain,
        });
    }
    stack.push(crate::ObligationFrame {
        trait_: predicate.trait_,
        subject: bump.alloc_str(&rendered),
        required_by: step.required_by,
    });
    match resolve_structural_eq(bump, database, predicate, givens, origin, stack, step) {
        Ok(Some(evidence)) => {
            stack.pop();
            return Ok(evidence);
        }
        Ok(None) => {}
        Err(error) => {
            stack.pop();
            return Err(error);
        }
    }
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
            match resolve_predicate(
                bump,
                database,
                &prerequisite,
                givens,
                origin,
                stack,
                ResolutionStep {
                    required_by: Some(implementation.id()),
                    ..step
                },
            ) {
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
    let chain = bump.alloc_slice_copy(stack);
    stack.pop();
    match successes.len() {
        1 => Ok(successes.pop().expect("one success").1),
        count if count > 1 => Err(SolveTraitError::AmbiguousInstance {
            trait_: predicate.trait_,
            subject: bump.alloc_str(&rendered),
            origin,
            details: bump.alloc(crate::AmbiguousInstanceDetails {
                candidates: bump
                    .alloc_slice_fill_iter(successes.into_iter().map(|(impl_id, _)| impl_id)),
                chain,
            }),
        }),
        _ if nested_error.is_some() => Err(nested_error.expect("checked above")),
        _ if has_variable_head(&subject) => {
            let mut variables = BTreeSet::new();
            collect_variables(&subject, &mut variables);
            if variables
                .iter()
                .all(|variable| step.generalized_variables.contains(variable))
            {
                Err(SolveTraitError::UnsatisfiedBound {
                    trait_: predicate.trait_,
                    subject: bump.alloc_str(&rendered),
                    origin,
                    chain,
                })
            } else {
                Err(SolveTraitError::AmbiguousTypeVariable {
                    trait_: predicate.trait_,
                    subject: bump.alloc_str(&rendered),
                    origin,
                    chain,
                })
            }
        }
        _ => Err(SolveTraitError::MissingInstance {
            trait_: predicate.trait_,
            subject: bump.alloc_str(&rendered),
            origin,
            chain,
        }),
    }
}

fn resolve_structural_eq<'a>(
    bump: &'a Bump,
    database: &TraitDatabase<'a>,
    predicate: &Predicate<'a>,
    givens: &[Given<'a>],
    origin: Region,
    stack: &mut Vec<crate::ObligationFrame<'a>>,
    step: ResolutionStep<'a, '_>,
) -> Result<Option<Evidence<'a>>, SolveTraitError<'a>> {
    if predicate.trait_ != builtin_trait_id("Eq") {
        return Ok(None);
    }
    let Some(subject) = predicate.args.first() else {
        return Ok(None);
    };
    let structural = match subject {
        Ty::Tuple(items) => Some((StructuralEqShape::Tuple, items.clone())),
        Ty::Record(fields, None) => Some((
            StructuralEqShape::Record(fields.keys().copied().collect()),
            fields.values().map(|(_, typ)| typ.clone()).collect(),
        )),
        Ty::ErrorRow { tags, tail: None } => Some((
            StructuralEqShape::ErrorRow(
                tags.iter()
                    .map(|(name, payloads)| (*name, payloads.len()))
                    .collect(),
            ),
            tags.values().flatten().cloned().collect(),
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
                step,
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
        ("Json", Some("Number")) => Intrinsic::JsonNumber,
        ("Json", Some("String")) => Intrinsic::JsonString,
        ("Json", Some("Bool")) => Intrinsic::JsonBool,
        ("Json", Some("BigInt")) => Intrinsic::JsonBigInt,
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
        ("Iterator", Some("ArrayIterator")) => Intrinsic::IteratorArray,
        ("Show", None) if matches!(subject, Ty::Unit) => Intrinsic::ShowKernel,
        ("Hash", None) if matches!(subject, Ty::Unit) => Intrinsic::HashKernel,
        ("Json", None) if matches!(subject, Ty::Unit) => Intrinsic::JsonUnit,
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

fn has_variable_head(typ: &Ty<'_>) -> bool {
    match typ {
        Ty::Var(_) => true,
        Ty::App(head, _) => has_variable_head(head),
        _ => false,
    }
}

fn collect_variables(typ: &Ty<'_>, variables: &mut BTreeSet<usize>) {
    match typ {
        Ty::Var(variable) => {
            variables.insert(*variable);
        }
        Ty::App(head, arguments) => {
            collect_variables(head, variables);
            for argument in arguments {
                collect_variables(argument, variables);
            }
        }
        Ty::Partial(_, slots) => {
            for slot in slots {
                if let TySlot::Fixed(typ) = slot {
                    collect_variables(typ, variables);
                }
            }
        }
        Ty::Projection(_, arguments, _) | Ty::Tuple(arguments) => {
            for argument in arguments {
                collect_variables(argument, variables);
            }
        }
        Ty::Fn(arguments, result) => {
            for argument in arguments {
                collect_variables(argument, variables);
            }
            collect_variables(result, variables);
        }
        Ty::RecordRow(row) => collect_variables(row, variables),
        Ty::Record(fields, tail) => {
            for (_, typ) in fields.values() {
                collect_variables(typ, variables);
            }
            if let Some(tail) = tail {
                collect_variables(tail, variables);
            }
        }
        Ty::ErrorRow { tags, tail } => {
            for payloads in tags.values() {
                for payload in payloads {
                    collect_variables(payload, variables);
                }
            }
            if let Some(tail) = tail {
                collect_variables(tail, variables);
            }
        }
        Ty::Con(_) | Ty::Unit | Ty::Any => {}
    }
}

fn render_ty(typ: &Ty<'_>, variable_names: &BTreeMap<usize, &str>) -> String {
    match typ {
        Ty::Var(id) => variable_names.get(id).copied().unwrap_or("a").to_owned(),
        Ty::Con(name) => name.name.to_owned(),
        Ty::App(head, args) => format!(
            "{}[{}]",
            render_ty(head, variable_names),
            args.iter()
                .map(|arg| render_ty(arg, variable_names))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Ty::Partial(name, slots) => format!(
            "{}[{}]",
            name.name,
            slots
                .iter()
                .map(|slot| match slot {
                    TySlot::Hole(_) => "_".to_owned(),
                    TySlot::Fixed(typ) => render_ty(typ, variable_names),
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Ty::Projection(trait_, args, assoc) => format!(
            "{}[{}]::{}",
            trait_.0.name,
            args.iter()
                .map(|arg| render_ty(arg, variable_names))
                .collect::<Vec<_>>()
                .join(", "),
            assoc.name
        ),
        Ty::Fn(params, ret) => format!(
            "fn({}) {}",
            params
                .iter()
                .map(|param| render_ty(param, variable_names))
                .collect::<Vec<_>>()
                .join(", "),
            render_ty(ret, variable_names)
        ),
        Ty::Unit => "()".to_owned(),
        Ty::Tuple(items) => format!(
            "({})",
            items
                .iter()
                .map(|item| render_ty(item, variable_names))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Ty::Record(_, _) | Ty::RecordRow(_) => "{ .. }".to_owned(),
        Ty::ErrorRow { tags, tail } => {
            render_error_row(tags, tail.as_deref(), |typ| render_ty(typ, variable_names))
        }
        Ty::Any => "_".to_owned(),
    }
}

fn render_error_row<'a>(
    tags: &BTreeMap<&str, Vec<Ty<'a>>>,
    tail: Option<&Ty<'a>>,
    mut render_type: impl FnMut(&Ty<'a>) -> String,
) -> String {
    let mut parts = tags
        .iter()
        .map(|(name, payloads)| {
            if payloads.is_empty() {
                format!(":{name}")
            } else {
                format!(
                    ":{name}({})",
                    payloads
                        .iter()
                        .map(&mut render_type)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        })
        .collect::<Vec<_>>();
    if tail.is_some() {
        parts.push("_".to_owned());
    }
    format!("[{}]", parts.join(" | "))
}

#[derive(Clone, Debug)]
struct Scheme<'a> {
    record_overlays: Vec<RecordOverlay<'a>>,
    quantified: Vec<usize>,
    predicates: Vec<Predicate<'a>>,
    projection_eqs: Vec<ProjectionEquation<'a>>,
    error_row_inclusions: Vec<ErrorRowInclusion<'a>>,
    typ: Ty<'a>,
}

#[derive(Clone, Debug)]
struct ErrorRowInclusion<'a> {
    exact_target: bool,
    source: Ty<'a>,
    target: Ty<'a>,
    region: Region,
}

#[derive(Clone, Debug)]
struct RecordOverlay<'a> {
    operands: Vec<Ty<'a>>,
    result: Ty<'a>,
    region: Region,
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
    ContractCheck,
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
    optional_accesses: BTreeSet<Region>,
    annotations: Annotations<'a>,
    bindings: BTreeMap<QualifiedName<'a>, BindingEvidence<'a>>,
    obligations: Vec<Obligation<'a>>,
    calls: Vec<CallSite<'a>>,
    variable_names: BTreeMap<usize, &'a str>,
    generalized_variables: BTreeSet<usize>,
}

#[derive(Clone, Copy, Debug)]
struct CallSite<'a> {
    use_id: UseId,
    callee_use: Option<UseId>,
    target: Option<DirectTarget<'a>>,
}

#[derive(Clone)]
struct MatchSite<'a> {
    scrutinee: Ty<'a>,
    arms: &'a [alder_ast::MatchArm<'a>],
    region: Region,
}

#[derive(Default)]
struct ErrorCoverage<'a> {
    all: bool,
    ok: bool,
    all_errors: bool,
    tags: BTreeSet<&'a str>,
}

fn collect_error_coverage<'a>(pattern: &'a Located<Pattern<'a>>, coverage: &mut ErrorCoverage<'a>) {
    match &pattern.value {
        Pattern::Anything | Pattern::Bind(_) => coverage.all = true,
        Pattern::Alias { pattern, .. } => collect_error_coverage(pattern, coverage),
        Pattern::Tag { name, .. } => {
            coverage.tags.insert(name.value);
        }
        Pattern::Constructor { constructor, args }
            if constructor.name.enum_.module.package == PackageId::Builtin
                && constructor.name.enum_.name == "Result" =>
        {
            match constructor.name.variant {
                "Ok" => coverage.ok = true,
                "Err" => {
                    if let Some(error) = args.first() {
                        match &error.value {
                            Pattern::Anything | Pattern::Bind(_) => coverage.all_errors = true,
                            _ => collect_error_coverage(error, coverage),
                        }
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn is_result_err_expr(expression: &Located<Expr<'_>>) -> bool {
    match expression.value {
        Expr::Constructor(constructor) => {
            constructor.name.enum_.module.package == PackageId::Builtin
                && constructor.name.enum_.name == "Result"
                && constructor.name.variant == "Err"
        }
        Expr::Var {
            reference: ValueRef::Foreign { reference, .. } | ValueRef::TopLevel(reference),
            ..
        } => reference.name == "err" && reference.module.path.last() == Some(&"Result"),
        _ => false,
    }
}

struct CallInput<'a> {
    region: Region,
    use_id: UseId,
    function: &'a Located<Expr<'a>>,
    arguments: &'a [&'a Located<Expr<'a>>],
    leading: Option<(Ty<'a>, Region)>,
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
    is_async: bool,
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

/// A declared universal contract must survive solving as distinct, unbound
/// variables, independent of its enclosing monomorphic environment. Checks are
/// deferred until the module is solved so recursive peers cannot specialize a
/// signature after its body has already been visited.
struct GenericContract<'a> {
    variables: BTreeMap<&'a str, Ty<'a>>,
    outer_free: BTreeSet<usize>,
    region: Region,
}

struct Infer<'a, 'db> {
    record_overlays: Vec<RecordOverlay<'a>>,
    bump: &'a Bump,
    database: &'db TraitDatabase<'a>,
    substitutions: Vec<Option<Ty<'a>>>,
    variable_kinds: Vec<VariableKind>,
    obligations: Vec<Obligation<'a>>,
    givens: Vec<Given<'a>>,
    projection_equations: Vec<ProjectionEquation<'a>>,
    error_row_inclusions: Vec<ErrorRowInclusion<'a>>,
    calls: Vec<CallSite<'a>>,
    inferred_error_rows: Vec<Ty<'a>>,
    error_row_joins: Vec<(Ty<'a>, Ty<'a>, Ty<'a>)>,
    match_sites: Vec<MatchSite<'a>>,
    tag_sites: Vec<Region>,
    legal_tag_sites: Vec<Region>,
    requirement_seeds: BTreeMap<UseId, RequirementSeed<'a>>,
    variable_names: BTreeMap<usize, &'a str>,
    generalized_variables: BTreeSet<usize>,
    generic_contracts: Vec<GenericContract<'a>>,
    annotation_scope: BTreeMap<&'a str, Ty<'a>>,
    active_scc: BTreeSet<QualifiedName<'a>>,
    loop_results: Vec<Ty<'a>>,
    reachable: bool,
    value_checks: Vec<(Ty<'a>, Ty<'a>, Region)>,
    field_accesses: Vec<(Ty<'a>, &'a str, Region)>,
}

/// Infer core annotations only, without validating coherence or resolving trait
/// obligations. This lower-level helper is not a complete compilation check;
/// use [`solve`] for the checked contract and dictionary evidence.
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

/// Validate coherence, infer the module, and resolve its trait obligations into
/// the checked schemes and dictionary evidence consumed by code generation.
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
            variable_kinds: Vec::new(),
            obligations: Vec::new(),
            givens: Vec::new(),
            projection_equations: Vec::new(),
            record_overlays: Vec::new(),
            error_row_inclusions: Vec::new(),
            calls: Vec::new(),
            inferred_error_rows: Vec::new(),
            error_row_joins: Vec::new(),
            match_sites: Vec::new(),
            tag_sites: Vec::new(),
            legal_tag_sites: Vec::new(),
            requirement_seeds: requirement_seeds
                .iter()
                .map(|seed| (seed.use_id, *seed))
                .collect(),
            variable_names: BTreeMap::new(),
            generalized_variables: BTreeSet::new(),
            generic_contracts: Vec::new(),
            annotation_scope: BTreeMap::new(),
            active_scc: BTreeSet::new(),
            loop_results: Vec::new(),
            reachable: true,
            value_checks: Vec::new(),
            field_accesses: Vec::new(),
        }
    }

    fn fresh(&mut self) -> Ty<'a> {
        self.fresh_with_kind(VariableKind::Unknown)
    }

    fn fresh_with_kind(&mut self, kind: VariableKind) -> Ty<'a> {
        let id = self.substitutions.len();
        self.substitutions.push(None);
        self.variable_kinds.push(kind);
        Ty::Var(id)
    }

    fn fresh_error_row(&mut self) -> Ty<'a> {
        Ty::ErrorRow {
            tags: BTreeMap::new(),
            tail: Some(Box::new(self.fresh_with_kind(VariableKind::ErrorRow))),
        }
    }

    fn open_record(&mut self, fields: BTreeMap<&'a str, (FieldPresence, Ty<'a>)>) -> Ty<'a> {
        Ty::Record(
            fields,
            Some(Box::new(self.fresh_with_kind(VariableKind::RecordRow))),
        )
    }

    fn infer_module(&mut self, module: &'a Module<'a>) -> Result<InferenceResult<'a>, Error> {
        let mut env = Env::default();
        let assigned: BTreeSet<_> = module.assigned_bindings.iter().copied().collect();
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
            self.active_scc = group.members.iter().copied().collect();
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
            let mut outer_free =
                self.environment_free_vars(&env, &group.members.iter().copied().collect());
            // Restricted bindings in this SCC are shared state too. Excluding
            // the entire recursive group must not let a sibling function
            // quantify variables reachable through one of these bindings.
            for member in group.members {
                if assigned.contains(member) || !generalizable_item(&value_items[member].value.kind)
                {
                    outer_free.extend(self.scheme_free_vars(&env.globals[member]));
                }
            }
            for member in group.members {
                let generalizable = !assigned.contains(member)
                    && generalizable_item(&value_items[member].value.kind);
                self.generalize_global(&mut env, *member, &outer_free, generalizable);
            }
        }

        self.active_scc.clear();
        for item in module.items {
            if !is_value_item(&item.value.kind) {
                self.infer_item(&mut env, &item.value.kind, item.region)?;
            }
        }

        loop {
            let before = (
                self.substitutions
                    .iter()
                    .filter(|typ| typ.is_some())
                    .count(),
                self.record_overlays.len(),
                self.error_row_inclusions.len(),
                self.error_row_joins.len(),
            );
            self.solve_record_overlays()?;
            self.check_overlay_contracts()?;
            self.solve_error_row_inclusions(&env)?;
            let after = (
                self.substitutions
                    .iter()
                    .filter(|typ| typ.is_some())
                    .count(),
                self.record_overlays.len(),
                self.error_row_inclusions.len(),
                self.error_row_joins.len(),
            );
            if before == after {
                break;
            }
        }
        // Contract rigidity is final: no subsequent pass may introduce type
        // equalities or unsolved error unions after these promises are checked.
        self.check_generic_contracts()?;
        for (actual, expected, region) in std::mem::take(&mut self.value_checks) {
            self.check_field_presence(actual, expected, region)?;
        }
        self.check_error_matches()?;
        self.check_error_tag_placement()?;

        let mut annotations = BTreeMap::new();
        let mut bindings = BTreeMap::new();
        for (name, scheme) in env.globals {
            if matches!(
                value_items[&name].value.visibility,
                alder_ast::Visibility::Public(_)
            ) && !self.scheme_free_vars(&scheme).is_empty()
            {
                return Err(Error {
                    region: value_items[&name].region,
                    kind: ErrorKind::UnresolvedSharedExport {
                        name: name.name.to_owned(),
                    },
                });
            }
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
        let mut optional_accesses = BTreeSet::new();
        for (record, field, region) in std::mem::take(&mut self.field_accesses) {
            if let Ty::Record(fields, _) = self.prune(record)
                && matches!(fields.get(field), Some((FieldPresence::Optional, _)))
            {
                optional_accesses.insert(region);
            }
        }
        Ok(InferenceResult {
            optional_accesses,
            annotations,
            bindings,
            obligations,
            calls: std::mem::take(&mut self.calls),
            variable_names: std::mem::take(&mut self.variable_names),
            generalized_variables: std::mem::take(&mut self.generalized_variables),
        })
    }

    fn predeclare(&mut self, env: &mut Env<'a>, name: QualifiedName<'a>) {
        let typ = self.fresh();
        env.globals.insert(
            name,
            Scheme {
                record_overlays: Vec::new(),
                error_row_inclusions: Vec::new(),
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
        let is_async = matches!(item, ItemKind::Fn(function) if function.is_async);
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
        let mut ret = match ret {
            Some(typ) => self.from_ast(typ, &mut vars),
            None => self.fresh(),
        };
        if is_async {
            ret = self.named("Task", vec![ret]);
        }
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
                let excluded = BTreeSet::from([function.name]);
                let outer_free = self.environment_free_vars(env, &excluded);
                self.generalize_global(env, function.name, &outer_free, true);
            }
            ItemKind::Extern(alder_ast::ExternDecl::Fn { name, .. }) => {
                self.infer_value_item(env, item, region)?;
                let excluded = BTreeSet::from([*name]);
                let outer_free = self.environment_free_vars(env, &excluded);
                self.generalize_global(env, *name, &outer_free, true);
            }
            ItemKind::Let(decl) => {
                self.infer_value_item(env, item, region)?;
                let excluded = decl.bindings.iter().copied().collect();
                let outer_free = self.environment_free_vars(env, &excluded);
                for binding in decl.bindings {
                    self.generalize_global(env, *binding, &outer_free, generalizable_item(item));
                }
            }
            ItemKind::Component(component) => {
                self.infer_value_item(env, item, region)?;
                let excluded = BTreeSet::from([component.name]);
                let outer_free = self.environment_free_vars(env, &excluded);
                self.generalize_global(env, component.name, &outer_free, true);
            }
            ItemKind::Impl(impl_) => {
                self.require_impl_superclasses(impl_, region);
                for item in impl_.items {
                    if let alder_ast::ImplItem::Fn(function) = item {
                        self.infer_function(
                            env,
                            FunctionInput {
                                is_async: function.is_async,
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
                                is_async: function.is_async,
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
                let errors = self.fresh_error_row();
                let result = self.named("Result", vec![Ty::Unit, errors]);
                self.infer_block(&mut env.clone(), test.body, Some(result))?;
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
                        is_async: function.is_async,
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
                let value = if let Some(annotation) = decl.annotation {
                    let annotated = self.from_ast(annotation, &mut BTreeMap::new());
                    self.infer_checked_expr(env, decl.value, annotated, None)?
                } else {
                    self.infer_expr(env, decl.value, None)?
                };
                self.infer_pattern(env, decl.pattern, value, true)
            }
            ItemKind::Component(component) => {
                let (typ, predicates, projection_eqs) = self.infer_function(
                    env,
                    FunctionInput {
                        is_async: false,
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
            is_async,
            params,
            ret,
            constraints,
            context,
            body,
            region,
        } = input;
        let excluded = self.active_scc.clone();
        let outer_free = self.environment_free_vars(env, &excluded);
        let mut local = env.clone();
        let mut vars = BTreeMap::new();
        if let FunctionContext::Default(trait_) = context {
            // The whole trait head scopes over a default body, even when a
            // method's parameter/result annotations do not mention it.
            for parameter in trait_.type_params {
                vars.insert(parameter.name.value, self.fresh());
            }
        }
        let mut args = Vec::with_capacity(params.len());
        for param in params {
            let typ = match param.annotation {
                Some(annotation) => self.from_ast(annotation, &mut vars),
                None => self.fresh(),
            };
            self.infer_pattern(&mut local, param.pattern, typ.clone(), false)?;
            args.push(typ);
        }
        let declared_result = match ret {
            Some(ret) => self.from_ast(ret, &mut vars),
            None => self.fresh(),
        };
        let (result, body_result) = if is_async {
            (
                self.named("Task", vec![declared_result.clone()]),
                declared_result,
            )
        } else {
            (declared_result.clone(), declared_result)
        };
        let predicates = self.predicates_from_constraints(constraints, &vars);
        let local_projection_equations =
            self.projection_equations_from_constraints(constraints, &vars)?;
        // An implementation receives exactly the dictionaries promised by the
        // trait declaration, in declaration order. Its own bounds are proof
        // obligations, not additional arguments in the method ABI.
        let mut method_predicates = Vec::new();
        let mut method_equations = Vec::new();
        let mut expected_method = None;
        if let FunctionContext::Impl {
            implementation,
            method,
        } = context
        {
            let mut expected_vars = BTreeMap::new();
            if let Some(header) = self.database.trait_(implementation.trait_ref.trait_) {
                for (parameter, argument) in header.params.iter().zip(implementation.trait_ref.args)
                {
                    expected_vars.insert(parameter.name.value, self.from_ast(argument, &mut vars));
                }
            }
            let subject_variables = expected_vars.keys().copied().collect::<BTreeSet<_>>();
            let expected = self.from_ast(method.scheme.typ, &mut expected_vars);
            expected_method = Some((expected, method.name.region));
            for predicate in method.scheme.trait_predicates {
                method_predicates
                    .push(self.predicate_from_trait_ref(*predicate, &mut expected_vars));
            }
            for equality in method.scheme.projection_equalities {
                let projection = self.projection_from_ast(equality.projection, &mut expected_vars);
                let typ = self.from_ast(equality.typ, &mut expected_vars);
                method_equations.push(ProjectionEquation { projection, typ });
            }
            expected_vars.retain(|name, _| !subject_variables.contains(name));
            self.generic_contracts.push(GenericContract {
                variables: expected_vars,
                outer_free: outer_free.clone(),
                region: method.name.region,
            });
        }
        for (name, typ) in &vars {
            if let Ty::Var(id) = typ {
                self.variable_names.entry(*id).or_insert(name);
            }
        }
        let outer_givens = std::mem::take(&mut self.givens);
        let outer_projection_equations = std::mem::take(&mut self.projection_equations);
        self.givens = outer_givens.clone();
        self.projection_equations = outer_projection_equations.clone();
        self.projection_equations
            .extend(if matches!(context, FunctionContext::Impl { .. }) {
                method_equations
            } else {
                local_projection_equations.clone()
            });
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
                self.add_parameter_givens(&method_predicates, prerequisites.len());
                self.add_parameter_superclass_givens(&method_predicates, prerequisites.len());
                for predicate in &predicates {
                    self.obligations.push(Obligation {
                        use_id: None,
                        predicate: predicate.clone(),
                        region,
                        action: ObligationAction::ContractCheck,
                        givens: self.givens.clone(),
                    });
                }
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
                                .expect("default method scope contains every trait parameter")
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
        self.generic_contracts.push(GenericContract {
            variables: vars.clone(),
            outer_free: outer_free.clone(),
            region,
        });
        let outer_annotation_scope = std::mem::replace(&mut self.annotation_scope, vars);
        let inferred = (|| {
            if let Some((expected, method_region)) = expected_method {
                self.unify(
                    Ty::Fn(args.clone(), Box::new(result.clone())),
                    expected,
                    method_region,
                )?;
            }
            if matches!(context, FunctionContext::Impl { .. }) {
                for equation in &local_projection_equations {
                    self.unify(equation.projection.clone(), equation.typ.clone(), region)?;
                }
            }
            let body_type = self.infer_block(&mut local, body, Some(body_result.clone()))?;
            if alder_ast::flow::block(body).falls_through {
                let expected = self.render(body_result.clone());
                self.unify_return(body_type, body_result, region)
                    .map_err(|error| {
                        if body.value.tail.is_none() {
                            Error {
                                region: body.region,
                                kind: ErrorKind::MissingReturn { expected },
                            }
                        } else {
                            error
                        }
                    })?;
            }
            let function_type = Ty::Fn(args, Box::new(self.prune(result)));
            Ok((function_type, predicates, local_projection_equations))
        })();
        self.givens = outer_givens;
        self.projection_equations = outer_projection_equations;
        self.annotation_scope = outer_annotation_scope;
        inferred
    }

    fn with_reachability<T>(
        &mut self,
        reachable: bool,
        infer: impl FnOnce(&mut Self) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let outer = self.reachable;
        self.reachable &= reachable;
        let result = infer(self);
        self.reachable = outer;
        result
    }

    fn infer_block(
        &mut self,
        env: &mut Env<'a>,
        block: &'a Located<Block<'a>>,
        return_type: Option<Ty<'a>>,
    ) -> Result<Ty<'a>, Error> {
        self.with_reachability(true, |this| {
            for statement in block.value.statements {
                this.infer_stmt(env, statement, return_type.clone())?;
                this.reachable &= alder_ast::flow::statement(statement).falls_through;
            }
            let result = match block.value.tail {
                Some(tail) => this.infer_expr(env, tail, return_type),
                None => Ok(Ty::Unit),
            }?;
            if alder_ast::flow::block(block).falls_through {
                Ok(result)
            } else {
                Ok(this.fresh())
            }
        })
    }

    fn infer_loop_body(
        &mut self,
        env: &mut Env<'a>,
        block: &'a Located<Block<'a>>,
        return_type: Option<Ty<'a>>,
        result: Ty<'a>,
    ) -> Result<Ty<'a>, Error> {
        self.loop_results.push(result);
        let body = self.infer_block(env, block, return_type);
        let result = self.loop_results.pop().expect("loop result frame");
        body.map(|_| result)
    }

    fn infer_stmt(
        &mut self,
        env: &mut Env<'a>,
        statement: &'a Located<Stmt<'a>>,
        return_type: Option<Ty<'a>>,
    ) -> Result<(), Error> {
        match &statement.value {
            Stmt::Let(decl) => {
                let value = if let Some(annotation) = decl.annotation {
                    // As in lambda signatures, existing names refer to the
                    // enclosing contract. New names are local to this annotation;
                    // the let binding itself remains monomorphic.
                    let annotated = self.from_ast(annotation, &mut self.annotation_scope.clone());
                    self.infer_checked_expr(env, decl.value, annotated, return_type.clone())?
                } else {
                    self.infer_expr(env, decl.value, return_type.clone())?
                };
                self.infer_pattern(env, decl.pattern, value, false)?;
            }
            Stmt::Use { .. } => {}
            Stmt::Assign {
                use_id,
                place,
                value,
                ..
            } => {
                let expected = self.place_type(
                    env,
                    place,
                    statement.region,
                    use_id.is_some(),
                    return_type.clone(),
                )?;
                let actual = self.infer_expr(env, value, return_type.clone())?;
                self.check_value(actual, expected.clone(), statement.region)?;
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
                self.with_reachability(alder_ast::flow::expression(iter).falls_through, |this| {
                    this.infer_loop_body(&mut nested, body, return_type, Ty::Unit)
                })?;
            }
            Stmt::While { condition, body } => {
                let condition_type = self.infer_expr(env, condition, return_type.clone())?;
                self.unify(
                    condition_type,
                    self.named("Bool", Vec::new()),
                    condition.region,
                )?;
                self.with_reachability(
                    alder_ast::flow::expression(condition).falls_through
                        && !matches!(condition.value, Expr::Bool(false)),
                    |this| this.infer_loop_body(&mut env.clone(), body, return_type, Ty::Unit),
                )?;
            }
            Stmt::Return(value) => {
                let expected = return_type.unwrap_or(Ty::Unit);
                let actual = match value {
                    Some(value) => self.infer_expr(env, value, Some(expected.clone()))?,
                    None => Ty::Unit,
                };
                if matches!(self.prune(expected.clone()), Ty::Var(_))
                    && let Some((_, errors)) = self.result_parts(actual.clone())
                    && match self.prune(errors) {
                        Ty::ErrorRow { .. } => true,
                        Ty::Var(id) => self.variable_kinds[id] == VariableKind::ErrorRow,
                        _ => false,
                    }
                {
                    // An early return contributes a lower bound; later returns
                    // and `?` may contribute other errors to the same result.
                    self.require_result_parts(expected.clone(), statement.region)?;
                }
                self.unify_return(actual, expected, statement.region)?;
            }
            Stmt::Break(value) => {
                let actual = match value {
                    Some(value) => self.infer_expr(env, value, return_type)?,
                    None => Ty::Unit,
                };
                let expected = self
                    .loop_results
                    .last()
                    .cloned()
                    .expect("canonicalization rejects break outside a loop");
                if self.reachable
                    && value.is_none_or(|value| alder_ast::flow::expression(value).falls_through)
                {
                    let joined = self.join_values(expected, actual, statement.region)?;
                    *self.loop_results.last_mut().expect("loop result frame") = joined;
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
            Expr::Str(_) => Ok(self.named("String", Vec::new())),
            Expr::Template(parts) => {
                for part in *parts {
                    if let alder_ast::TemplatePart::Expr(expr) = part {
                        self.infer_expr(env, expr, return_type.clone())?;
                    }
                }
                Ok(self.named("String", Vec::new()))
            }
            Expr::TaggedTemplate { tag, parts } => {
                let function_type = self.infer_expr(env, tag, return_type.clone())?;
                let strings = self.named("Array", vec![self.named("String", Vec::new())]);
                let mut args = vec![strings];
                for part in *parts {
                    if let alder_ast::TemplatePart::Expr(argument) = part {
                        let expected = match self.prune(function_type.clone()) {
                            Ty::Fn(params, _) => params.get(args.len()).cloned(),
                            _ => None,
                        };
                        args.push(
                            if let Some(expected) = expected
                                && matches!(argument.value, Expr::Record(_) | Expr::Array(_))
                            {
                                self.infer_checked_expr(
                                    env,
                                    argument,
                                    expected,
                                    return_type.clone(),
                                )?
                            } else {
                                self.infer_expr(env, argument, return_type.clone())?
                            },
                        );
                    }
                }
                let result = self.fresh();
                let call_type = Ty::Fn(args, Box::new(result.clone()));
                self.value_checks
                    .push((function_type.clone(), call_type.clone(), region));
                self.unify(call_type, function_type, region)?;
                self.solve_record_overlays()?;
                // Tagged calls use the tag as a function value. Its reference
                // retains dictionary evidence rather than becoming a direct call.
                Ok(self.prune(result))
            }
            Expr::Bool(_) => Ok(self.named("Bool", Vec::new())),
            Expr::Unit => Ok(Ty::Unit),
            Expr::Var { use_id, reference } => {
                self.infer_reference(env, *use_id, *reference, region)
            }
            Expr::Constructor(constructor) => {
                Ok(self.instantiate_annotation(constructor.annotation, region))
            }
            Expr::Tag { name, args, .. } => {
                self.tag_sites.push(region);
                let mut payloads = Vec::with_capacity(args.len());
                for arg in *args {
                    payloads.push(self.infer_expr(env, arg, return_type.clone())?);
                }
                Ok(Ty::ErrorRow {
                    tags: BTreeMap::from([(name.value, payloads)]),
                    tail: None,
                })
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
                let constructor_type = self.instantiate_annotation(constructor.annotation, region);
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
            } => self.infer_call(
                env,
                CallInput {
                    region,
                    use_id: *use_id,
                    function,
                    arguments,
                    leading: None,
                },
                return_type,
            ),
            Expr::Access { record, field } => {
                let record_type = self.infer_expr(env, record, return_type)?;
                self.field_accesses
                    .push((record_type.clone(), field.value, region));
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
                let actual = self.infer_expr(env, expr, return_type)?;
                self.infer_await_type(actual, region)
            }
            Expr::Try(expr) => {
                let actual = self.infer_expr(env, expr, return_type.clone())?;
                self.infer_try_type(actual, return_type, region)
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
            Expr::Async(block) => {
                let result = self.fresh();
                let outer_loops = std::mem::take(&mut self.loop_results);
                let outer_reachable = std::mem::replace(&mut self.reachable, true);
                let body_type = self.infer_block(&mut env.clone(), block, Some(result.clone()));
                self.reachable = outer_reachable;
                self.loop_results = outer_loops;
                let body_type = body_type?;
                if alder_ast::flow::block(block).falls_through {
                    self.unify_return(body_type, result.clone(), region)?;
                }
                let result = self.prune(result);
                Ok(self.named("Task", vec![result]))
            }
            Expr::Lambda { params, ret, body } => {
                let mut local = env.clone();
                let mut vars = self.annotation_scope.clone();
                let mut args = Vec::with_capacity(params.len());
                for param in *params {
                    let typ = param
                        .annotation
                        .map(|annotation| self.from_ast(annotation, &mut vars))
                        .unwrap_or_else(|| self.fresh());
                    self.infer_pattern(&mut local, param.pattern, typ.clone(), false)?;
                    args.push(typ);
                }
                let declared_result = ret
                    .map(|ret| self.from_ast(ret, &mut vars))
                    .unwrap_or_else(|| self.fresh());
                let (result, body_result) = (declared_result.clone(), declared_result);
                let outer_annotation_scope = std::mem::replace(&mut self.annotation_scope, vars);
                let outer_loops = std::mem::take(&mut self.loop_results);
                let outer_reachable = std::mem::replace(&mut self.reachable, true);
                let body_type = self.infer_expr(&local, body, Some(body_result.clone()));
                self.reachable = outer_reachable;
                self.loop_results = outer_loops;
                self.annotation_scope = outer_annotation_scope;
                let body_type = body_type?;
                if alder_ast::flow::expression(body).falls_through {
                    self.unify_return(body_type, body_result, region)?;
                }
                Ok(Ty::Fn(args, Box::new(self.prune(result))))
            }
            Expr::If {
                branches,
                final_else,
            } => {
                let mut result = self.fresh();
                let mut remaining = self.reachable;
                for branch in *branches {
                    let condition = self.with_reachability(remaining, |this| {
                        this.infer_expr(env, branch.condition, return_type.clone())
                    })?;
                    self.unify(
                        condition,
                        self.named("Bool", Vec::new()),
                        branch.condition.region,
                    )?;
                    remaining &= alder_ast::flow::expression(branch.condition).falls_through;
                    let body = self.with_reachability(
                        remaining && !matches!(branch.condition.value, Expr::Bool(false)),
                        |this| this.infer_block(&mut env.clone(), branch.body, return_type.clone()),
                    )?;
                    result = self.join_values(result, body, branch.body.region)?;
                    remaining &= !matches!(branch.condition.value, Expr::Bool(true));
                }
                if let Some(final_else) = final_else {
                    let body = self.with_reachability(remaining, |this| {
                        this.infer_block(&mut env.clone(), final_else, return_type)
                    })?;
                    result = self.join_values(result, body, final_else.region)?;
                } else {
                    self.unify(Ty::Unit, result.clone(), region)?;
                }
                Ok(self.prune(result))
            }
            Expr::Match { scrutinee, arms } => {
                let scrutinee_type = self.infer_expr(env, scrutinee, return_type.clone())?;
                self.match_sites.push(MatchSite {
                    scrutinee: scrutinee_type.clone(),
                    arms,
                    region,
                });
                let mut result = self.fresh();
                for arm in *arms {
                    let mut local = env.clone();
                    for pattern in arm.patterns {
                        self.infer_pattern(&mut local, pattern, scrutinee_type.clone(), false)?;
                    }
                    let scrutinee_continues = alder_ast::flow::expression(scrutinee).falls_through;
                    if let Some(guard) = arm.guard {
                        let guard_type = self.with_reachability(scrutinee_continues, |this| {
                            this.infer_expr(&local, guard, return_type.clone())
                        })?;
                        self.unify(guard_type, self.named("Bool", Vec::new()), guard.region)?;
                    }
                    let body = self.with_reachability(
                        scrutinee_continues
                            && arm.guard.is_none_or(|guard| {
                                alder_ast::flow::expression(guard).falls_through
                                    && !matches!(guard.value, Expr::Bool(false))
                            }),
                        |this| this.infer_expr(&local, arm.body, return_type.clone()),
                    )?;
                    result = self.join_values(result, body, arm.body.region)?;
                }
                Ok(self.prune(result))
            }
            Expr::Loop(block) => {
                let result = self.fresh();
                let result = self.infer_loop_body(&mut env.clone(), block, return_type, result)?;
                if alder_ast::flow::expression(expression).falls_through {
                    Ok(self.prune(result))
                } else {
                    Ok(self.fresh())
                }
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
            ValueRef::Local(local) => Ok(self.instantiate(&env.locals[&local.id.0], region)),
            ValueRef::TopLevel(name) => {
                let (typ, predicates) = self.instantiate_scheme(&env.globals[&name], region);
                self.record_predicates(
                    use_id,
                    predicates,
                    region,
                    ObligationAction::Reference(None),
                );
                Ok(typ)
            }
            ValueRef::Foreign { annotation, .. } => {
                let (typ, vars) = self.instantiate_annotation_with_vars(annotation, region);
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
                let (typ, vars) = self.instantiate_annotation_with_vars(annotation, origin);
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
        let mut operands = Vec::new();
        for field in fields {
            let (typ, region) = match field {
                RecordField::Field { name, value } => {
                    let typ = self.infer_expr(env, value, return_type.clone())?;
                    (
                        Ty::Record(
                            BTreeMap::from([(name.value, (FieldPresence::Required, typ))]),
                            None,
                        ),
                        value.region,
                    )
                }
                RecordField::Spread(expr) => {
                    let typ = self.infer_expr(env, expr, return_type.clone())?;
                    let expected = self.open_record(BTreeMap::new());
                    self.unify(typ.clone(), expected, expr.region)?;
                    (typ, expr.region)
                }
            };
            operands.push((typ, region));
        }
        let open_count = operands
            .iter()
            .filter(|(typ, _)| matches!(self.prune(typ.clone()), Ty::Record(_, Some(_))))
            .count();
        if open_count > 1 {
            let result = self.open_record(BTreeMap::new());
            self.record_overlays.push(RecordOverlay {
                operands: operands.iter().map(|(typ, _)| typ.clone()).collect(),
                result: result.clone(),
                region: operands
                    .last()
                    .map(|(_, region)| *region)
                    .unwrap_or_else(Region::zero),
            });
            return Ok(result);
        }
        self.merge_record_operands(operands)
    }

    fn merge_record_operands(&mut self, operands: Vec<(Ty<'a>, Region)>) -> Result<Ty<'a>, Error> {
        enum Payload<'a> {
            Known(Ty<'a>, Region),
            OpenRow(Ty<'a>, Region),
        }
        let mut result: BTreeMap<&'a str, (FieldPresence, Vec<Payload<'a>>)> = BTreeMap::new();
        let mut tail: Option<Box<Ty<'a>>> = None;
        for (spread, region) in operands {
            if let Ty::Record(fields, inherited) = self.prune(spread) {
                if let Some(inherited) = &inherited {
                    for (name, (_, alternatives)) in &mut result {
                        if !fields.contains_key(name) {
                            alternatives.push(Payload::OpenRow((**inherited).clone(), region));
                        }
                    }
                }
                for (name, (presence, typ)) in fields {
                    if presence == FieldPresence::Optional
                        && let Some((_, alternatives)) = result.get_mut(name)
                    {
                        // An absent spread property leaves the earlier
                        // value intact. Both payloads are possible, and
                        // an existing required property stays present.
                        alternatives.push(Payload::Known(typ, region));
                    } else {
                        let mut alternatives = Vec::new();
                        if presence == FieldPresence::Optional
                            && let Some(previous) = &tail
                        {
                            alternatives.push(Payload::OpenRow((**previous).clone(), region));
                        }
                        alternatives.push(Payload::Known(typ, region));
                        result.insert(name, (presence, alternatives));
                    }
                }
                if let (Some(previous), Some(next)) = (&tail, &inherited) {
                    self.unify((**previous).clone(), (**next).clone(), region)?;
                }
                if inherited.is_some() {
                    tail = inherited;
                }
            }
        }
        // A later required property discards every earlier alternative. Join
        // only the payloads that can survive in the completed record.
        let mut joined = BTreeMap::new();
        for (name, (presence, alternatives)) in result {
            let mut joined_payload = None;
            for alternative in alternatives {
                let (other, region) = match alternative {
                    Payload::Known(typ, region) => (typ, region),
                    Payload::OpenRow(row, region) => {
                        let typ = self.fresh();
                        let expected = self.open_record(BTreeMap::from([(
                            name,
                            (FieldPresence::Optional, typ.clone()),
                        )]));
                        self.unify(row, Ty::RecordRow(Box::new(expected)), region)?;
                        (typ, region)
                    }
                };
                joined_payload = Some(match joined_payload {
                    Some(typ) => self.join_values(typ, other, region)?,
                    None => other,
                });
            }
            joined.insert(
                name,
                (presence, joined_payload.expect("each field has a payload")),
            );
        }
        Ok(Ty::Record(joined, tail))
    }

    fn solve_record_overlays(&mut self) -> Result<(), Error> {
        let mut pending = std::mem::take(&mut self.record_overlays);
        loop {
            let bound_before = self
                .substitutions
                .iter()
                .filter(|typ| typ.is_some())
                .count();
            // Ordered overlay is a type-level function: equal input shapes
            // cannot independently choose incompatible output payloads. Keep
            // both constraints (and their sites), but identify their results.
            for right in 0..pending.len() {
                for left in 0..right {
                    let left_operands = pending[left]
                        .operands
                        .iter()
                        .map(|operand| self.prune(operand.clone()))
                        .collect::<Vec<_>>();
                    let right_operands = pending[right]
                        .operands
                        .iter()
                        .map(|operand| self.prune(operand.clone()))
                        .collect::<Vec<_>>();
                    if left_operands == right_operands {
                        self.unify(
                            pending[left].result.clone(),
                            pending[right].result.clone(),
                            pending[right].region,
                        )?;
                    }
                }
            }
            let mut unresolved = Vec::new();
            let mut progress = false;
            for overlay in pending {
                let operands = overlay
                    .operands
                    .iter()
                    .map(|operand| self.prune(operand.clone()))
                    .collect::<Vec<_>>();
                if operands
                    .iter()
                    .all(|operand| matches!(operand, Ty::Record(_, None)))
                {
                    let merged = self.merge_record_operands(
                        operands
                            .into_iter()
                            .map(|operand| (operand, overlay.region))
                            .collect(),
                    )?;
                    self.check_value(merged, overlay.result, overlay.region)?;
                    progress = true;
                } else {
                    progress |= self.expose_overlay_fields(&overlay, &operands)?;
                    unresolved.push(overlay);
                }
            }
            // Payload equalities can make another overlay solvable without
            // adding a field. Count bindings, not fresh variables, so repeated
            // checks that allocate no new information cannot sustain the loop.
            progress |= self
                .substitutions
                .iter()
                .filter(|typ| typ.is_some())
                .count()
                != bound_before;
            if !progress {
                self.record_overlays = unresolved;
                return Ok(());
            }
            pending = unresolved;
        }
    }

    /// Expose only fields whose final presence and payload are independent of
    /// unresolved input tails. In particular, an unknown right-hand tail may
    /// overwrite an earlier field; do not constrain that tail just to publish
    /// a speculative result shape.
    fn expose_overlay_fields(
        &mut self,
        overlay: &RecordOverlay<'a>,
        operands: &[Ty<'a>],
    ) -> Result<bool, Error> {
        let names = operands
            .iter()
            .filter_map(|operand| match operand {
                Ty::Record(fields, _) => Some(fields.keys().copied()),
                _ => None,
            })
            .flatten()
            .collect::<BTreeSet<_>>();
        let Ty::Record(existing, _) = self.prune(overlay.result.clone()) else {
            return Ok(false);
        };
        let mut additions = BTreeMap::new();
        for name in names {
            let mut payloads = Vec::new();
            let mut presence = FieldPresence::Optional;
            let mut known = true;
            for operand in operands.iter().rev() {
                let Ty::Record(fields, tail) = operand else {
                    known = false;
                    break;
                };
                if let Some((field_presence, typ)) = fields.get(name) {
                    payloads.push(typ.clone());
                    if *field_presence == FieldPresence::Required {
                        presence = FieldPresence::Required;
                        break;
                    }
                } else if tail.is_some() {
                    known = false;
                    break;
                }
            }
            if !known {
                continue;
            }
            let mut payloads = payloads.into_iter();
            let Some(mut typ) = payloads.next() else {
                continue;
            };
            for other in payloads {
                typ = self.join_values(typ, other, overlay.region)?;
            }
            if let Some(expected) = existing.get(name) {
                self.check_value(
                    Ty::Record(BTreeMap::from([(name, (presence, typ))]), None),
                    Ty::Record(BTreeMap::from([(name, expected.clone())]), None),
                    overlay.region,
                )?;
            } else {
                additions.insert(name, (presence, typ));
            }
        }
        if additions.is_empty() {
            return Ok(false);
        }
        let shape = self.open_record(additions);
        self.unify(overlay.result.clone(), shape, overlay.region)?;
        Ok(true)
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
                            record_overlays: Vec::new(),
                            error_row_inclusions: Vec::new(),
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
                let constructor_type =
                    self.instantiate_annotation(constructor.annotation, pattern.region);
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
                let constructor_type =
                    self.instantiate_annotation(constructor.annotation, pattern.region);
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
                let record = Ty::Record(
                    declared
                        .iter()
                        .zip(arg_types)
                        .map(|(field, typ)| (field.name, (field.presence, typ)))
                        .collect(),
                    None,
                );
                for field in *fields {
                    self.field_accesses
                        .push((record.clone(), field.name.value, field.name.region));
                    let typ =
                        self.access_field(record.clone(), field.name.value, field.name.region)?;
                    self.infer_pattern(env, field.pattern, typ, false)?;
                }
            }
            Pattern::Record { fields, .. } => {
                let record = self.open_record(BTreeMap::new());
                self.unify(expected.clone(), record, pattern.region)?;
                for field in *fields {
                    self.field_accesses.push((
                        expected.clone(),
                        field.name.value,
                        field.name.region,
                    ));
                    let typ =
                        self.access_field(expected.clone(), field.name.value, field.name.region)?;
                    self.infer_pattern(env, field.pattern, typ, false)?;
                }
            }
            Pattern::Tag { name, args, .. } => {
                if let Ty::ErrorRow { tags, tail: None } = self.prune(expected.clone()) {
                    let Some(payloads) = tags.get(name.value) else {
                        return Err(Error {
                            region: pattern.region,
                            kind: ErrorKind::ImpossibleErrorPattern {
                                tag: name.value.to_owned(),
                            },
                        });
                    };
                    if payloads.len() != args.len() {
                        return Err(Error {
                            region: pattern.region,
                            kind: ErrorKind::Arity {
                                expected: payloads.len(),
                                actual: args.len(),
                            },
                        });
                    }
                    for (arg, typ) in args.iter().zip(payloads.iter().cloned()) {
                        self.infer_pattern(env, arg, typ, false)?;
                    }
                    return Ok(());
                }
                let mut payloads = Vec::with_capacity(args.len());
                for arg in *args {
                    let typ = self.fresh();
                    self.infer_pattern(env, arg, typ.clone(), false)?;
                    payloads.push(typ);
                }
                let tail = self.fresh_with_kind(VariableKind::ErrorRow);
                self.unify(
                    expected,
                    Ty::ErrorRow {
                        tags: BTreeMap::from([(name.value, payloads)]),
                        tail: Some(Box::new(tail)),
                    },
                    pattern.region,
                )?;
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
                            record_overlays: Vec::new(),
                            error_row_inclusions: Vec::new(),
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
                            record_overlays: Vec::new(),
                            error_row_inclusions: Vec::new(),
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

    fn infer_await_type(&mut self, actual: Ty<'a>, region: Region) -> Result<Ty<'a>, Error> {
        let value = self.fresh();
        self.unify(actual, self.named("Task", vec![value.clone()]), region)?;
        Ok(self.prune(value))
    }

    fn infer_try_type(
        &mut self,
        actual: Ty<'a>,
        return_type: Option<Ty<'a>>,
        region: Region,
    ) -> Result<Ty<'a>, Error> {
        let value = self.fresh();
        let error = self.fresh_error_row();
        self.unify(
            actual,
            self.named("Result", vec![value.clone(), error.clone()]),
            region,
        )?;
        let Some(return_type) = return_type else {
            return Err(Error {
                region,
                kind: ErrorKind::InvalidTry,
            });
        };
        let (_, enclosing_errors) = self.require_result_parts(return_type, region)?;
        self.include_error_rows(error, enclosing_errors, region)?;
        Ok(self.prune(value))
    }

    fn infer_pipe_destination(
        &mut self,
        env: &Env<'a>,
        destination: &'a Located<Expr<'a>>,
        leading: Ty<'a>,
        leading_region: Region,
        return_type: Option<Ty<'a>>,
    ) -> Result<Ty<'a>, Error> {
        match destination.value {
            Expr::Call {
                use_id,
                function,
                arguments,
            } => self.infer_call(
                env,
                CallInput {
                    region: destination.region,
                    use_id,
                    function,
                    arguments,
                    leading: Some((leading, leading_region)),
                },
                return_type,
            ),
            Expr::Await(inner) => {
                let actual =
                    self.infer_pipe_destination(env, inner, leading, leading_region, return_type)?;
                self.infer_await_type(actual, destination.region)
            }
            Expr::Try(inner) => {
                let actual = self.infer_pipe_destination(
                    env,
                    inner,
                    leading,
                    leading_region,
                    return_type.clone(),
                )?;
                self.infer_try_type(actual, return_type, destination.region)
            }
            _ => {
                let destination_type = self.infer_expr(env, destination, return_type)?;
                let result = self.fresh();
                self.check_value(
                    destination_type,
                    Ty::Fn(vec![leading], Box::new(result.clone())),
                    destination.region,
                )?;
                self.solve_record_overlays()?;
                Ok(self.prune(result))
            }
        }
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
        if op == BinOp::Pipe {
            return self
                .with_reachability(alder_ast::flow::expression(left).falls_through, |this| {
                    this.infer_pipe_destination(env, right, left_type, left.region, return_type)
                });
        }

        let right_type = self
            .with_reachability(alder_ast::flow::binary_rhs_reachable(op, left), |this| {
                this.infer_expr(env, right, return_type)
            })?;
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
                let optional = self.named("Option", vec![right_type.clone()]);
                self.unify(left_type, optional, left.region)?;
                Ok(self.prune(right_type))
            }
            BinOp::Pipe => unreachable!("pipe expressions return before ordinary binop inference"),
            BinOp::In => Ok(self.named("Bool", Vec::new())),
        }
    }

    fn infer_call(
        &mut self,
        env: &Env<'a>,
        call: CallInput<'a>,
        return_type: Option<Ty<'a>>,
    ) -> Result<Ty<'a>, Error> {
        let CallInput {
            region,
            use_id,
            function,
            arguments,
            leading,
        } = call;
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
            } => (Some(use_id), Some(DirectTarget::Binding(name))),
            Expr::Var {
                use_id,
                reference: ValueRef::TraitMethod { method, .. },
            } => (Some(use_id), Some(DirectTarget::TraitMethod(method))),
            Expr::Var { use_id, .. } => (Some(use_id), None),
            _ => (None, None),
        };
        let function_type = self.infer_expr(env, function, return_type.clone())?;
        let mut args = Vec::with_capacity(arguments.len() + usize::from(leading.is_some()));
        let accepts_error_tag = is_result_err_expr(function);
        if let Some((typ, _)) = &leading {
            args.push(typ.clone());
        }
        if accepts_error_tag {
            if let Some((_, region)) = &leading {
                self.legal_tag_sites.push(*region);
            } else if let Some(argument) = arguments.first() {
                self.legal_tag_sites.push(argument.region);
            }
        }
        for argument in arguments {
            let expected = match self.prune(function_type.clone()) {
                Ty::Fn(params, _) => params.get(args.len()).cloned(),
                _ => None,
            };
            args.push(
                if let Some(expected) = expected
                    && matches!(argument.value, Expr::Record(_) | Expr::Array(_))
                {
                    self.infer_checked_expr(env, argument, expected, return_type.clone())?
                } else {
                    self.infer_expr(env, argument, return_type.clone())?
                },
            );
        }
        let result = self.fresh();
        let call_type = Ty::Fn(args, Box::new(result.clone()));
        let call_region = leading.map_or(region, |(_, region)| region);
        self.value_checks
            .push((function_type.clone(), call_type.clone(), call_region));
        self.unify(call_type, function_type, call_region)?;
        // Arguments can close an instantiated overlay. Resolve its shape
        // before a caller reads or destructures the result: optional fields
        // must produce Option payloads at that use site, not be guessed as
        // required fields and corrected only after expression inference.
        self.solve_record_overlays()?;
        self.calls.push(CallSite {
            use_id,
            callee_use,
            target,
        });
        Ok(self.prune(result))
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
        read_before_write: bool,
        return_type: Option<Ty<'a>>,
    ) -> Result<Ty<'a>, Error> {
        let mut typ = match place.root {
            BindingName::Local(local) => self.instantiate(&env.locals[&local.id.0], region),
            BindingName::TopLevel(name) => self.instantiate(&env.globals[&name], region),
        };
        for (index, step) in place.steps.iter().enumerate() {
            typ = match step {
                alder_ast::PlaceStep::Field(field) => {
                    // The final member stores a raw payload, even when reading
                    // that member would produce Option[T]. Intermediate members
                    // remain reads: an optional parent cannot be traversed as T.
                    let payload = if index + 1 == place.steps.len() {
                        match self.prune(typ.clone()) {
                            Ty::Record(fields, _) => {
                                fields.get(field.value).map(|(_, typ)| typ.clone())
                            }
                            _ => None,
                        }
                    } else {
                        None
                    };
                    match payload {
                        Some(payload) => {
                            if read_before_write {
                                let read = self.access_field(typ, field.value, field.region)?;
                                self.check_value(read, payload.clone(), field.region)?;
                            }
                            payload
                        }
                        None => self.access_field(typ, field.value, field.region)?,
                    }
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
                    let index_type = self.infer_expr(env, index, return_type.clone())?;
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
            Ty::Record(fields, tail) => match fields.get(field) {
                Some((FieldPresence::Required, typ)) => Ok(typ.clone()),
                Some((FieldPresence::Optional, typ)) => Ok(self.named("Option", vec![typ.clone()])),
                None if tail.is_some() => {
                    let result = self.fresh();
                    let fields =
                        BTreeMap::from([(field, (FieldPresence::Required, result.clone()))]);
                    let fragment = self.open_record(fields);
                    self.unify(
                        *tail.expect("open tail"),
                        Ty::RecordRow(Box::new(fragment)),
                        region,
                    )?;
                    Ok(result)
                }
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
                let record = self.open_record(fields);
                self.bind(id, record, region)?;
                Ok(result)
            }
            Ty::Any => Ok(Ty::Any),
            actual => {
                let expected = self.open_record(BTreeMap::new());
                Err(self.mismatch(region, actual, expected))
            }
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

    fn generalize_global(
        &mut self,
        env: &mut Env<'a>,
        name: QualifiedName<'a>,
        outer_free: &BTreeSet<usize>,
        generalizable: bool,
    ) {
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
        for predicate in &predicates {
            for argument in &predicate.args {
                self.free_vars(argument, &mut vars);
            }
        }
        for equation in &projection_eqs {
            self.free_vars(&equation.projection, &mut vars);
            self.free_vars(&equation.typ, &mut vars);
        }
        // Constraints may connect a visible result tail to otherwise hidden
        // intermediate tails. Preserve the whole connected component, not just
        // constraints whose endpoints both occur directly in the function type.
        let mut error_row_inclusions = Vec::new();
        let mut selected = BTreeSet::new();
        let mut record_overlays = Vec::new();
        let mut selected_overlays = BTreeSet::new();
        let pending_overlays = self.record_overlays.clone();
        let pending = self.scheme_error_row_inclusions(&vars, outer_free);
        loop {
            let before = selected.len() + selected_overlays.len();
            for (index, overlay) in pending_overlays.iter().enumerate() {
                if selected_overlays.contains(&index) {
                    continue;
                }
                let mut related = BTreeSet::new();
                self.free_vars(&overlay.result, &mut related);
                for operand in &overlay.operands {
                    self.free_vars(operand, &mut related);
                }
                if !related.is_disjoint(&vars) {
                    vars.extend(related);
                    selected_overlays.insert(index);
                    record_overlays.push(RecordOverlay {
                        operands: overlay
                            .operands
                            .iter()
                            .map(|operand| self.prune(operand.clone()))
                            .collect(),
                        result: self.prune(overlay.result.clone()),
                        region: overlay.region,
                    });
                }
            }
            for (index, inclusion) in pending.iter().enumerate() {
                if selected.contains(&index) {
                    continue;
                }
                let mut related = BTreeSet::new();
                self.free_vars(&inclusion.source, &mut related);
                self.free_vars(&inclusion.target, &mut related);
                if !related.is_disjoint(&vars) {
                    vars.extend(related);
                    selected.insert(index);
                    error_row_inclusions.push(ErrorRowInclusion {
                        exact_target: inclusion.exact_target,
                        source: self.prune(inclusion.source.clone()),
                        target: self.prune(inclusion.target.clone()),
                        region: inclusion.region,
                    });
                }
            }
            if before == selected.len() + selected_overlays.len() {
                break;
            }
        }
        if generalizable {
            vars.retain(|variable| !outer_free.contains(variable));
        } else {
            vars.clear();
        }
        self.generalized_variables.extend(vars.iter().copied());
        env.globals.insert(
            name,
            Scheme {
                record_overlays,
                error_row_inclusions,
                quantified: vars.into_iter().collect(),
                predicates,
                projection_eqs,
                typ,
            },
        );
    }

    fn scheme_error_row_inclusions(
        &mut self,
        visible: &BTreeSet<usize>,
        outer_free: &BTreeSet<usize>,
    ) -> Vec<ErrorRowInclusion<'a>> {
        let mut pending = self.error_row_inclusions.clone();
        for inclusion in &mut pending {
            inclusion.source = self.prune(inclusion.source.clone());
            inclusion.target = self.prune(inclusion.target.clone());
        }
        pending.retain(|inclusion| inclusion.exact_target || inclusion.source != inclusion.target);
        let mut protected = visible.union(outer_free).copied().collect::<BTreeSet<_>>();
        // A source tail hidden from the function type can still be selected by
        // an overlay. It is not an unconstrained existential: replacing it by
        // the empty row would disconnect a later selected Result payload from
        // its propagation requirement. Preserve it before scheme selection
        // closes over the connected overlay/error-row component.
        for overlay in self.record_overlays.clone() {
            self.free_vars(&overlay.result, &mut protected);
            for operand in &overlay.operands {
                self.free_vars(operand, &mut protected);
            }
        }
        let universals = self
            .generic_contracts
            .iter()
            .flat_map(|contract| contract.variables.values().cloned())
            .collect::<Vec<_>>();
        for variable in universals {
            self.free_vars(&variable, &mut protected);
        }
        for inclusion in &pending {
            self.free_vars(&inclusion.target, &mut protected);
            if let Ty::ErrorRow { tags, .. } = &inclusion.source {
                for payload in tags.values().flatten() {
                    self.free_vars(payload, &mut protected);
                }
            }
        }
        // A variable occurring only as a hidden source tail is existential:
        // choosing its empty row satisfies every upper bound. Eliminate that
        // variable from this scheme, not from global substitutions. Input tails,
        // payload variables, shared state, universals and intermediate targets
        // must retain their relationships.
        for inclusion in &mut pending {
            if let Ty::ErrorRow { tail, .. } = &mut inclusion.source
                && let Some(value) = tail
                && let Ty::Var(id) = **value
                && !protected.contains(&id)
            {
                *tail = None;
            }
        }
        pending.retain(|inclusion| {
            inclusion.exact_target
                || !matches!(&inclusion.source,
            Ty::ErrorRow { tags, tail: None } if tags.is_empty())
        });
        pending
    }

    fn environment_free_vars(
        &mut self,
        env: &Env<'a>,
        excluded: &BTreeSet<QualifiedName<'a>>,
    ) -> BTreeSet<usize> {
        let schemes = env
            .globals
            .iter()
            .filter(|(name, _)| !excluded.contains(name))
            .map(|(_, scheme)| scheme.clone())
            .chain(env.locals.values().cloned())
            .collect::<Vec<_>>();
        let mut result = BTreeSet::new();
        for scheme in schemes {
            result.extend(self.scheme_free_vars(&scheme));
        }
        result
    }

    fn scheme_free_vars(&mut self, scheme: &Scheme<'a>) -> BTreeSet<usize> {
        let mut free = BTreeSet::new();
        for overlay in &scheme.record_overlays {
            self.free_vars(&overlay.result, &mut free);
            for operand in &overlay.operands {
                self.free_vars(operand, &mut free);
            }
        }
        self.free_vars(&scheme.typ, &mut free);
        for inclusion in &scheme.error_row_inclusions {
            self.free_vars(&inclusion.source, &mut free);
            self.free_vars(&inclusion.target, &mut free);
        }
        for predicate in &scheme.predicates {
            for argument in &predicate.args {
                self.free_vars(argument, &mut free);
            }
        }
        for equation in &scheme.projection_eqs {
            self.free_vars(&equation.projection, &mut free);
            self.free_vars(&equation.typ, &mut free);
        }
        free.retain(|variable| !scheme.quantified.contains(variable));
        free
    }

    fn instantiate(&mut self, scheme: &Scheme<'a>, region: Region) -> Ty<'a> {
        self.instantiate_scheme(scheme, region).0
    }

    fn instantiate_scheme(
        &mut self,
        scheme: &Scheme<'a>,
        region: Region,
    ) -> (Ty<'a>, Vec<Predicate<'a>>) {
        let replacements: BTreeMap<_, _> = scheme
            .quantified
            .iter()
            .map(|id| (*id, self.fresh_with_kind(self.variable_kinds[*id])))
            .collect();
        let typ = self.replace_vars(&scheme.typ, &replacements);
        let overlays = scheme
            .record_overlays
            .iter()
            .map(|overlay| RecordOverlay {
                operands: overlay
                    .operands
                    .iter()
                    .map(|operand| self.replace_vars(operand, &replacements))
                    .collect(),
                result: self.replace_vars(&overlay.result, &replacements),
                region,
            })
            .collect::<Vec<_>>();
        self.record_overlays.extend(overlays);
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
        // A deferred failure belongs to this instance's reference site. The
        // scheme's stored region may refer to a different source module.
        let inclusions = scheme
            .error_row_inclusions
            .iter()
            .map(|inclusion| ErrorRowInclusion {
                exact_target: inclusion.exact_target,
                source: self.replace_vars(&inclusion.source, &replacements),
                target: self.replace_vars(&inclusion.target, &replacements),
                region,
            })
            .collect::<Vec<_>>();
        self.error_row_inclusions.extend(inclusions);
        (typ, predicates)
    }

    fn instantiate_annotation(&mut self, annotation: &'a Annotation<'a>, region: Region) -> Ty<'a> {
        self.instantiate_annotation_with_vars(annotation, region).0
    }

    fn instantiate_annotation_with_vars(
        &mut self,
        annotation: &'a Annotation<'a>,
        region: Region,
    ) -> (Ty<'a>, BTreeMap<&'a str, Ty<'a>>) {
        let mut vars = BTreeMap::new();
        let typ = self.from_ast(annotation.typ, &mut vars);
        for overlay in annotation.record_overlays {
            let operands = overlay
                .operands
                .iter()
                .map(|operand| self.from_ast(operand, &mut vars))
                .collect();
            let result = self.from_ast(overlay.result, &mut vars);
            self.record_overlays.push(RecordOverlay {
                operands,
                result,
                region,
            });
        }
        for inclusion in annotation.error_row_inclusions {
            let source = self.convert_ast_error_type(inclusion.source, &mut vars);
            let target = self.convert_ast_error_type(inclusion.target, &mut vars);
            self.error_row_inclusions.push(ErrorRowInclusion {
                exact_target: inclusion.exact_target,
                source,
                target,
                region,
            });
        }
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
                    let expected = render_ty(&previous.typ, &self.variable_names);
                    let actual = render_ty(&equation.typ, &self.variable_names);
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
                let mut converted = args
                    .iter()
                    .enumerate()
                    .map(|(index, arg)| {
                        if reference.name == "Result" && index == 1 {
                            self.convert_ast_error_type(arg, vars)
                        } else {
                            self.from_ast(arg, vars)
                        }
                    })
                    .collect::<Vec<_>>();
                if reference.name == "Result" && converted.len() == 1 {
                    converted.push(self.fresh_error_row());
                }
                self.apply(Ty::Con(*reference), converted)
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
                match ext {
                    RowExtension::Closed => None,
                    RowExtension::Open(name) => Some(Box::new(
                        vars.entry(name)
                            .or_insert_with(|| self.fresh_with_kind(VariableKind::RecordRow))
                            .clone(),
                    )),
                },
            ),
            Type::ErrorRow { tags, ext } => self.error_row_from_tags(tags, *ext, vars),
            Type::Alias { target, .. } => match target {
                alder_ast::AliasType::Open(real) | alder_ast::AliasType::Filled(real) => {
                    self.from_ast(real, vars)
                }
            },
        }
    }

    fn convert_ast_error_type(
        &mut self,
        typ: &'a Located<Type<'a>>,
        vars: &mut BTreeMap<&'a str, Ty<'a>>,
    ) -> Ty<'a> {
        match &typ.value {
            Type::Var { name, args: [] } => {
                if let Some(existing) = vars.get(name) {
                    if let Ty::Var(id) = existing {
                        self.variable_kinds[*id] = VariableKind::ErrorRow;
                    }
                    return existing.clone();
                }
                let row = self.fresh_with_kind(VariableKind::ErrorRow);
                vars.insert(name, row.clone());
                row
            }
            Type::Named {
                reference,
                args: [],
            } => {
                if let Some(tags) = self.database.error_group(*reference) {
                    self.error_row_from_tags(tags, RowExtension::Closed, vars)
                } else {
                    self.from_ast(typ, vars)
                }
            }
            Type::ErrorRow { tags, ext } => self.error_row_from_tags(tags, *ext, vars),
            Type::Alias { target, .. } => match target {
                alder_ast::AliasType::Open(real) | alder_ast::AliasType::Filled(real) => {
                    self.convert_ast_error_type(real, vars)
                }
            },
            _ => self.from_ast(typ, vars),
        }
    }

    fn error_row_from_tags(
        &mut self,
        tags: &'a [alder_ast::ErrorTagType<'a>],
        ext: RowExtension<'a>,
        vars: &mut BTreeMap<&'a str, Ty<'a>>,
    ) -> Ty<'a> {
        let tags = tags
            .iter()
            .map(|tag| {
                (
                    tag.name,
                    tag.args
                        .iter()
                        .map(|arg| self.from_ast(arg, vars))
                        .collect(),
                )
            })
            .collect();
        let tail = match ext {
            RowExtension::Closed => None,
            RowExtension::Open(name) => {
                let row = if let Some(existing) = vars.get(name) {
                    if let Ty::Var(id) = existing {
                        self.variable_kinds[*id] = VariableKind::ErrorRow;
                    }
                    existing.clone()
                } else {
                    let row = self.fresh_with_kind(VariableKind::ErrorRow);
                    vars.insert(name, row.clone());
                    row
                };
                Some(Box::new(row))
            }
        };
        Ty::ErrorRow { tags, tail }
    }

    fn result_parts(&mut self, typ: Ty<'a>) -> Option<(Ty<'a>, Ty<'a>)> {
        match self.prune(typ) {
            Ty::App(head, args)
                if matches!(*head, Ty::Con(reference) if reference.name == "Result")
                    && args.len() == 2 =>
            {
                let mut args = args.into_iter();
                Some((args.next()?, args.next()?))
            }
            _ => None,
        }
    }

    fn require_result_parts(
        &mut self,
        typ: Ty<'a>,
        region: Region,
    ) -> Result<(Ty<'a>, Ty<'a>), Error> {
        if let Some(parts) = self.result_parts(typ.clone()) {
            return Ok(parts);
        }
        let Ty::Var(id) = self.prune(typ) else {
            return Err(Error {
                region,
                kind: ErrorKind::InvalidTry,
            });
        };
        let value = self.fresh();
        let errors = self.fresh_error_row();
        let result = self.named("Result", vec![value.clone(), errors.clone()]);
        self.bind(id, result, region)?;
        self.inferred_error_rows.push(errors.clone());
        Ok((value, errors))
    }

    fn unify_return(
        &mut self,
        actual: Ty<'a>,
        expected: Ty<'a>,
        region: Region,
    ) -> Result<(), Error> {
        self.value_checks
            .push((actual.clone(), expected.clone(), region));
        let expected_parts = self.result_parts(expected.clone());
        let Some((expected_value, expected_errors)) = expected_parts else {
            return self.unify(actual, expected, region);
        };
        let actual_parts = match self.result_parts(actual.clone()) {
            Some(parts) => parts,
            None => {
                let Ty::Var(id) = self.prune(actual.clone()) else {
                    return self.unify(actual, expected, region);
                };
                // An unknown returned value is another Result source, not an
                // alias for the accumulating output row. Its input errors must
                // stay independent from errors propagated earlier by `?`.
                let errors = self.fresh_error_row();
                let result = self.named("Result", vec![expected_value.clone(), errors.clone()]);
                self.bind(id, result, region)?;
                (expected_value.clone(), errors)
            }
        };
        let (actual_value, actual_errors) = actual_parts;
        self.unify(actual_value, expected_value, region)?;
        self.include_error_rows(actual_errors, expected_errors, region)
    }

    fn include_error_rows(
        &mut self,
        source: Ty<'a>,
        target: Ty<'a>,
        region: Region,
    ) -> Result<(), Error> {
        let source = self.prune_error_row(source);
        let target = self.prune_error_row(target);
        let resolved_target = self.prune(target.clone());
        let exact_target = self
            .inferred_error_rows
            .clone()
            .into_iter()
            .any(|row| self.prune(row) == resolved_target);
        self.error_row_inclusions.push(ErrorRowInclusion {
            exact_target,
            source: source.clone(),
            target: target.clone(),
            region,
        });
        self.propagate_error_row_inclusion(source, target, region)
    }

    fn prune_error_row(&mut self, typ: Ty<'a>) -> Ty<'a> {
        match self.prune(typ) {
            Ty::Var(id) => Ty::ErrorRow {
                tags: BTreeMap::new(),
                tail: Some(Box::new(Ty::Var(id))),
            },
            typ => typ,
        }
    }

    fn propagate_error_row_inclusion(
        &mut self,
        source: Ty<'a>,
        target: Ty<'a>,
        region: Region,
    ) -> Result<(), Error> {
        let source = self.prune_error_row(source);
        let target = self.prune_error_row(target);
        let (
            Ty::ErrorRow {
                tags: mut source_tags,
                tail: _,
            },
            Ty::ErrorRow {
                tags: target_tags,
                tail: target_tail,
            },
        ) = (source.clone(), target.clone())
        else {
            return Err(self.mismatch(region, source, target));
        };

        for (name, target_payloads) in &target_tags {
            let Some(source_payloads) = source_tags.remove(name) else {
                continue;
            };
            if source_payloads.len() != target_payloads.len() {
                return Err(Error {
                    region,
                    kind: ErrorKind::Arity {
                        expected: target_payloads.len(),
                        actual: source_payloads.len(),
                    },
                });
            }
            for (source, target) in source_payloads
                .into_iter()
                .zip(target_payloads.iter().cloned())
            {
                self.unify(source, target, region)?;
            }
        }

        match target_tail {
            None if !source_tags.is_empty() => Err(self.mismatch(region, source, target)),
            None => Ok(()),
            Some(target_tail) if source_tags.is_empty() => Ok(()),
            Some(target_tail) => {
                let open_tail = self.fresh_with_kind(VariableKind::ErrorRow);
                self.unify(
                    *target_tail,
                    Ty::ErrorRow {
                        tags: source_tags,
                        tail: Some(Box::new(open_tail)),
                    },
                    region,
                )
            }
        }
    }

    fn solve_error_row_inclusions(&mut self, env: &Env<'a>) -> Result<(), Error> {
        // Local lambdas do not pass through global scheme generalization. Apply
        // the same existential-source simplification to their constraints, but
        // retain every variable exposed by a module binding or trait obligation.
        let mut visible = BTreeSet::new();
        for scheme in env.globals.values().chain(env.locals.values()) {
            self.free_vars(&scheme.typ, &mut visible);
            for predicate in &scheme.predicates {
                for argument in &predicate.args {
                    self.free_vars(argument, &mut visible);
                }
            }
            for equation in &scheme.projection_eqs {
                self.free_vars(&equation.projection, &mut visible);
                self.free_vars(&equation.typ, &mut visible);
            }
        }
        let obligation_args = self
            .obligations
            .iter()
            .flat_map(|obligation| obligation.predicate.args.iter().cloned())
            .collect::<Vec<_>>();
        for argument in obligation_args {
            self.free_vars(&argument, &mut visible);
        }
        self.error_row_inclusions = self.scheme_error_row_inclusions(&visible, &BTreeSet::new());
        // Calls refine previously unknown input rows. Replay known lower bounds
        // before choosing any flexible tail under a closed upper bound; choosing
        // that tail eagerly would discard errors from another input.
        loop {
            let before = self
                .substitutions
                .iter()
                .filter(|value| value.is_some())
                .count();
            for inclusion in self.error_row_inclusions.clone() {
                self.propagate_error_row_inclusion(
                    inclusion.source,
                    inclusion.target,
                    inclusion.region,
                )?;
            }
            let after = self
                .substitutions
                .iter()
                .filter(|value| value.is_some())
                .count();
            if before != after {
                continue;
            }

            let mut universals = BTreeSet::new();
            let contract_variables = self
                .generic_contracts
                .iter()
                .flat_map(|contract| contract.variables.values().cloned())
                .collect::<Vec<_>>();
            for variable in contract_variables {
                self.free_vars(&variable, &mut universals);
            }
            let mut closed = false;
            let mut exact_targets = BTreeMap::new();
            let mut open_sources = BTreeSet::new();
            let mut dependencies = BTreeMap::<usize, BTreeSet<usize>>::new();
            for inclusion in self.error_row_inclusions.clone() {
                let source = self.prune(inclusion.source.clone());
                let target = self.prune(inclusion.target.clone());
                if let Ty::ErrorRow {
                    tail: Some(target), ..
                } = target
                    && let Ty::Var(target) = *target
                {
                    if inclusion.exact_target {
                        exact_targets.insert(target, inclusion.region);
                    }
                    match source {
                        Ty::ErrorRow { tail: None, .. } => {}
                        Ty::ErrorRow {
                            tail: Some(source), ..
                        } if matches!(*source, Ty::Var(_)) => {
                            let Ty::Var(source) = *source else {
                                unreachable!()
                            };
                            dependencies.entry(target).or_default().insert(source);
                        }
                        _ => {
                            open_sources.insert(target);
                        }
                    }
                }
            }
            // An exact cycle has no unknown errors of its own. Starting with
            // every exact target, remove those depending on an external open
            // source (and their dependents). The remaining closed components
            // can take the least fixed point of the known tags propagated above.
            exact_targets
                .retain(|target, _| !universals.contains(target) && !open_sources.contains(target));
            loop {
                let excluded = exact_targets
                    .keys()
                    .copied()
                    .filter(|target| {
                        dependencies.get(target).is_some_and(|sources| {
                            sources
                                .iter()
                                .any(|source| !exact_targets.contains_key(source))
                        })
                    })
                    .collect::<Vec<_>>();
                if excluded.is_empty() {
                    break;
                }
                for target in excluded {
                    exact_targets.remove(&target);
                }
            }
            for (target, region) in exact_targets {
                self.bind(
                    target,
                    Ty::ErrorRow {
                        tags: BTreeMap::new(),
                        tail: None,
                    },
                    region,
                )?;
                closed = true;
            }
            for inclusion in self.error_row_inclusions.clone() {
                let source = self.prune(inclusion.source.clone());
                let target = self.prune(inclusion.target.clone());
                if let (
                    Ty::ErrorRow {
                        tail: Some(tail), ..
                    },
                    Ty::ErrorRow { tail: None, .. },
                ) = (&source, &target)
                    && let Ty::Var(id) = **tail
                {
                    if universals.contains(&id) {
                        return Err(self.mismatch(inclusion.region, source, target));
                    }
                    self.bind(
                        id,
                        Ty::ErrorRow {
                            tags: BTreeMap::new(),
                            tail: None,
                        },
                        inclusion.region,
                    )?;
                    closed = true;
                }
            }
            if !closed {
                return Ok(());
            }
        }
    }

    fn check_error_matches(&mut self) -> Result<(), Error> {
        let sites = std::mem::take(&mut self.match_sites);
        for site in sites {
            let Some((_, errors)) = self.result_parts(site.scrutinee) else {
                continue;
            };
            let (tags, open) = match self.prune(errors) {
                Ty::ErrorRow { tags, tail } => (tags, tail.is_some()),
                Ty::Var(id) if self.variable_kinds[id] == VariableKind::ErrorRow => {
                    (BTreeMap::new(), true)
                }
                _ => continue,
            };
            let mut coverage = ErrorCoverage::default();
            for arm in site.arms {
                if arm.guard.is_some() {
                    continue;
                }
                for pattern in arm.patterns {
                    collect_error_coverage(pattern, &mut coverage);
                }
            }
            if coverage.all {
                continue;
            }
            let mut missing = Vec::new();
            if !coverage.ok {
                missing.push("Ok".to_owned());
            }
            if !coverage.all_errors {
                missing.extend(
                    tags.keys()
                        .filter(|tag| !coverage.tags.contains(**tag))
                        .map(|tag| format!(":{tag}")),
                );
            }
            let open = open && !coverage.all_errors;
            if !missing.is_empty() || open {
                return Err(Error {
                    region: site.region,
                    kind: ErrorKind::NonExhaustiveErrorMatch { missing, open },
                });
            }
        }
        Ok(())
    }

    fn check_error_tag_placement(&self) -> Result<(), Error> {
        if let Some(region) = self
            .tag_sites
            .iter()
            .find(|region| !self.legal_tag_sites.contains(region))
        {
            return Err(Error {
                region: *region,
                kind: ErrorKind::InvalidErrorTagPlacement,
            });
        }
        Ok(())
    }

    fn check_overlay_contracts(&mut self) -> Result<(), Error> {
        let variables = self
            .generic_contracts
            .iter()
            .flat_map(|contract| {
                contract
                    .variables
                    .iter()
                    .map(|(name, typ)| (*name, typ.clone()))
            })
            .collect::<Vec<_>>();
        let mut universals = BTreeMap::new();
        for (name, variable) in variables {
            if let Ty::Var(id) = self.prune(variable) {
                universals.insert(id, name);
            }
        }
        self.check_universal_overlay_fields(&universals)
    }

    fn check_generic_contracts(&mut self) -> Result<(), Error> {
        let contracts = std::mem::take(&mut self.generic_contracts);
        let mut universals = BTreeMap::new();
        for contract in &contracts {
            for (name, variable) in &contract.variables {
                if let Ty::Var(id) = self.prune(variable.clone()) {
                    universals.insert(id, *name);
                }
            }
        }
        for contract in contracts {
            let mut representatives = BTreeMap::new();
            let mut escaped = BTreeSet::new();
            for outer in contract.outer_free {
                self.free_vars(&Ty::Var(outer), &mut escaped);
            }
            for (name, variable) in contract.variables {
                let resolved = self.prune(variable);
                let Ty::Var(id) = resolved else {
                    return Err(Error {
                        region: contract.region,
                        kind: ErrorKind::GenericSpecialization {
                            variable: name.to_owned(),
                            actual: self.render(resolved),
                        },
                    });
                };
                if let Some(previous) = representatives.insert(id, name) {
                    return Err(Error {
                        region: contract.region,
                        kind: ErrorKind::GenericSpecialization {
                            variable: name.to_owned(),
                            actual: previous.to_owned(),
                        },
                    });
                }
                if escaped.contains(&id) {
                    return Err(Error {
                        region: contract.region,
                        kind: ErrorKind::GenericEscape {
                            variable: name.to_owned(),
                        },
                    });
                }
            }
            self.check_universal_error_row_inclusions(&representatives)?;
        }
        Ok(())
    }

    #[cfg(test)]
    fn assert_shared_overlay_expansion_is_bounded(&mut self) {
        let left = self.open_record(BTreeMap::new());
        let right = self.open_record(BTreeMap::new());
        let mut operands = vec![left.clone(), right.clone()];
        for _ in 0..12 {
            let result = self.open_record(BTreeMap::new());
            self.record_overlays.push(RecordOverlay {
                operands,
                result: result.clone(),
                region: Region::zero(),
            });
            operands = vec![result.clone(), result];
        }
        let mut overlay = RecordOverlay {
            operands,
            result: self.open_record(BTreeMap::new()),
            region: Region::zero(),
        };
        let expanded = self.expanded_overlay_operands(&overlay);
        assert_eq!(
            expanded.len(),
            2,
            "shared producers must not unfold repeatedly"
        );
        assert_eq!(expanded, vec![left.clone(), right.clone()]);
        let middle = Ty::Record(
            BTreeMap::from([("marker", (FieldPresence::Required, Ty::Unit))]),
            None,
        );
        overlay.operands.insert(1, middle.clone());
        assert_eq!(
            self.expanded_overlay_operands(&overlay),
            vec![middle, left, right],
            "retain the rightmost producer relative to intervening writes"
        );
    }

    /// Follow intermediate overlay results before checking a field promise.
    /// Their inferred fields are obligations, not independent evidence that
    /// those fields exist. Keep operand order so a final required write still
    /// masks every earlier possible payload.
    fn expanded_overlay_operands(&mut self, overlay: &RecordOverlay<'a>) -> Vec<Ty<'a>> {
        let overlays = self.record_overlays.clone();
        let mut pending = overlay
            .operands
            .iter()
            .map(|operand| (operand.clone(), BTreeSet::new()))
            .collect::<Vec<_>>();
        let mut visited = BTreeSet::new();
        let mut expanded = Vec::new();
        while let Some((operand, path)) = pending.pop() {
            let operand = self.prune(operand);
            let producer = overlays.iter().enumerate().find(|(index, candidate)| {
                !path.contains(index)
                    && matches!(&operand, Ty::Record(_, Some(_)))
                    && self.prune(candidate.result.clone()) == operand
            });
            if let Some((index, producer)) = producer {
                // Visit rightmost occurrences first. Repeating the same
                // producer cannot add a new possible field payload/presence
                // before its later occurrence, even with intervening writes.
                // Expand each shared producer once instead of unfolding a DAG
                // into an exponentially large tree. Active-path cycles still
                // remain opaque via the producer lookup above.
                if !visited.insert(index) {
                    continue;
                }
                let mut path = path;
                path.insert(index);
                pending.extend(
                    producer
                        .operands
                        .iter()
                        .map(|operand| (operand.clone(), path.clone())),
                );
            } else {
                expanded.push(operand);
            }
        }
        expanded.reverse();
        expanded
    }

    fn check_universal_overlay_fields(
        &mut self,
        universals: &BTreeMap<usize, &'a str>,
    ) -> Result<(), Error> {
        for overlay in self.record_overlays.clone() {
            let operands = self.expanded_overlay_operands(&overlay);
            let Ty::Record(expected_fields, expected_tail) = self.prune(overlay.result) else {
                continue;
            };
            if let Some(expected_tail) = expected_tail
                && let Ty::Var(expected_id) = self.prune(*expected_tail)
                && universals.contains_key(&expected_id)
            {
                for operand in &operands {
                    if let Ty::Record(_, Some(tail)) = self.prune(operand.clone()) {
                        let mut variables = BTreeSet::new();
                        self.free_vars(&tail, &mut variables);
                        if let Some(variable) = variables
                            .iter()
                            .filter(|id| **id != expected_id)
                            .find_map(|id| universals.get(id))
                        {
                            return Err(Error {
                                region: overlay.region,
                                kind: ErrorKind::GenericSpecialization {
                                    variable: (*variable).to_owned(),
                                    actual: format!(
                                        "the declared result row `{}`",
                                        universals[&expected_id]
                                    ),
                                },
                            });
                        }
                    }
                }
            }
            for (name, (expected_presence, expected_type)) in expected_fields {
                let mut payloads = Vec::new();
                let mut required = false;
                let mut known = true;
                for operand in operands.iter().rev() {
                    let Ty::Record(fields, tail) = self.prune(operand.clone()) else {
                        known = false;
                        break;
                    };
                    if let Some((presence, typ)) = fields.get(name) {
                        payloads.push(typ.clone());
                        if *presence == FieldPresence::Required {
                            required = true;
                            break;
                        }
                    } else if let Some(tail) = tail {
                        let mut variables = BTreeSet::new();
                        self.free_vars(&tail, &mut variables);
                        if let Some(variable) = variables.iter().find_map(|id| universals.get(id)) {
                            return Err(Error {
                                region: overlay.region,
                                kind: ErrorKind::GenericSpecialization {
                                    variable: (*variable).to_owned(),
                                    actual: format!(
                                        "a record row with a constrained `{name}` field"
                                    ),
                                },
                            });
                        }
                        known = false;
                        break;
                    }
                }
                if known && !payloads.is_empty() {
                    let mut payloads = payloads.into_iter();
                    let mut typ = payloads.next().expect("nonempty payloads");
                    for other in payloads {
                        typ = self.join_values(typ, other, overlay.region)?;
                    }
                    let actual = Ty::Record(
                        BTreeMap::from([(
                            name,
                            (
                                if required {
                                    FieldPresence::Required
                                } else {
                                    FieldPresence::Optional
                                },
                                typ,
                            ),
                        )]),
                        None,
                    );
                    let expected = Ty::Record(
                        BTreeMap::from([(name, (expected_presence, expected_type))]),
                        None,
                    );
                    self.check_value(actual, expected, overlay.region)?;
                }
            }
        }
        Ok(())
    }

    /// Independent universally quantified tails cannot acquire an inclusion
    /// relationship from the implementation body. Follow intermediate inferred
    /// tails too: `?` can forward an input through more than one result variable.
    fn check_universal_error_row_inclusions(
        &mut self,
        universals: &BTreeMap<usize, &'a str>,
    ) -> Result<(), Error> {
        let mut edges = BTreeMap::<usize, Vec<(usize, ErrorRowInclusion<'a>)>>::new();
        for inclusion in self.error_row_inclusions.clone() {
            let source = self.prune(inclusion.source.clone());
            let target = self.prune(inclusion.target.clone());
            let tail_variable = |row: Ty<'a>| match row {
                Ty::Var(id) => Some(id),
                Ty::ErrorRow {
                    tail: Some(tail), ..
                } => match *tail {
                    Ty::Var(id) => Some(id),
                    _ => None,
                },
                _ => None,
            };
            if let (Some(source), Some(target)) = (tail_variable(source), tail_variable(target)) {
                edges.entry(source).or_default().push((target, inclusion));
            }
        }
        for source in universals.keys().copied() {
            let mut visited = BTreeSet::new();
            let mut pending = vec![source];
            while let Some(current) = pending.pop() {
                if !visited.insert(current) {
                    continue;
                }
                for (target, inclusion) in edges.get(&current).into_iter().flatten() {
                    if *target != source && universals.contains_key(target) {
                        return Err(self.mismatch(
                            inclusion.region,
                            inclusion.source.clone(),
                            inclusion.target.clone(),
                        ));
                    }
                    pending.push(*target);
                }
            }
        }
        Ok(())
    }

    fn join_values(
        &mut self,
        left: Ty<'a>,
        right: Ty<'a>,
        region: Region,
    ) -> Result<Ty<'a>, Error> {
        let left = self.prune(left);
        let right = self.prune(right);
        if let Some((left_value, left_errors)) = self.result_parts(left.clone())
            && let Some((right_value, right_errors)) = self.result_parts(right.clone())
            && matches!(
                self.prune_error_row(left_errors.clone()),
                Ty::ErrorRow { .. }
            )
            && matches!(
                self.prune_error_row(right_errors.clone()),
                Ty::ErrorRow { .. }
            )
        {
            // Result alternatives widen only the error set. Their success
            // payloads can share mutable objects and must remain invariant.
            self.unify(left_value.clone(), right_value, region)?;
            let left_errors = self.prune_error_row(left_errors);
            let right_errors = self.prune_error_row(right_errors);
            let cached =
                self.error_row_joins
                    .clone()
                    .into_iter()
                    .find_map(|(first, second, target)| {
                        let first = self.prune_error_row(first);
                        let second = self.prune_error_row(second);
                        ((first == left_errors && second == right_errors)
                            || (first == right_errors && second == left_errors))
                            .then_some(target)
                    });
            let errors = if left_errors == right_errors {
                left_errors
            } else if let Some(target) = cached {
                target
            } else {
                // Reuse this target on repeated fixed-point visits; allocating
                // a new union each time would manufacture endless progress.
                let target = self.fresh_error_row();
                self.inferred_error_rows.push(target.clone());
                self.include_error_rows(left_errors.clone(), target.clone(), region)?;
                self.include_error_rows(right_errors.clone(), target.clone(), region)?;
                self.error_row_joins
                    .push((left_errors, right_errors, target.clone()));
                target
            };
            let joined = self.named("Result", vec![left_value, errors]);
            self.value_checks.push((left, joined.clone(), region));
            self.value_checks.push((right, joined.clone(), region));
            return Ok(joined);
        }
        self.unify(right.clone(), left.clone(), region)?;
        let joined = match (self.prune(left.clone()), self.prune(right.clone())) {
            (Ty::Record(mut fields, tail), Ty::Record(other, _)) => {
                for (name, (presence, typ)) in other {
                    match fields.get_mut(name) {
                        Some((existing, _)) if presence == FieldPresence::Optional => {
                            *existing = FieldPresence::Optional;
                        }
                        None => {
                            fields.insert(name, (presence, typ));
                        }
                        _ => {}
                    }
                }
                Ty::Record(fields, tail)
            }
            (left, _) => left,
        };
        self.value_checks.push((left, joined.clone(), region));
        self.value_checks.push((right, joined.clone(), region));
        Ok(joined)
    }

    fn infer_checked_expr(
        &mut self,
        env: &Env<'a>,
        expression: &'a Located<Expr<'a>>,
        expected: Ty<'a>,
        return_type: Option<Ty<'a>>,
    ) -> Result<Ty<'a>, Error> {
        if let Expr::Record(fields) = expression.value
            && fields
                .iter()
                .all(|field| matches!(field, RecordField::Field { .. }))
            && let Ty::Record(expected_fields, _) = self.prune(expected.clone())
        {
            let mut actual_fields = BTreeMap::new();
            for field in fields {
                let RecordField::Field { name, value } = field else {
                    unreachable!()
                };
                let typ = if let Some((_, typ)) = expected_fields.get(name.value) {
                    self.infer_checked_expr(env, value, typ.clone(), return_type.clone())?
                } else {
                    self.infer_expr(env, value, return_type.clone())?
                };
                actual_fields.insert(name.value, (FieldPresence::Required, typ));
            }
            self.check_value(
                Ty::Record(actual_fields, None),
                expected.clone(),
                expression.region,
            )?;
            return Ok(self.prune(expected));
        }
        // Fresh arrays have no pre-existing aliases. Check each element against
        // the annotation rather than treating construction as a conversion of
        // an already-shared invariant container.
        if let Expr::Array(items) = expression.value {
            let expected = self.prune(expected);
            if let Ty::App(head, args) = &expected
                && **head == self.named("Array", Vec::new())
                && args.len() == 1
            {
                for item in items {
                    self.infer_checked_expr(env, item, args[0].clone(), return_type.clone())?;
                }
                return Ok(expected);
            }
            let actual = self.infer_expr(env, expression, return_type)?;
            self.check_value(actual, expected.clone(), expression.region)?;
            return Ok(self.prune(expected));
        }
        let actual = self.infer_expr(env, expression, return_type)?;
        self.check_value(actual, expected.clone(), expression.region)?;
        Ok(self.prune(expected))
    }

    fn check_value(
        &mut self,
        actual: Ty<'a>,
        expected: Ty<'a>,
        region: Region,
    ) -> Result<(), Error> {
        self.value_checks
            .push((actual.clone(), expected.clone(), region));
        self.unify(actual, expected, region)
    }

    fn check_field_presence(
        &mut self,
        actual: Ty<'a>,
        expected: Ty<'a>,
        region: Region,
    ) -> Result<(), Error> {
        let actual = self.prune(actual);
        let expected = self.prune(expected);
        match (&actual, &expected) {
            (Ty::Record(actual_fields, _), Ty::Record(expected_fields, _)) => {
                for (name, (presence, expected_type)) in expected_fields {
                    if let Some((actual_presence, actual_type)) = actual_fields.get(name) {
                        if *presence == FieldPresence::Required
                            && *actual_presence == FieldPresence::Optional
                        {
                            return Err(self.mismatch(region, actual.clone(), expected.clone()));
                        }
                        // A nested field can be replaced through a mutable alias.
                        self.check_field_presence(
                            actual_type.clone(),
                            expected_type.clone(),
                            region,
                        )?;
                        self.check_field_presence(
                            expected_type.clone(),
                            actual_type.clone(),
                            region,
                        )?;
                    }
                }
            }
            (Ty::Fn(actual_args, actual_ret), Ty::Fn(expected_args, expected_ret)) => {
                for (actual, expected) in actual_args.iter().zip(expected_args) {
                    self.check_field_presence(expected.clone(), actual.clone(), region)?;
                }
                self.check_field_presence(
                    (**actual_ret).clone(),
                    (**expected_ret).clone(),
                    region,
                )?;
            }
            (Ty::App(_, actual_args), Ty::App(_, expected_args))
            | (Ty::Tuple(actual_args), Ty::Tuple(expected_args)) => {
                for (actual, expected) in actual_args.iter().zip(expected_args) {
                    self.check_field_presence(actual.clone(), expected.clone(), region)?;
                    self.check_field_presence(expected.clone(), actual.clone(), region)?;
                }
            }
            _ => {}
        }
        Ok(())
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
            (Ty::Any, _) | (_, Ty::Any) => Ok(()),
            (Ty::Var(left), Ty::Var(right)) if left == right => Ok(()),
            (Ty::Var(id), typ) | (typ, Ty::Var(id)) => self.bind(id, typ, region),
            (Ty::Unit, Ty::Unit) => Ok(()),
            (Ty::RecordRow(left), Ty::RecordRow(right)) => self.unify(*left, *right, region),
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
            (
                Ty::ErrorRow {
                    tags: left_tags,
                    tail: left_tail,
                },
                Ty::ErrorRow {
                    tags: right_tags,
                    tail: right_tail,
                },
            ) => self.unify_error_rows(left_tags, left_tail, right_tags, right_tail, region),
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
            Ty::RecordRow(row) => Ty::RecordRow(Box::new(self.normalize_type(*row))),
            Ty::Record(fields, open) => Ty::Record(
                fields
                    .into_iter()
                    .map(|(name, (presence, typ))| (name, (presence, self.normalize_type(typ))))
                    .collect(),
                open.map(|tail| Box::new(self.normalize_type(*tail))),
            ),
            Ty::ErrorRow { tags, tail } => Ty::ErrorRow {
                tags: tags
                    .into_iter()
                    .map(|(name, payloads)| {
                        (
                            name,
                            payloads
                                .into_iter()
                                .map(|typ| self.normalize_type(typ))
                                .collect(),
                        )
                    })
                    .collect(),
                tail: tail.map(|tail| Box::new(self.normalize_type(*tail))),
            },
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
        mut left: BTreeMap<&'a str, (FieldPresence, Ty<'a>)>,
        left_open: Option<Box<Ty<'a>>>,
        mut right: BTreeMap<&'a str, (FieldPresence, Ty<'a>)>,
        right_open: Option<Box<Ty<'a>>>,
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
                None if right_open.is_none() && *left_presence == FieldPresence::Required => {
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
            if !left.contains_key(name)
                && left_open.is_none()
                && *presence == FieldPresence::Required
            {
                return Err(Error {
                    region,
                    kind: ErrorKind::MissingField {
                        field: (*name).to_owned(),
                    },
                });
            }
        }
        let common = left
            .keys()
            .filter(|name| right.contains_key(*name))
            .copied()
            .collect::<Vec<_>>();
        for name in common {
            left.remove(name);
            right.remove(name);
        }
        match (left_open, right_open) {
            (None, None) => Ok(()),
            (Some(tail), None) => self.unify(
                *tail,
                Ty::RecordRow(Box::new(Ty::Record(right, None))),
                region,
            ),
            (None, Some(tail)) => self.unify(
                *tail,
                Ty::RecordRow(Box::new(Ty::Record(left, None))),
                region,
            ),
            (Some(left_tail), Some(right_tail)) => {
                let left_tail = self.prune(*left_tail);
                let right_tail = self.prune(*right_tail);
                if left_tail == right_tail {
                    return if left.is_empty() && right.is_empty() {
                        Ok(())
                    } else {
                        Err(Error {
                            region,
                            kind: ErrorKind::InfiniteType,
                        })
                    };
                }
                let shared = self.fresh_with_kind(VariableKind::RecordRow);
                self.unify(
                    left_tail,
                    Ty::RecordRow(Box::new(Ty::Record(right, Some(Box::new(shared.clone()))))),
                    region,
                )?;
                self.unify(
                    right_tail,
                    Ty::RecordRow(Box::new(Ty::Record(left, Some(Box::new(shared))))),
                    region,
                )
            }
        }
    }

    fn unify_error_rows(
        &mut self,
        mut left: BTreeMap<&'a str, Vec<Ty<'a>>>,
        left_tail: Option<Box<Ty<'a>>>,
        mut right: BTreeMap<&'a str, Vec<Ty<'a>>>,
        right_tail: Option<Box<Ty<'a>>>,
        region: Region,
    ) -> Result<(), Error> {
        let actual = Ty::ErrorRow {
            tags: left.clone(),
            tail: left_tail.clone(),
        };
        let expected = Ty::ErrorRow {
            tags: right.clone(),
            tail: right_tail.clone(),
        };
        let common = left
            .keys()
            .filter(|name| right.contains_key(*name))
            .copied()
            .collect::<Vec<_>>();
        for name in common {
            let left_payloads = left.remove(name).expect("common left tag");
            let right_payloads = right.remove(name).expect("common right tag");
            if left_payloads.len() != right_payloads.len() {
                return Err(Error {
                    region,
                    kind: ErrorKind::Arity {
                        expected: right_payloads.len(),
                        actual: left_payloads.len(),
                    },
                });
            }
            for (left, right) in left_payloads.into_iter().zip(right_payloads) {
                self.unify(left, right, region)?;
            }
        }

        match (left_tail, right_tail) {
            (None, None) if left.is_empty() && right.is_empty() => Ok(()),
            (None, None) => Err(self.mismatch(region, actual, expected)),
            (Some(left_tail), None) => {
                if !left.is_empty() {
                    return Err(self.mismatch(region, actual, expected));
                }
                self.unify(
                    *left_tail,
                    Ty::ErrorRow {
                        tags: right,
                        tail: None,
                    },
                    region,
                )
            }
            (None, Some(right_tail)) => {
                if !right.is_empty() {
                    return Err(self.mismatch(region, actual, expected));
                }
                self.unify(
                    *right_tail,
                    Ty::ErrorRow {
                        tags: left,
                        tail: None,
                    },
                    region,
                )
            }
            (Some(left_tail), Some(right_tail)) => {
                let left_tail = self.prune(*left_tail);
                let right_tail = self.prune(*right_tail);
                if left_tail == right_tail {
                    return if left.is_empty() && right.is_empty() {
                        Ok(())
                    } else {
                        Err(self.mismatch(region, actual, expected))
                    };
                }
                let shared = self.fresh_with_kind(VariableKind::ErrorRow);
                // An empty residual is just the shared tail, not a new row
                // structure that specializes a universally quantified variable.
                let left_binding = if right.is_empty() {
                    shared.clone()
                } else {
                    Ty::ErrorRow {
                        tags: right,
                        tail: Some(Box::new(shared.clone())),
                    }
                };
                let right_binding = if left.is_empty() {
                    shared
                } else {
                    Ty::ErrorRow {
                        tags: left,
                        tail: Some(Box::new(shared)),
                    }
                };
                self.unify(left_tail, left_binding, region)?;
                self.unify(right_tail, right_binding, region)
            }
        }
    }

    fn bind(&mut self, id: usize, typ: Ty<'a>, region: Region) -> Result<(), Error> {
        let typ = self.prune(typ);
        let incoming = match &typ {
            Ty::Var(other) => self.variable_kinds[*other],
            Ty::ErrorRow { .. } => VariableKind::ErrorRow,
            Ty::RecordRow(_) => VariableKind::RecordRow,
            Ty::Any => VariableKind::Unknown,
            _ => VariableKind::Type,
        };
        let current = self.variable_kinds[id];
        let merged = match (current, incoming) {
            (VariableKind::Unknown, kind) | (kind, VariableKind::Unknown) => kind,
            (left, right) if left == right => left,
            _ => return Err(self.mismatch(region, Ty::Var(id), typ)),
        };
        self.variable_kinds[id] = merged;
        if let Ty::Var(other) = &typ {
            self.variable_kinds[*other] = merged;
        }
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
            Ty::RecordRow(row) => match self.prune(*row) {
                Ty::Record(fields, Some(tail)) if fields.is_empty() => self.prune(*tail),
                row => Ty::RecordRow(Box::new(row)),
            },
            Ty::Record(mut fields, tail) => {
                let Some(tail) = tail else {
                    return Ty::Record(fields, None);
                };
                match self.prune(*tail) {
                    Ty::RecordRow(row) => match *row {
                        Ty::Record(inherited, tail) => {
                            for (name, field) in inherited {
                                fields.entry(name).or_insert(field);
                            }
                            Ty::Record(fields, tail)
                        }
                        _ => unreachable!("record-row substitutions contain record fragments"),
                    },
                    tail => Ty::Record(fields, Some(Box::new(tail))),
                }
            }
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
            Ty::ErrorRow { mut tags, tail } => {
                for payloads in tags.values_mut() {
                    for payload in payloads {
                        *payload = self.prune(payload.clone());
                    }
                }
                let Some(tail) = tail else {
                    return Ty::ErrorRow { tags, tail: None };
                };
                match self.prune(*tail) {
                    Ty::ErrorRow {
                        tags: inherited,
                        tail,
                    } => {
                        for (name, payloads) in inherited {
                            tags.entry(name).or_insert(payloads);
                        }
                        Ty::ErrorRow { tags, tail }
                    }
                    tail => Ty::ErrorRow {
                        tags,
                        tail: Some(Box::new(tail)),
                    },
                }
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
            Ty::RecordRow(row) => self.occurs(needle, &row),
            Ty::Record(fields, tail) => {
                fields.values().any(|(_, typ)| self.occurs(needle, typ))
                    || tail.is_some_and(|tail| self.occurs(needle, &tail))
            }
            Ty::ErrorRow { tags, tail } => {
                tags.values().flatten().any(|typ| self.occurs(needle, typ))
                    || tail.is_some_and(|tail| self.occurs(needle, &tail))
            }
            Ty::Unit | Ty::Any => false,
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
            Ty::RecordRow(row) => self.free_vars(&row, result),
            Ty::Record(fields, tail) => {
                for (_, typ) in fields.values() {
                    self.free_vars(typ, result);
                }
                if let Some(tail) = tail {
                    self.free_vars(&tail, result);
                }
            }
            Ty::ErrorRow { tags, tail } => {
                for typ in tags.values().flatten() {
                    self.free_vars(typ, result);
                }
                if let Some(tail) = tail {
                    self.free_vars(&tail, result);
                }
            }
            Ty::Unit | Ty::Any => {}
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
            Ty::RecordRow(row) => Ty::RecordRow(Box::new(self.replace_vars(&row, replacements))),
            Ty::Record(fields, open) => Ty::Record(
                fields
                    .iter()
                    .map(|(name, (presence, typ))| {
                        (*name, (*presence, self.replace_vars(typ, replacements)))
                    })
                    .collect(),
                open.map(|tail| Box::new(self.replace_vars(&tail, replacements))),
            ),
            Ty::ErrorRow { tags, tail } => Ty::ErrorRow {
                tags: tags
                    .iter()
                    .map(|(name, payloads)| {
                        (
                            *name,
                            payloads
                                .iter()
                                .map(|typ| self.replace_vars(typ, replacements))
                                .collect(),
                        )
                    })
                    .collect(),
                tail: tail
                    .as_deref()
                    .map(|tail| Box::new(self.replace_vars(tail, replacements))),
            },
            other => other,
        }
    }

    fn annotation(&mut self, scheme: &Scheme<'a>) -> &'a Annotation<'a> {
        let typ = self.prune(scheme.typ.clone());
        let mut arities = BTreeMap::new();
        self.collect_kind_arities(&typ, &mut arities);
        for overlay in &scheme.record_overlays {
            self.collect_kind_arities(&overlay.result, &mut arities);
            for operand in &overlay.operands {
                self.collect_kind_arities(operand, &mut arities);
            }
        }
        for predicate in &scheme.predicates {
            for argument in &predicate.args {
                self.collect_kind_arities(argument, &mut arities);
            }
        }
        for equation in &scheme.projection_eqs {
            self.collect_kind_arities(&equation.projection, &mut arities);
            self.collect_kind_arities(&equation.typ, &mut arities);
        }
        for inclusion in &scheme.error_row_inclusions {
            self.collect_kind_arities(&inclusion.source, &mut arities);
            self.collect_kind_arities(&inclusion.target, &mut arities);
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
        let mut error_row_inclusions = Vec::new();
        for inclusion in &scheme.error_row_inclusions {
            error_row_inclusions.push(alder_ast::ErrorRowInclusion {
                exact_target: inclusion.exact_target,
                source: self.to_ast(&inclusion.source, &mut names),
                target: self.to_ast(&inclusion.target, &mut names),
                region: inclusion.region,
            });
        }
        let record_overlays = self
            .bump
            .alloc_slice_fill_iter(scheme.record_overlays.iter().map(|overlay| {
                alder_ast::RecordOverlay {
                    operands: self.bump.alloc_slice_fill_iter(
                        overlay
                            .operands
                            .iter()
                            .map(|operand| self.to_ast(operand, &mut names)),
                    ),
                    result: self.to_ast(&overlay.result, &mut names),
                    region: overlay.region,
                }
            }));
        let mut params = names.into_iter().collect::<Vec<_>>();
        params.retain(|(id, _)| scheme.quantified.contains(id));
        params.sort_by_key(|(_, name)| generated_type_name_rank(name));
        self.bump.alloc(Annotation {
            record_overlays,
            error_row_inclusions: self.bump.alloc_slice_copy(&error_row_inclusions),
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
            Ty::Con(_) | Ty::Unit | Ty::Any => {}
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
            Ty::RecordRow(row) => self.collect_kind_arities(&row, arities),
            Ty::Record(fields, tail) => {
                for (_, typ) in fields.values() {
                    self.collect_kind_arities(typ, arities);
                }
                if let Some(tail) = tail {
                    self.collect_kind_arities(&tail, arities);
                }
            }
            Ty::ErrorRow { tags, tail } => {
                for typ in tags.values().flatten() {
                    self.collect_kind_arities(typ, arities);
                }
                if let Some(tail) = tail {
                    self.collect_kind_arities(&tail, arities);
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
            Ty::RecordRow(row) => return self.to_ast(&row, names),
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
                ext: match open {
                    Some(tail) => match *tail {
                        Ty::Var(id) => RowExtension::Open(self.type_var_name(id, names)),
                        _ => unreachable!("record tails are pruned before publication"),
                    },
                    None => RowExtension::Closed,
                },
            },
            Ty::ErrorRow { tags, tail } => {
                let tags = self.bump.alloc_slice_fill_iter(tags.iter().enumerate().map(
                    |(index, (name, payloads))| alder_ast::ErrorTagType {
                        index: index as u16,
                        name,
                        args: self.bump.alloc_slice_fill_iter(
                            payloads.iter().map(|payload| self.to_ast(payload, names)),
                        ),
                    },
                ));
                let ext = match tail {
                    None => RowExtension::Closed,
                    Some(tail) => match self.prune(*tail) {
                        Ty::Var(id) => RowExtension::Open(self.type_var_name(id, names)),
                        other => panic!("pruned error-row tail is not a variable: {other:?}"),
                    },
                };
                Type::ErrorRow { tags, ext }
            }
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
                "fn({}) {}",
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
            Ty::RecordRow(row) => self.render(*row),
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
            Ty::ErrorRow { tags, tail } => {
                render_error_row(&tags, tail.as_deref(), |typ| self.render(typ.clone()))
            }
            Ty::Any => "_".to_owned(),
        }
    }
}

fn generalizable_item(item: &ItemKind<'_>) -> bool {
    match item {
        ItemKind::Let(decl) => {
            matches!(
                decl.value.value,
                Expr::Lambda { .. }
                    | Expr::Var { .. }
                    | Expr::Constructor(_)
                    | Expr::Number { .. }
                    | Expr::BigInt(_)
                    | Expr::Str(_)
                    | Expr::Bool(_)
                    | Expr::Unit
            )
        }
        _ => true,
    }
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

#[test]
fn shared_overlay_producers_have_bounded_expansion() {
    let bump = Bump::new();
    let parsed = alder_parse::parse_module(&bump, "").unwrap();
    let canonical = alder_can::canonicalize(
        &bump,
        alder_can::Context {
            home: alder_ast::ModuleId {
                package: alder_ast::PackageId::Application,
                path: &["Main"],
            },
            imports: &[],
            interfaces: &[],
        },
        &parsed,
    )
    .unwrap();
    let database = TraitDatabase::build(&bump, canonical.module, &[]);
    Infer::new(&bump, &database, &[]).assert_shared_overlay_expansion_is_bounded();
}
