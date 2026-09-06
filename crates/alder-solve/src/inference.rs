use std::collections::{BTreeMap, BTreeSet};

use alder_ast::{
    Annotation, BinOp, BindingName, Block, Child, ChildBlock, ChildItem, Expr, ImplId, ItemKind,
    MethodId, Module, ModuleId, PackageId, Pattern, QualifiedName, RecordField, RowExtension, Stmt,
    TraitId, Type, TypeSlot, UseId, ValueRef,
};
use alder_can::Annotations;
use alder_constrain::{
    Constraints, DiagnosticType, Error, ErrorKind, ExpectationKind, RequirementKind,
    RequirementSeed,
};
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
    Record(BTreeMap<&'a str, Ty<'a>>, Option<Box<Ty<'a>>>),
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

fn is_builtin_result(reference: QualifiedName<'_>) -> bool {
    reference.module.package == PackageId::Builtin
        && reference.module.path.is_empty()
        && reference.name == "Result"
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
            option_tries: result.option_tries,
            omitted_arguments: result.omitted_arguments,
            argument_lifts: result.argument_lifts,
            field_lifts: result.field_lifts,
            omitted_record_fields: result.omitted_record_fields,
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
        let mut converter = Infer::new(bump, database, &[]);
        let mut vars = implementation
            .params
            .iter()
            .map(|parameter| (parameter.name.value, converter.fresh()))
            .collect::<BTreeMap<_, _>>();
        let self_predicate =
            converter.predicate_from_trait_ref(implementation.trait_ref, &mut vars);
        let mut givens = implementation
            .trait_predicates
            .iter()
            .enumerate()
            .map(|(index, predicate)| Given {
                predicate: converter.predicate_from_trait_ref(*predicate, &mut vars),
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
                            &mut converter,
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
fn resolve_derived_variant_fields<'a>(
    bump: &'a Bump,
    database: &TraitDatabase<'a>,
    implementation: &'a alder_ast::ImplDecl<'a>,
    self_predicate: &Predicate<'a>,
    givens: &[Given<'a>],
    variant: u16,
    fields: impl Iterator<Item = &'a Located<Type<'a>>>,
    vars: &mut BTreeMap<&'a str, Ty<'a>>,
    converter: &mut Infer<'a, '_>,
    resolved: &mut BTreeMap<DerivedFieldKey<'a>, Evidence<'a>>,
    errors: &mut Vec<SolveError<'a>>,
) {
    for (field, typ) in fields.enumerate() {
        let predicate = Predicate {
            trait_: self_predicate.trait_,
            args: vec![converter.from_ast(typ, vars)],
        };
        if let Some(error) = converter.annotation_error.take() {
            errors.push(SolveError::Core(error));
            continue;
        }
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
    match resolve_structural_capability(bump, database, predicate, givens, origin, stack, step) {
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
            .all(|(template, goal)| match_type(template, goal, &mut bindings, database))
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

fn resolve_structural_capability<'a>(
    bump: &'a Bump,
    database: &TraitDatabase<'a>,
    predicate: &Predicate<'a>,
    givens: &[Given<'a>],
    origin: Region,
    stack: &mut Vec<crate::ObligationFrame<'a>>,
    step: ResolutionStep<'a, '_>,
) -> Result<Option<Evidence<'a>>, SolveTraitError<'a>> {
    let capability = if predicate.trait_ == builtin_trait_id("Show") {
        Some(crate::StructuralErrorCapability::Show)
    } else if predicate.trait_ == builtin_trait_id("Json") {
        Some(crate::StructuralErrorCapability::Json)
    } else if predicate.trait_ == builtin_trait_id("Hash") {
        Some(crate::StructuralErrorCapability::Hash)
    } else {
        None
    };
    if let Some(capability) = capability
        && let Some(Ty::ErrorRow { tags, tail: None }) = predicate.args.first()
    {
        let mut tag_evidence = Vec::with_capacity(tags.len());
        for (name, payloads) in tags {
            let mut fields = Vec::with_capacity(payloads.len());
            for payload in payloads {
                fields.push(resolve_predicate(
                    bump,
                    database,
                    &Predicate {
                        trait_: predicate.trait_,
                        args: vec![payload.clone()],
                    },
                    givens,
                    origin,
                    stack,
                    step,
                )?);
            }
            tag_evidence.push((*name, fields));
        }
        return Ok(Some(Evidence::StructuralError {
            capability,
            tags: tag_evidence,
        }));
    }
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
            fields.values().cloned().collect(),
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
            "Ord" if container == IntrinsicContainer::Option => Some(Intrinsic::OrdOption),
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
        ("Ord", _) if matches!(subject, Ty::Unit) => Intrinsic::OrdUnit,
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
    database: &TraitDatabase<'a>,
) -> bool {
    match &template.value {
        Type::Var { name, args: [] } => match bindings.get(name) {
            Some(bound) => bound == goal,
            None => {
                bindings.insert(name, goal.clone());
                true
            }
        },
        Type::Named {
            reference,
            args: [],
        } if matches!(goal, Ty::ErrorRow { .. }) => {
            database.error_group(*reference).is_some_and(|tags| {
                match_error_row(tags, RowExtension::Closed, goal, bindings, database)
            })
        }
        Type::Named { reference, args } => match nominal_parts(goal) {
            Some((actual, actual_args))
                if actual == *reference && actual_args.len() == args.len() =>
            {
                args.iter()
                    .zip(actual_args)
                    .all(|(template, actual)| match_type(template, actual, bindings, database))
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
                                match_type(left, right, bindings, database)
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
                .all(|(template, actual)| match_type(template, actual, bindings, database)),
            _ => false,
        },
        Type::Alias { target, .. } => match target {
            alder_ast::AliasType::Open(real) | alder_ast::AliasType::Filled(real) => {
                match_type(real, goal, bindings, database)
            }
        },
        Type::ErrorRow { tags, ext } => match_error_row(tags, *ext, goal, bindings, database),
        Type::Record { fields, ext } => {
            let Ty::Record(actual, tail) = goal else {
                return false;
            };
            let mut remaining = actual.clone();
            for field in *fields {
                let Some(typ) = remaining.remove(field.name) else {
                    return false;
                };
                if !match_type(field.typ, &typ, bindings, database) {
                    return false;
                }
            }
            match ext {
                RowExtension::Closed => remaining.is_empty() && tail.is_none(),
                RowExtension::Open(name) => {
                    let residual = Ty::Record(remaining, tail.clone());
                    match bindings.get(name) {
                        Some(bound) => *bound == residual,
                        None => {
                            bindings.insert(name, residual);
                            true
                        }
                    }
                }
            }
        }
        Type::Fn { params, ret } => match goal {
            Ty::Fn(actual_params, actual_ret) => {
                params.len() == actual_params.len()
                    && params
                        .iter()
                        .zip(actual_params)
                        .all(|(template, actual)| match_type(template, actual, bindings, database))
                    && match_type(ret, actual_ret, bindings, database)
            }
            _ => false,
        },
        Type::Var { name, args } => {
            let Ty::App(head, actual_args) = goal else {
                return false;
            };
            if args.len() > actual_args.len() {
                return false;
            }
            let constructor = match head.as_ref() {
                Ty::Con(constructor) => {
                    // Use canonical constructor sections: abstract the leftmost
                    // arguments and retain any remaining fixed slots.
                    let Some(slots) = actual_args
                        .iter()
                        .enumerate()
                        .map(|(index, typ)| {
                            if index < args.len() {
                                u16::try_from(index).ok().map(TySlot::Hole)
                            } else {
                                Some(TySlot::Fixed(typ.clone()))
                            }
                        })
                        .collect::<Option<Vec<_>>>()
                    else {
                        return false;
                    };
                    Ty::Partial(*constructor, slots)
                }
                other if args.len() == actual_args.len() => other.clone(),
                _ => return false,
            };
            match bindings.get(name) {
                Some(bound) if bound != &constructor => return false,
                Some(_) => {}
                None => {
                    bindings.insert(name, constructor);
                }
            }
            args.iter()
                .zip(actual_args)
                .all(|(template, actual)| match_type(template, actual, bindings, database))
        }
        Type::Projection(_) => false,
    }
}

fn match_error_row<'a>(
    tags: &'a [alder_ast::ErrorTagType<'a>],
    ext: RowExtension<'a>,
    goal: &Ty<'a>,
    bindings: &mut BTreeMap<&'a str, Ty<'a>>,
    database: &TraitDatabase<'a>,
) -> bool {
    let Ty::ErrorRow { tags: actual, tail } = goal else {
        return false;
    };
    let mut remaining = actual.clone();
    for tag in tags {
        let Some(payload) = remaining.remove(tag.name) else {
            return false;
        };
        if payload.len() != tag.args.len()
            || !tag
                .args
                .iter()
                .zip(&payload)
                .all(|(template, actual)| match_type(template, actual, bindings, database))
        {
            return false;
        }
    }
    match ext {
        RowExtension::Closed => remaining.is_empty() && tail.is_none(),
        RowExtension::Open(name) => {
            let residual = Ty::ErrorRow {
                tags: remaining,
                tail: tail.clone(),
            };
            match bindings.get(name) {
                Some(bound) => *bound == residual,
                None => {
                    bindings.insert(name, residual);
                    true
                }
            }
        }
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
            for typ in fields.values() {
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
    tuple_shapes: Vec<SparseTupleShape<'a>>,
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
    omitted_record_fields: BTreeMap<Region, Vec<&'a str>>,
    option_tries: BTreeSet<Region>,
    omitted_arguments: BTreeMap<UseId, usize>,
    argument_lifts: BTreeMap<(UseId, usize), usize>,
    field_lifts: BTreeMap<Region, usize>,
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

#[derive(Clone)]
struct TupleProjection<'a> {
    tuple: Ty<'a>,
    index: u32,
    result: Ty<'a>,
    region: Region,
}

#[derive(Clone, Debug)]
struct SparseTupleShape<'a> {
    tuple: Ty<'a>,
    length: u64,
    elements: BTreeMap<u32, Ty<'a>>,
    region: Region,
}

struct TryConstraint<'a> {
    actual: Ty<'a>,
    return_type: Ty<'a>,
    value: Ty<'a>,
    region: Region,
}

struct OptionLift<'a> {
    actual: Ty<'a>,
    expected: Ty<'a>,
    region: Region,
    site: OptionLiftSite,
    expectation: Option<ExpectationKind>,
}

impl OptionLift<'_> {
    fn explain(&self, error: Error) -> Error {
        match &self.expectation {
            Some(expectation) => error.expected_by(expectation.clone(), None),
            None => error,
        }
    }
}

#[derive(Clone, Copy)]
enum OptionLiftSite {
    Argument(UseId, usize),
    Field(Region),
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
        } => {
            reference.module.package == PackageId::Builtin
                && reference.module.path == ["Result"]
                && reference.name == "err"
        }
        _ => false,
    }
}

fn is_option_some_expr(expression: &Located<Expr<'_>>) -> bool {
    match expression.value {
        Expr::Constructor(constructor) => {
            constructor.name.enum_.module.package == PackageId::Builtin
                && constructor.name.enum_.name == "Option"
                && constructor.name.variant == "Some"
        }
        Expr::Var {
            reference: ValueRef::Foreign { reference, .. } | ValueRef::TopLevel(reference),
            ..
        } => {
            reference.module.package == PackageId::Builtin
                && reference.module.path == ["Option"]
                && reference.name == "some"
        }
        _ => false,
    }
}

struct CallInput<'a> {
    region: Region,
    use_id: UseId,
    function: &'a Located<Expr<'a>>,
    arguments: &'a [&'a Located<Expr<'a>>],
    leading: Option<&'a Located<Expr<'a>>>,
    expected_result: Option<Ty<'a>>,
}

#[derive(Clone)]
enum ExprExpectation<'a> {
    Exact(Ty<'a>),
    LiftInput(Ty<'a>),
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
    return_origin: Option<Region>,
    active_scc: BTreeSet<QualifiedName<'a>>,
    loop_results: Vec<Ty<'a>>,
    reachable: bool,
    error_kind_checks: Vec<(Ty<'a>, Region)>,
    expanding_error_groups: BTreeSet<QualifiedName<'a>>,
    annotation_error: Option<Error>,
    tuple_projections: Vec<TupleProjection<'a>>,
    tuple_shapes: Vec<SparseTupleShape<'a>>,
    try_constraints: Vec<TryConstraint<'a>>,
    option_tries: BTreeSet<Region>,
    omitted_arguments: BTreeMap<UseId, usize>,
    argument_lifts: BTreeMap<(UseId, usize), usize>,
    field_lifts: BTreeMap<Region, usize>,
    option_lifts: Vec<OptionLift<'a>>,
    omitted_record_fields: BTreeMap<Region, Vec<&'a str>>,
    record_initializers: Vec<(Ty<'a>, Ty<'a>, Region)>,
}

/// Infer core annotations only, without validating coherence or resolving trait
/// obligations. This lower-level helper is not a complete compilation check;
/// use [`solve`] for the checked contract and dictionary evidence.
pub fn run<'a>(
    bump: &'a Bump,
    constraints: &Constraints<'a>,
) -> Result<Annotations<'a>, Vec<Error>> {
    let database = TraitDatabase::build(bump, constraints.module, &[]);
    let recovered = infer_recovering(bump, &database, constraints);
    if recovered.errors.is_empty() {
        Ok(recovered
            .remainder
            .expect("error-free inference has a result")
            .1
            .annotations)
    } else {
        Err(recovered.errors)
    }
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
    let recovered = infer_recovering(bump, database, constraints);
    let mut errors = recovered
        .errors
        .into_iter()
        .map(SolveError::Core)
        .collect::<Vec<_>>();
    if let Some((module, result)) = recovered.remainder {
        match resolve_obligations(bump, module, database, result) {
            Ok(output) if errors.is_empty() => return Ok(output),
            Ok(_) => {}
            Err(trait_errors) => errors.extend(trait_errors),
        }
    }
    Err(errors)
}

/// A cleanly re-inferred remainder is useful for discovering independent trait
/// failures, but is never publishable when an earlier attempt reported errors.
struct RecoveredInference<'a> {
    errors: Vec<Error>,
    remainder: Option<(&'a Module<'a>, InferenceResult<'a>)>,
}

/// Each failed attempt is discarded in its entirety. In particular, neither
/// partial substitutions nor deferred constraints/evidence survive a failure.
/// Removed bindings taint all transitive users (including writes and recursive
/// peers). Rechecking is only for diagnostics: once any attempt failed, even a
/// successful remainder must never escape as a checked module/interface.
fn infer_recovering<'a>(
    bump: &'a Bump,
    database: &TraitDatabase<'a>,
    constraints: &Constraints<'a>,
) -> RecoveredInference<'a> {
    let original = constraints.module;
    let mut module = original;
    let mut errors = Vec::new();
    let mut excluded = BTreeSet::new();
    loop {
        match Infer::new(bump, database, constraints.requirement_seeds).infer_module(module) {
            Ok(result) => {
                errors.sort_by_key(|error: &Error| error.region);
                return RecoveredInference {
                    errors,
                    remainder: Some((module, result)),
                };
            }
            Err(error) => {
                // An unknown/external origin cannot safely identify a recovery
                // unit. Report it and stop instead of guessing a declaration.
                let owner = original.items.iter().position(|item| {
                    !excluded.contains(&item.region) && item.region.contains(&error.region)
                });
                errors.push(error);
                let Some(owner) = owner else { break };
                // Nominal declarations also live in the frozen trait/type
                // database. Removing their AST item alone would leave invalid
                // metadata available to retries and can duplicate cycle errors.
                // Only executable declarations are recovery units here.
                if !is_value_item(&original.items[owner].value.kind)
                    && !matches!(
                        original.items[owner].value.kind,
                        ItemKind::Test(_) | ItemKind::Tests(_)
                    )
                {
                    break;
                }
                excluded.insert(original.items[owner].region);
            }
        }

        loop {
            let invalid_names: BTreeSet<_> = original
                .items
                .iter()
                .filter(|item| excluded.contains(&item.region))
                .flat_map(|item| value_names(&item.value.kind))
                .collect();
            let before = excluded.len();
            for item in original.items {
                if alder_can::value_dependencies(original.id, &item.value.kind)
                    .iter()
                    .any(|name| invalid_names.contains(name))
                {
                    excluded.insert(item.region);
                }
            }
            if before == excluded.len() {
                break;
            }
        }

        let items = bump.alloc_slice_fill_iter(
            original
                .items
                .iter()
                .copied()
                .filter(|item| !excluded.contains(&item.region))
                .collect::<Vec<_>>(),
        );
        let surviving_names: BTreeSet<_> = items
            .iter()
            .flat_map(|item| value_names(&item.value.kind))
            .collect();
        let value_sccs = bump.alloc_slice_fill_iter(
            original
                .value_sccs
                .iter()
                .copied()
                .filter(|group| {
                    group
                        .members
                        .iter()
                        .all(|name| surviving_names.contains(name.name))
                })
                .collect::<Vec<_>>(),
        );
        module = bump.alloc(Module {
            id: original.id,
            imports: original.imports,
            items,
            value_sccs,
            assigned_bindings: original.assigned_bindings,
        });
    }
    errors.sort_by_key(|error| error.region);
    RecoveredInference {
        errors,
        remainder: None,
    }
}

fn value_names<'a>(item: &ItemKind<'a>) -> Vec<&'a str> {
    match item {
        ItemKind::Fn(function) => vec![function.name.name],
        ItemKind::Let(declaration) => declaration.bindings.iter().map(|name| name.name).collect(),
        ItemKind::Component(component) => vec![component.name.name],
        ItemKind::Extern(alder_ast::ExternDecl::Fn { name, .. }) => vec![name.name],
        _ => Vec::new(),
    }
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
            tuple_shapes: Vec::new(),
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
            return_origin: None,
            active_scc: BTreeSet::new(),
            loop_results: Vec::new(),
            reachable: true,
            error_kind_checks: Vec::new(),
            expanding_error_groups: BTreeSet::new(),
            annotation_error: None,
            tuple_projections: Vec::new(),
            try_constraints: Vec::new(),
            option_tries: BTreeSet::new(),
            omitted_arguments: BTreeMap::new(),
            argument_lifts: BTreeMap::new(),
            field_lifts: BTreeMap::new(),
            omitted_record_fields: BTreeMap::new(),
            option_lifts: Vec::new(),
            record_initializers: Vec::new(),
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

    fn open_record(&mut self, fields: BTreeMap<&'a str, Ty<'a>>) -> Ty<'a> {
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
            // Overlay result equalities can allocate fresh residual row tails.
            // Solve them before quantifying this SCC, so those tails belong to
            // its schemes rather than appearing as late shared export state.
            self.solve_record_overlays()?;
            self.solve_try_constraints()?;
            self.solve_tuple_projections()?;
            self.solve_tuple_shapes()?;
            self.solve_option_lifts()?;
            self.solve_record_initializers()?;
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

        self.solve_option_lifts()?;
        self.solve_record_initializers()?;
        self.solve_try_constraints()?;
        self.solve_tuple_projections()?;
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
            self.solve_tuple_shapes()?;
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
        if let Some(error) = self.annotation_error.take() {
            return Err(error);
        }
        for (typ, region) in std::mem::take(&mut self.error_kind_checks) {
            let typ = self.prune(typ);
            let valid = match &typ {
                Ty::ErrorRow { .. } => true,
                Ty::Var(id) => self.variable_kinds[*id] == VariableKind::ErrorRow,
                _ => false,
            };
            if !valid {
                return Err(Error {
                    expectation: None,
                    region,
                    kind: ErrorKind::InvalidResultErrorType {
                        actual: self.render(typ),
                    },
                });
            }
        }
        // Contract rigidity is final: no subsequent pass may introduce type
        // equalities or unsolved error unions after these promises are checked.
        self.check_generic_contracts()?;
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
                    expectation: None,
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
        Ok(InferenceResult {
            option_tries: std::mem::take(&mut self.option_tries),
            omitted_arguments: std::mem::take(&mut self.omitted_arguments),
            argument_lifts: std::mem::take(&mut self.argument_lifts),
            field_lifts: std::mem::take(&mut self.field_lifts),
            omitted_record_fields: std::mem::take(&mut self.omitted_record_fields),
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
                tuple_shapes: Vec::new(),
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
                let mut binding_vars = BTreeMap::new();
                for binding in impl_.assoc_bindings {
                    self.from_ast(binding.typ, &mut binding_vars);
                }
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
                        && function.body.is_none()
                    {
                        self.from_ast(function.scheme.typ, &mut BTreeMap::new());
                    }
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
            ItemKind::TypeAlias(alias) => {
                // Validate declarations even when no expression instantiates
                // them: exported aliases must not contain invalid error kinds.
                self.from_ast(alias.typ, &mut BTreeMap::new());
            }
            ItemKind::Enum(declaration) => {
                let mut vars = BTreeMap::new();
                for variant in declaration.variants {
                    match variant.payload {
                        alder_ast::VariantPayload::Unit => {}
                        alder_ast::VariantPayload::Tuple(types) => {
                            for typ in types {
                                self.from_ast(typ, &mut vars);
                            }
                        }
                        alder_ast::VariantPayload::Record(fields) => {
                            for field in fields {
                                self.from_ast(field.typ, &mut vars);
                            }
                        }
                    }
                }
            }
            ItemKind::ErrorGroup(group) => {
                let mut vars = BTreeMap::new();
                for tag in group.tags {
                    for typ in tag.args {
                        self.from_ast(typ, &mut vars);
                    }
                }
            }
            ItemKind::Table(_)
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
                    self.infer_checked_expr(env, decl.value, annotated, None)
                        .map_err(|error| {
                            error.expected_by(ExpectationKind::Annotation, Some(annotation.region))
                        })?
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
        let outer_return_origin =
            std::mem::replace(&mut self.return_origin, ret.map(|ret| ret.region));
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
            let field_context = if body.value.tail.is_some()
                && matches!(self.prune(body_result.clone()), Ty::Record(..))
            {
                Some(body_result.clone())
            } else {
                None
            };
            let body_type = self.infer_block_with_expected(
                &mut local,
                body,
                Some(body_result.clone()),
                field_context,
            )?;
            self.resolve_try_boundary(body_result.clone(), &body_type, region)?;
            if alder_ast::flow::block(body).falls_through {
                let expected = self.render(body_result.clone());
                self.unify_return(
                    body_type,
                    body_result,
                    body.value.tail.map_or(body.region, |tail| tail.region),
                )
                .map_err(|error| {
                    if body.value.tail.is_none() {
                        Error {
                            expectation: None,
                            region: body.region,
                            kind: ErrorKind::MissingReturn { expected },
                        }
                    } else {
                        error.expected_by(ExpectationKind::Return, ret.map(|ret| ret.region))
                    }
                })?;
            }
            let function_type = Ty::Fn(args, Box::new(self.prune(result)));
            Ok((function_type, predicates, local_projection_equations))
        })();
        self.givens = outer_givens;
        self.projection_equations = outer_projection_equations;
        self.annotation_scope = outer_annotation_scope;
        self.return_origin = outer_return_origin;
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
        self.infer_block_with_expected(env, block, return_type, None)
    }

    fn infer_block_with_expected(
        &mut self,
        env: &mut Env<'a>,
        block: &'a Located<Block<'a>>,
        return_type: Option<Ty<'a>>,
        expected: Option<Ty<'a>>,
    ) -> Result<Ty<'a>, Error> {
        self.infer_block_context(
            env,
            block,
            return_type,
            expected.map(ExprExpectation::Exact),
        )
    }

    fn infer_block_context(
        &mut self,
        env: &mut Env<'a>,
        block: &'a Located<Block<'a>>,
        return_type: Option<Ty<'a>>,
        expected: Option<ExprExpectation<'a>>,
    ) -> Result<Ty<'a>, Error> {
        self.with_reachability(true, |this| {
            for statement in block.value.statements {
                this.infer_stmt(env, statement, return_type.clone())?;
                this.reachable &= alder_ast::flow::statement(statement).falls_through;
            }
            let falls_through = alder_ast::flow::block(block).falls_through;
            let result = match (block.value.tail, expected) {
                (Some(tail), Some(expected)) if falls_through => {
                    this.infer_expr_context(env, tail, return_type, Some(expected))
                }
                (Some(tail), _) => this.infer_expr(env, tail, return_type),
                (None, Some(ExprExpectation::Exact(expected))) if falls_through => {
                    this.check_value(Ty::Unit, expected.clone(), block.region)?;
                    Ok(this.prune(expected))
                }
                (None, _) => Ok(Ty::Unit),
            }?;
            if falls_through {
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
                    self.infer_checked_expr(env, decl.value, annotated, return_type.clone())
                        .map_err(|error| {
                            error.expected_by(ExpectationKind::Annotation, Some(annotation.region))
                        })?
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
                let target_continues = place.steps.iter().all(|step| match step {
                    alder_ast::PlaceStep::Index(index) => {
                        alder_ast::flow::expression(index).falls_through
                    }
                    _ => true,
                });
                let actual = self.with_reachability(target_continues, |this| {
                    this.infer_expr(env, value, return_type.clone())
                })?;
                self.check_value(actual, expected.clone(), value.region)
                    .map_err(|error| error.expected_by(ExpectationKind::Assignment, None))?;
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
                )
                .map_err(|error| error.expected_by(ExpectationKind::Condition, None))?;
                self.with_reachability(
                    alder_ast::flow::expression(condition).falls_through
                        && !matches!(condition.value, Expr::Bool(false)),
                    |this| this.infer_loop_body(&mut env.clone(), body, return_type, Ty::Unit),
                )?;
            }
            Stmt::Return(value) => {
                let expected = return_type.unwrap_or(Ty::Unit);
                let actual = match value {
                    Some(value) if matches!(self.prune(expected.clone()), Ty::Record(..)) => self
                        .infer_checked_expr(
                        env,
                        value,
                        expected.clone(),
                        Some(expected.clone()),
                    )?,
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
                self.unify_return(
                    actual,
                    expected,
                    value.map_or(statement.region, |value| value.region),
                )
                .map_err(|error| error.expected_by(ExpectationKind::Return, self.return_origin))?;
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
                self.unify(actual, self.named("Bool", Vec::new()), expr.region)
                    .map_err(|error| error.expected_by(ExpectationKind::Condition, None))?;
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
                let mut reachable = true;
                for part in *parts {
                    if let alder_ast::TemplatePart::Expr(expr) = part {
                        self.with_reachability(reachable, |this| {
                            this.infer_expr(env, expr, return_type.clone())
                        })?;
                        reachable &= alder_ast::flow::expression(expr).falls_through;
                    }
                }
                Ok(self.named("String", Vec::new()))
            }
            Expr::TaggedTemplate { tag, parts } => {
                let function_type = self.infer_expr(env, tag, return_type.clone())?;
                let strings = self.named("Array", vec![self.named("String", Vec::new())]);
                let mut args = vec![strings];
                let mut reachable = alder_ast::flow::expression(tag).falls_through;
                for part in *parts {
                    if let alder_ast::TemplatePart::Expr(argument) = part {
                        let expected = match self.prune(function_type.clone()) {
                            Ty::Fn(params, _) => params.get(args.len()).cloned(),
                            _ => None,
                        };
                        args.push(self.with_reachability(reachable, |this| {
                            if let Some(expected) = expected
                                && matches!(argument.value, Expr::Record(_) | Expr::Array(_))
                            {
                                this.infer_checked_expr(
                                    env,
                                    argument,
                                    expected,
                                    return_type.clone(),
                                )
                            } else {
                                this.infer_expr(env, argument, return_type.clone())
                            }
                        })?);
                        reachable &= alder_ast::flow::expression(argument).falls_through;
                    }
                }
                let result = self.fresh();
                let call_type = Ty::Fn(args, Box::new(result.clone()));
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
                let mut reachable = true;
                for arg in *args {
                    payloads.push(self.with_reachability(reachable, |this| {
                        this.infer_expr(env, arg, return_type.clone())
                    })?);
                    reachable &= alder_ast::flow::expression(arg).falls_through;
                }
                Ok(Ty::ErrorRow {
                    tags: BTreeMap::from([(name.value, payloads)]),
                    tail: None,
                })
            }
            Expr::Array(items) => {
                let item_type = self.fresh();
                let mut reachable = true;
                for (index, item) in items.iter().enumerate() {
                    let actual = self.with_reachability(reachable, |this| {
                        this.infer_expr(env, item, return_type.clone())
                    })?;
                    reachable &= alder_ast::flow::expression(item).falls_through;
                    self.unify(actual, item_type.clone(), item.region)
                        .map_err(|error| {
                            error.expected_by(
                                ExpectationKind::ArrayElement {
                                    position: index + 1,
                                },
                                items
                                    .first()
                                    .filter(|first| first.region != item.region)
                                    .map(|first| first.region),
                            )
                        })?;
                }
                let item_type = self.prune(item_type);
                Ok(self.named("Array", vec![item_type]))
            }
            Expr::Tuple(items) => {
                let mut types = Vec::with_capacity(items.len());
                let mut reachable = true;
                for item in *items {
                    types.push(self.with_reachability(reachable, |this| {
                        this.infer_expr(env, item, return_type.clone())
                    })?);
                    reachable &= alder_ast::flow::expression(item).falls_through;
                }
                Ok(Ty::Tuple(types))
            }
            Expr::Record(fields) => self.infer_record(env, fields, return_type),
            Expr::RecordConstructor {
                constructor,
                fields,
            } => {
                let constructor_type = self.instantiate_annotation(constructor.annotation, region);
                let alder_ast::VariantPayload::Record(expected_fields) = constructor.payload else {
                    unreachable!("record constructor carries a record payload")
                };
                match constructor_type {
                    Ty::Fn(expected_types, result)
                        if expected_types.len() == expected_fields.len() =>
                    {
                        let expected = Ty::Record(
                            expected_fields
                                .iter()
                                .zip(expected_types)
                                .map(|(field, typ)| (field.name, typ))
                                .collect(),
                            None,
                        );
                        let record = self.bump.alloc(Located {
                            region,
                            value: Expr::Record(fields),
                        });
                        self.infer_checked_expr(env, record, expected, return_type)?;
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
                    expected_result: None,
                },
                return_type,
            ),
            Expr::Access { record, field } => {
                let record_type = self.infer_expr(env, record, return_type)?;
                self.access_field(record_type, field.value, field.region)
            }
            Expr::TupleAccess { tuple, index } => {
                let tuple_type = self.infer_expr(env, tuple, return_type)?;
                self.project_tuple(tuple_type, index.value, index.region)
            }
            Expr::Index { target, index } => {
                let item = self.fresh();
                let target_type = self.infer_expr(env, target, return_type.clone())?;
                self.unify(
                    target_type,
                    self.named("Array", vec![item.clone()]),
                    target.region,
                )?;
                let index_type = self.with_reachability(
                    alder_ast::flow::expression(target).falls_through,
                    |this| this.infer_expr(env, index, return_type),
                )?;
                self.unify(index_type, self.named("Number", Vec::new()), index.region)?;
                Ok(self.prune(item))
            }
            Expr::Await(expr) => {
                let actual = self.infer_expr(env, expr, return_type)?;
                self.infer_await_type(actual, region)
                    .map_err(|error| error.expected_by(ExpectationKind::Await, None))
            }
            Expr::Try(expr) => {
                let actual = self.infer_expr(env, expr, return_type.clone())?;
                self.infer_try_type(actual, return_type, region)
                    .map_err(|error| error.expected_by(ExpectationKind::Propagation, None))
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
                let outer_return_origin = self.return_origin.take();
                let outer_loops = std::mem::take(&mut self.loop_results);
                let outer_reachable = std::mem::replace(&mut self.reachable, true);
                let body_type = self.infer_block(&mut env.clone(), block, Some(result.clone()));
                self.reachable = outer_reachable;
                self.loop_results = outer_loops;
                self.return_origin = outer_return_origin;
                let body_type = body_type?;
                self.resolve_try_boundary(result.clone(), &body_type, region)?;
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
                let outer_return_origin =
                    std::mem::replace(&mut self.return_origin, ret.map(|ret| ret.region));
                let outer_loops = std::mem::take(&mut self.loop_results);
                let outer_reachable = std::mem::replace(&mut self.reachable, true);
                let body_type = if matches!(self.prune(body_result.clone()), Ty::Record(..)) {
                    self.infer_checked_expr(
                        &local,
                        body,
                        body_result.clone(),
                        Some(body_result.clone()),
                    )
                } else {
                    self.infer_expr(&local, body, Some(body_result.clone()))
                };
                self.reachable = outer_reachable;
                self.loop_results = outer_loops;
                self.annotation_scope = outer_annotation_scope;
                self.return_origin = outer_return_origin;
                let body_type = body_type?;
                self.resolve_try_boundary(body_result.clone(), &body_type, region)?;
                if alder_ast::flow::expression(body).falls_through {
                    self.unify_return(body_type, body_result, body.region)
                        .map_err(|error| {
                            error.expected_by(ExpectationKind::Return, ret.map(|ret| ret.region))
                        })?;
                }
                Ok(Ty::Fn(args, Box::new(self.prune(result))))
            }
            Expr::If { .. } | Expr::Match { .. } => {
                self.infer_branch_context(env, expression, return_type, None)
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
        self.infer_record_fields(env, fields, return_type, false)
    }

    fn infer_record_fields(
        &mut self,
        env: &Env<'a>,
        fields: &'a [RecordField<'a>],
        return_type: Option<Ty<'a>>,
        contextual: bool,
    ) -> Result<Ty<'a>, Error> {
        let mut operands = Vec::new();
        let mut reachable = true;
        for field in fields {
            let (typ, region) = match field {
                RecordField::Field { name, value } => {
                    let typ = if contextual {
                        // The ordered merge determines whether this field survives.
                        // Do not bind a discarded initializer to the final annotation.
                        let context = self.fresh();
                        let actual = self.with_reachability(reachable, |this| {
                            this.infer_lift_input(env, value, context, return_type.clone())
                        })?;
                        let expected = self.fresh();
                        self.option_lifts.push(OptionLift {
                            actual,
                            expected: expected.clone(),
                            region: value.region,
                            site: OptionLiftSite::Field(name.region),
                            expectation: None,
                        });
                        expected
                    } else {
                        self.with_reachability(reachable, |this| {
                            this.infer_expr(env, value, return_type.clone())
                        })?
                    };
                    (
                        Ty::Record(BTreeMap::from([(name.value, typ)]), None),
                        value.region,
                    )
                }
                RecordField::Spread(expr) => {
                    let typ = self.with_reachability(reachable, |this| {
                        this.infer_expr(env, expr, return_type.clone())
                    })?;
                    let expected = self.open_record(BTreeMap::new());
                    self.unify(typ.clone(), expected, expr.region)?;
                    (typ, expr.region)
                }
            };
            let expression = match field {
                RecordField::Field { value, .. } | RecordField::Spread(value) => value,
            };
            reachable &= alder_ast::flow::expression(expression).falls_through;
            operands.push((typ, region));
        }
        let open_count = operands
            .iter()
            .filter(|(typ, _)| matches!(self.prune(typ.clone()), Ty::Record(_, Some(_))))
            .count();
        if open_count > 0 {
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
        Ok(self.merge_record_operands(operands))
    }

    fn merge_record_operands(&mut self, operands: Vec<(Ty<'a>, Region)>) -> Ty<'a> {
        let mut result = BTreeMap::new();
        for (operand, _) in operands {
            let Ty::Record(fields, None) = self.prune(operand) else {
                unreachable!("open record operands retain an explicit overlay constraint");
            };
            // Every stored field exists. A later Option field overwrites an
            // earlier field even when its runtime value is None.
            result.extend(fields);
        }
        Ty::Record(result, None)
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
                    let (mut left_operands, left_cyclic) =
                        self.expanded_overlay_operands_from(&pending[left], &pending);
                    let (mut right_operands, right_cyclic) =
                        self.expanded_overlay_operands_from(&pending[right], &pending);
                    // Associativity identifies acyclic compositions, not
                    // arbitrary fixed points of recursive overlay equations.
                    if left_cyclic || right_cyclic {
                        left_operands = pending[left]
                            .operands
                            .iter()
                            .map(|operand| self.prune(operand.clone()))
                            .collect::<Vec<_>>();
                        right_operands = pending[right]
                            .operands
                            .iter()
                            .map(|operand| self.prune(operand.clone()))
                            .collect::<Vec<_>>();
                    }
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
                    );
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
            let mut selected = None;
            for operand in operands.iter().rev() {
                let Ty::Record(fields, tail) = operand else {
                    break;
                };
                if let Some(typ) = fields.get(name) {
                    selected = Some(typ.clone());
                    break;
                } else if tail.is_some() {
                    break;
                }
            }
            let Some(typ) = selected else {
                continue;
            };
            if let Some(expected) = existing.get(name) {
                self.check_value(
                    Ty::Record(BTreeMap::from([(name, typ)]), None),
                    Ty::Record(BTreeMap::from([(name, expected.clone())]), None),
                    overlay.region,
                )?;
            } else {
                additions.insert(name, typ);
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
        self.infer_pattern_with_return(env, pattern, expected, top_level, None)
    }

    /// Sibling patterns are tested only after every earlier sibling matched.
    /// Keep ordinary type checks even when a previous pin has already exited.
    fn infer_pattern_child(
        &mut self,
        env: &mut Env<'a>,
        pattern: &'a Located<Pattern<'a>>,
        expected: Ty<'a>,
        return_type: Option<Ty<'a>>,
        reachable: &mut bool,
    ) -> Result<(), Error> {
        self.with_reachability(*reachable, |this| {
            this.infer_pattern_with_return(env, pattern, expected, false, return_type)
        })?;
        *reachable &= alder_ast::flow::pattern(pattern).matches;
        Ok(())
    }

    fn infer_pattern_with_return(
        &mut self,
        env: &mut Env<'a>,
        pattern: &'a Located<Pattern<'a>>,
        expected: Ty<'a>,
        top_level: bool,
        return_type: Option<Ty<'a>>,
    ) -> Result<(), Error> {
        self.infer_pattern_inner(env, pattern, expected, top_level, return_type)
            .map_err(|error| error.expected_by(ExpectationKind::Pattern, None))
    }

    fn infer_pattern_inner(
        &mut self,
        env: &mut Env<'a>,
        pattern: &'a Located<Pattern<'a>>,
        expected: Ty<'a>,
        top_level: bool,
        return_type: Option<Ty<'a>>,
    ) -> Result<(), Error> {
        let mut reachable = true;
        match &pattern.value {
            Pattern::Anything => {}
            Pattern::Bind(binding) => match binding {
                BindingName::Local(local) => {
                    if let Some(existing) = env.locals.get(&local.id.0) {
                        return self.unify(existing.typ.clone(), expected, pattern.region);
                    }
                    env.locals.insert(
                        local.id.0,
                        Scheme {
                            record_overlays: Vec::new(),
                            tuple_shapes: Vec::new(),
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
                let actual = self.infer_expr(env, expr, return_type)?;
                self.unify(actual, expected.clone(), pattern.region)?;
                self.record_builtin_obligation(
                    *use_id,
                    expected,
                    pattern.region,
                    ObligationAction::Pin,
                );
            }
            Pattern::Number { .. } => {
                self.unify(self.named("Number", Vec::new()), expected, pattern.region)?;
            }
            Pattern::BigInt(_) => {
                self.unify(self.named("BigInt", Vec::new()), expected, pattern.region)?;
            }
            Pattern::Str(_) => {
                self.unify(self.named("String", Vec::new()), expected, pattern.region)?;
            }
            Pattern::Bool(_) => {
                self.unify(self.named("Bool", Vec::new()), expected, pattern.region)?;
            }
            Pattern::Unit => self.unify(Ty::Unit, expected, pattern.region)?,
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
                        self.infer_pattern_child(
                            env,
                            arg,
                            typ,
                            return_type.clone(),
                            &mut reachable,
                        )?;
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
                        .map(|(field, typ)| (field.name, typ))
                        .collect(),
                    None,
                );
                for field in *fields {
                    let typ =
                        self.access_field(record.clone(), field.name.value, field.name.region)?;
                    self.infer_pattern_child(
                        env,
                        field.pattern,
                        typ,
                        return_type.clone(),
                        &mut reachable,
                    )?;
                }
            }
            Pattern::Record { fields, .. } => {
                let record = self.open_record(BTreeMap::new());
                self.unify(expected.clone(), record, pattern.region)?;
                for field in *fields {
                    let typ =
                        self.access_field(expected.clone(), field.name.value, field.name.region)?;
                    self.infer_pattern_child(
                        env,
                        field.pattern,
                        typ,
                        return_type.clone(),
                        &mut reachable,
                    )?;
                }
            }
            Pattern::Tag { name, args, .. } => {
                if let Ty::ErrorRow { tags, tail: None } = self.prune(expected.clone()) {
                    let Some(payloads) = tags.get(name.value) else {
                        return Err(Error {
                            expectation: None,
                            region: pattern.region,
                            kind: ErrorKind::ImpossibleErrorPattern {
                                tag: name.value.to_owned(),
                            },
                        });
                    };
                    if payloads.len() != args.len() {
                        return Err(Error {
                            expectation: None,
                            region: pattern.region,
                            kind: ErrorKind::Arity {
                                expected: payloads.len(),
                                actual: args.len(),
                            },
                        });
                    }
                    for (arg, typ) in args.iter().zip(payloads.iter().cloned()) {
                        self.infer_pattern_child(
                            env,
                            arg,
                            typ,
                            return_type.clone(),
                            &mut reachable,
                        )?;
                    }
                    return Ok(());
                }
                let mut payloads = Vec::with_capacity(args.len());
                for arg in *args {
                    let typ = self.fresh();
                    self.infer_pattern_child(
                        env,
                        arg,
                        typ.clone(),
                        return_type.clone(),
                        &mut reachable,
                    )?;
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
                    self.infer_pattern_child(
                        env,
                        item,
                        typ.clone(),
                        return_type.clone(),
                        &mut reachable,
                    )?;
                    types.push(typ);
                }
                self.unify(expected, Ty::Tuple(types), pattern.region)?;
            }
            Pattern::Array { elements, rest } => {
                let item = self.fresh();
                for element in *elements {
                    self.infer_pattern_child(
                        env,
                        element,
                        item.clone(),
                        return_type.clone(),
                        &mut reachable,
                    )?;
                }
                if let Some(rest) = rest.and_then(|rest| rest.name)
                    && let BindingName::Local(local) = rest
                {
                    if let Some(existing) = env.locals.get(&local.id.0) {
                        self.unify(
                            existing.typ.clone(),
                            self.named("Array", vec![item.clone()]),
                            pattern.region,
                        )?;
                    }
                    env.locals.insert(
                        local.id.0,
                        Scheme {
                            record_overlays: Vec::new(),
                            tuple_shapes: Vec::new(),
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
                self.infer_pattern_with_return(env, pattern, expected.clone(), false, return_type)?;
                if let BindingName::Local(local) = name {
                    if let Some(existing) = env.locals.get(&local.id.0) {
                        return self.unify(existing.typ.clone(), expected, pattern.region);
                    }
                    env.locals.insert(
                        local.id.0,
                        Scheme {
                            record_overlays: Vec::new(),
                            tuple_shapes: Vec::new(),
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

    fn project_tuple(
        &mut self,
        tuple: Ty<'a>,
        index: u32,
        region: Region,
    ) -> Result<Ty<'a>, Error> {
        match self.prune(tuple) {
            Ty::Tuple(items) => items.get(index as usize).cloned().ok_or(Error {
                expectation: None,
                region,
                kind: ErrorKind::TupleIndexOutOfBounds {
                    index,
                    length: items.len(),
                },
            }),
            tuple @ Ty::Var(_) => {
                for slot in 0..self.tuple_shapes.len() {
                    let operand = self.tuple_shapes[slot].tuple.clone();
                    if self.prune(operand) != tuple {
                        continue;
                    }
                    let length = self.tuple_shapes[slot].length;
                    if u64::from(index) >= length {
                        return Err(Error {
                            expectation: None,
                            region,
                            kind: ErrorKind::TupleIndexOutOfBounds {
                                index,
                                // A failing u32 index guarantees this length fits usize
                                // on supported 32-bit and wider hosts.
                                length: length as usize,
                            },
                        });
                    }
                    if let Some(element) = self.tuple_shapes[slot].elements.get(&index) {
                        return Ok(element.clone());
                    }
                    let element = self.fresh();
                    self.tuple_shapes[slot]
                        .elements
                        .insert(index, element.clone());
                    return Ok(element);
                }
                let result = self.fresh();
                self.tuple_projections.push(TupleProjection {
                    tuple,
                    index,
                    result: result.clone(),
                    region,
                });
                Ok(result)
            }
            actual => Err(self.mismatch(region, actual, Ty::Tuple(Vec::new()))),
        }
    }

    fn solve_tuple_projections(&mut self) -> Result<(), Error> {
        let mut pending = std::mem::take(&mut self.tuple_projections);
        while !pending.is_empty() {
            let before = self
                .substitutions
                .iter()
                .filter(|entry| entry.is_some())
                .count();
            let mut unresolved = Vec::new();
            let mut elements = BTreeMap::new();
            for projection in pending {
                match self.prune(projection.tuple.clone()) {
                    Ty::Var(id) => {
                        let mut fixed = false;
                        for shape in self.tuple_shapes.clone() {
                            if self.prune(shape.tuple) == Ty::Var(id) {
                                let element = self.project_tuple(
                                    Ty::Var(id),
                                    projection.index,
                                    projection.region,
                                )?;
                                self.unify(projection.result.clone(), element, projection.region)?;
                                fixed = true;
                                break;
                            }
                        }
                        if fixed {
                            continue;
                        }
                        if let Some(previous) =
                            elements.insert((id, projection.index), projection.result.clone())
                        {
                            self.unify(previous, projection.result.clone(), projection.region)?;
                        }
                        unresolved.push(projection);
                    }
                    known => {
                        let element =
                            self.project_tuple(known, projection.index, projection.region)?;
                        self.unify(projection.result, element, projection.region)?;
                    }
                }
            }
            pending = unresolved;
            if pending.is_empty() {
                break;
            }
            if before
                != self
                    .substitutions
                    .iter()
                    .filter(|entry| entry.is_some())
                    .count()
            {
                continue;
            }
            // Equal projections are unified before choosing a shape: this also
            // joins nested projection operands reached through different aliases.
            let mut shapes = BTreeMap::new();
            for projection in std::mem::take(&mut pending) {
                let Ty::Var(id) = self.prune(projection.tuple.clone()) else {
                    unreachable!("stable unresolved projection");
                };
                let shape = shapes.entry(id).or_insert_with(|| SparseTupleShape {
                    tuple: Ty::Var(id),
                    length: 2,
                    elements: BTreeMap::new(),
                    region: projection.region,
                });
                shape.length = shape.length.max(u64::from(projection.index) + 1);
                shape.elements.insert(projection.index, projection.result);
            }
            self.tuple_shapes.extend(shapes.into_values());
        }
        Ok(())
    }

    fn solve_tuple_shapes(&mut self) -> Result<(), Error> {
        let mut unresolved: BTreeMap<usize, SparseTupleShape<'a>> = BTreeMap::new();
        for mut shape in std::mem::take(&mut self.tuple_shapes) {
            shape.tuple = self.prune(shape.tuple);
            match shape.tuple.clone() {
                Ty::Tuple(items) if items.len() as u64 == shape.length => {
                    for (index, expected) in shape.elements {
                        let Some(actual) = items.get(index as usize) else {
                            return Err(Error {
                                expectation: None,
                                region: shape.region,
                                kind: ErrorKind::TupleIndexOutOfBounds {
                                    index,
                                    length: items.len(),
                                },
                            });
                        };
                        self.unify(actual.clone(), expected, shape.region)?;
                    }
                }
                Ty::Var(id) => {
                    for element in shape.elements.values() {
                        if self.occurs(id, element) {
                            return Err(Error {
                                expectation: None,
                                region: shape.region,
                                kind: ErrorKind::InfiniteType,
                            });
                        }
                    }
                    if let Some(previous) = unresolved.get_mut(&id) {
                        if previous.length != shape.length {
                            return Err(Error {
                                expectation: None,
                                region: shape.region,
                                kind: ErrorKind::Mismatch {
                                    actual: DiagnosticType::TupleShape(shape.length),
                                    expected: DiagnosticType::TupleShape(previous.length),
                                },
                            });
                        }
                        for (index, element) in shape.elements {
                            if let Some(existing) = previous.elements.get(&index) {
                                self.unify(existing.clone(), element, shape.region)?;
                            } else {
                                previous.elements.insert(index, element);
                            }
                        }
                    } else {
                        unresolved.insert(id, shape);
                    }
                }
                actual => {
                    return Err(Error {
                        expectation: None,
                        region: shape.region,
                        kind: ErrorKind::Mismatch {
                            actual: self.diagnostic_type(actual, &mut BTreeMap::new()),
                            expected: DiagnosticType::TupleShape(shape.length),
                        },
                    });
                }
            }
        }
        self.tuple_shapes = unresolved.into_values().collect();
        self.check_tuple_shape_cycles()?;
        Ok(())
    }

    fn check_tuple_shape_cycles(&mut self) -> Result<(), Error> {
        let mut graph = BTreeMap::<usize, BTreeSet<usize>>::new();
        let mut regions = BTreeMap::new();
        for shape in self.tuple_shapes.clone() {
            let Ty::Var(id) = self.prune(shape.tuple) else {
                continue;
            };
            let edges = graph.entry(id).or_default();
            regions.insert(id, shape.region);
            for element in shape.elements.values() {
                self.free_vars(element, edges);
            }
        }
        let mut incoming = graph
            .keys()
            .map(|id| (*id, 0usize))
            .collect::<BTreeMap<_, _>>();
        for edges in graph.values_mut() {
            edges.retain(|id| incoming.contains_key(id));
            for id in edges.iter() {
                *incoming.get_mut(id).expect("shape vertex") += 1;
            }
        }
        let mut ready = incoming
            .iter()
            .filter_map(|(id, count)| (*count == 0).then_some(*id))
            .collect::<Vec<_>>();
        let mut visited = 0;
        while let Some(id) = ready.pop() {
            visited += 1;
            for target in &graph[&id] {
                let count = incoming.get_mut(target).expect("shape edge");
                *count -= 1;
                if *count == 0 {
                    ready.push(*target);
                }
            }
        }
        if visited != graph.len() {
            let id = incoming
                .iter()
                .find(|(_, count)| **count != 0)
                .expect("cyclic vertex")
                .0;
            return Err(Error {
                expectation: None,
                region: regions[id],
                kind: ErrorKind::InfiniteType,
            });
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
        if let Some(return_type) = return_type.as_ref()
            && matches!(self.prune(actual.clone()), Ty::Var(_))
            && matches!(self.prune(return_type.clone()), Ty::Var(_))
        {
            let value = self.fresh();
            self.try_constraints.push(TryConstraint {
                actual,
                return_type: return_type.clone(),
                value: value.clone(),
                region,
            });
            return Ok(value);
        }
        self.resolve_try_type(actual, return_type, region)
    }

    fn resolve_try_boundary(
        &mut self,
        result: Ty<'a>,
        body: &Ty<'a>,
        region: Region,
    ) -> Result<(), Error> {
        let result = self.prune(result);
        let pending = std::mem::take(&mut self.try_constraints);
        if !pending
            .iter()
            .any(|constraint| self.prune(constraint.return_type.clone()) == result)
        {
            self.try_constraints = pending;
            return Ok(());
        }
        let body = self.prune(body.clone());
        if matches!(result, Ty::Var(_)) {
            if self.result_parts(body.clone()).is_some() {
                self.require_result_parts(result.clone(), region)?;
            } else if matches!(nominal_parts(&body), Some((name, [_]))
                if name.module.package == PackageId::Builtin && name.module.path.is_empty()
                    && name.name == "Option")
            {
                let payload = self.fresh();
                self.unify(result.clone(), self.named("Option", vec![payload]), region)?;
            }
        }
        let target = self.prune(result);
        for constraint in pending {
            if self.prune(constraint.return_type.clone()) == target {
                // A recursive peer may not have contributed its return type
                // yet. Do not default an undetermined carrier at this boundary;
                // the SCC pass resolves it after checking every member.
                if matches!(target, Ty::Var(_))
                    && matches!(body, Ty::Var(_))
                    && matches!(self.prune(constraint.actual.clone()), Ty::Var(_))
                {
                    self.try_constraints.push(constraint);
                    continue;
                }
                let value = self.resolve_try_type(
                    constraint.actual,
                    Some(constraint.return_type),
                    constraint.region,
                )?;
                self.unify(value, constraint.value, constraint.region)?;
            } else {
                self.try_constraints.push(constraint);
            }
        }
        Ok(())
    }

    fn solve_try_constraints(&mut self) -> Result<(), Error> {
        for constraint in std::mem::take(&mut self.try_constraints) {
            let value = self.resolve_try_type(
                constraint.actual,
                Some(constraint.return_type),
                constraint.region,
            )?;
            self.unify(value, constraint.value, constraint.region)?;
        }
        Ok(())
    }

    fn resolve_try_type(
        &mut self,
        actual: Ty<'a>,
        return_type: Option<Ty<'a>>,
        region: Region,
    ) -> Result<Ty<'a>, Error> {
        let is_option = |typ: &Ty<'a>| {
            matches!(
                nominal_parts(typ),
                Some((reference, [_])) if reference.module.package == PackageId::Builtin
                    && reference.module.path.is_empty() && reference.name == "Option"
            )
        };
        let actual = self.prune(actual);
        let return_type = return_type.map(|typ| self.prune(typ));
        if is_option(&actual) || return_type.as_ref().is_some_and(is_option) {
            let value = self.fresh();
            self.unify(actual, self.named("Option", vec![value.clone()]), region)?;
            let Some(return_type) = return_type else {
                return Err(Error {
                    expectation: None,
                    region,
                    kind: ErrorKind::InvalidTry,
                });
            };
            let returned = self.fresh();
            self.unify(return_type, self.named("Option", vec![returned]), region)?;
            self.option_tries.insert(region);
            return Ok(self.prune(value));
        }
        let (value, error) = if let Some(parts) = self.result_parts(actual.clone()) {
            // A known Result already carries its source error row. Rebinding
            // it to a fresh open row can turn a universal row variable into
            // an empty row extension, spuriously specializing its contract.
            parts
        } else {
            let value = self.fresh();
            let error = self.fresh_error_row();
            self.unify(
                actual,
                self.named("Result", vec![value.clone(), error.clone()]),
                region,
            )?;
            (value, error)
        };
        let Some(return_type) = return_type else {
            return Err(Error {
                expectation: None,
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
        pipe_use_id: UseId,
        destination: &'a Located<Expr<'a>>,
        leading: &'a Located<Expr<'a>>,
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
                    leading: Some(leading),
                    expected_result: None,
                },
                return_type,
            ),
            Expr::Await(inner) => {
                let actual =
                    self.infer_pipe_destination(env, pipe_use_id, inner, leading, return_type)?;
                self.infer_await_type(actual, destination.region)
                    .map_err(|error| error.expected_by(ExpectationKind::Await, None))
            }
            Expr::Try(inner) => {
                let actual = self.infer_pipe_destination(
                    env,
                    pipe_use_id,
                    inner,
                    leading,
                    return_type.clone(),
                )?;
                self.infer_try_type(actual, return_type, destination.region)
                    .map_err(|error| error.expected_by(ExpectationKind::Propagation, None))
            }
            _ => self.infer_call(
                env,
                CallInput {
                    region: destination.region,
                    use_id: pipe_use_id,
                    function: destination,
                    arguments: &[],
                    leading: Some(leading),
                    expected_result: None,
                },
                return_type,
            ),
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
        if op == BinOp::Pipe {
            return self.infer_pipe_destination(env, use_id, right, left, return_type);
        }
        let left_type = self.infer_expr(env, left, return_type.clone())?;

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

    fn option_spine(&mut self, typ: Ty<'a>) -> (usize, Ty<'a>) {
        let mut depth = 0;
        let mut root = self.normalize_projection_root(typ);
        while let Some((reference, [payload])) = nominal_parts(&root)
            && reference.module.package == PackageId::Builtin
            && reference.module.path.is_empty()
            && reference.name == "Option"
        {
            depth += 1;
            root = self.normalize_projection_root(payload.clone());
        }
        (depth, root)
    }

    fn lift_option(&self, mut typ: Ty<'a>, depth: usize) -> Ty<'a> {
        for _ in 0..depth {
            typ = self.named("Option", vec![typ]);
        }
        typ
    }

    fn solve_option_lifts(&mut self) -> Result<(), Error> {
        let constraints = std::mem::take(&mut self.option_lifts);
        if constraints.is_empty() {
            return Ok(());
        }
        loop {
            let mut variables = BTreeMap::<usize, (usize, Ty<'a>)>::new();
            let mut edges = Vec::with_capacity(constraints.len());
            for constraint in &constraints {
                let (actual_depth, actual_root) = self.option_spine(constraint.actual.clone());
                let (expected_depth, expected_root) =
                    self.option_spine(constraint.expected.clone());
                let mut endpoints = Vec::with_capacity(2);
                for root in [actual_root, expected_root] {
                    endpoints.push(if let Ty::Var(id) = root {
                        let next = variables.len() + 1;
                        variables
                            .entry(id)
                            .or_insert_with(|| (next, self.fresh()))
                            .clone()
                    } else {
                        (0, root)
                    });
                }
                // Wrapping changes only the outer Option spine. Mutable
                // payloads and their nested fields still use ordinary equality.
                self.check_value(
                    endpoints[0].1.clone(),
                    endpoints[1].1.clone(),
                    constraint.region,
                )
                .map_err(|error| constraint.explain(error))?;
                edges.push(crate::option_levels::Edge {
                    from: endpoints[0].0,
                    to: endpoints[1].0,
                    offset: actual_depth as i64 - expected_depth as i64,
                });
            }
            // Payload equations can expose an outer variable through a nested
            // type. Rebuild the depth problem with those ordinary equalities
            // applied, rather than keeping stale spine nodes.
            if variables
                .keys()
                .any(|id| self.prune(Ty::Var(*id)) != Ty::Var(*id))
            {
                continue;
            }
            // Row-kind variables are already known not to have outer Option
            // structure, even when they were inferred rather than annotated.
            // Preserve that kind while choosing wrapping for their uses.
            for (id, (node, _)) in &variables {
                if matches!(
                    self.variable_kinds[*id],
                    VariableKind::RecordRow | VariableKind::ErrorRow
                ) {
                    edges.push(crate::option_levels::Edge {
                        from: *node,
                        to: 0,
                        offset: 0,
                    });
                }
            }
            // An explicitly universal type is opaque here: its definition
            // cannot assume any outer Option structure. Choosing a direct
            // match by specializing it would discard a valid Some insertion
            // and violate the contract when this SCC is generalized.
            let universals = self
                .generic_contracts
                .iter()
                .flat_map(|contract| contract.variables.values().cloned())
                .collect::<Vec<_>>();
            for universal in universals {
                if let Ty::Var(id) = self.prune(universal)
                    && let Some((node, _)) = variables.get(&id)
                {
                    edges.push(crate::option_levels::Edge {
                        from: *node,
                        to: 0,
                        offset: 0,
                    });
                }
            }
            for shape in self.tuple_shapes.clone() {
                if let Ty::Var(id) = self.prune(shape.tuple)
                    && let Some((node, _)) = variables.get(&id)
                {
                    // A sparse tuple is known not to be an Option even though
                    // its exact shape remains represented by a type variable.
                    edges.push(crate::option_levels::Edge {
                        from: *node,
                        to: 0,
                        offset: 0,
                    });
                }
            }
            let levels =
                crate::option_levels::solve(variables.len() + 1, &edges).map_err(|failure| {
                    // Source constraints precede synthetic kind/shape edges.
                    // Every nonempty component originates at a source site,
                    // so its earliest edge identifies a real participating use.
                    let constraint = &constraints[failure.edge];
                    constraint.explain(match failure.kind {
                        crate::option_levels::Failure::Ambiguous => Error {
                            expectation: None,
                            region: constraint.region,
                            kind: ErrorKind::AmbiguousOptionLifting,
                        },
                        crate::option_levels::Failure::Inconsistent => self.mismatch(
                            constraint.region,
                            constraint.actual.clone(),
                            constraint.expected.clone(),
                        ),
                    })
                })?;
            for (id, (node, payload)) in variables {
                self.bind(
                    id,
                    self.lift_option(payload, levels[node]),
                    constraints[0].region,
                )?;
            }
            for (constraint, edge) in constraints.iter().zip(edges) {
                let depth = usize::try_from(
                    levels[edge.to] as i64 - levels[edge.from] as i64 - edge.offset,
                )
                .expect("solved Option inequalities have nonnegative slack");
                self.check_value(
                    self.lift_option(constraint.actual.clone(), depth),
                    constraint.expected.clone(),
                    constraint.region,
                )
                .map_err(|error| constraint.explain(error))?;
                if depth > 0 {
                    match constraint.site {
                        OptionLiftSite::Argument(use_id, index) => {
                            self.argument_lifts.insert((use_id, index), depth);
                        }
                        OptionLiftSite::Field(region) => {
                            self.field_lifts.insert(region, depth);
                        }
                    }
                }
            }
            return Ok(());
        }
    }

    fn infer_lift_input(
        &mut self,
        env: &Env<'a>,
        expression: &'a Located<Expr<'a>>,
        expected: Ty<'a>,
        return_type: Option<Ty<'a>>,
    ) -> Result<Ty<'a>, Error> {
        if let Expr::Block(block) = expression.value {
            return self.infer_block_context(
                &mut env.clone(),
                block,
                return_type,
                Some(ExprExpectation::LiftInput(expected)),
            );
        }
        if matches!(expression.value, Expr::If { .. } | Expr::Match { .. }) {
            return self.infer_branch_context(
                env,
                expression,
                return_type,
                Some(ExprExpectation::LiftInput(expected)),
            );
        }
        if let Expr::Call { function, .. } = expression.value
            && is_option_some_expr(function)
        {
            return self.infer_checked_expr(env, expression, expected, return_type);
        }
        let (_, payload) = self.option_spine(expected);
        let fresh_record = matches!(expression.value, Expr::Record(_))
            && matches!(payload, Ty::Record(..) | Ty::Var(_));
        let fresh_array = matches!(expression.value, Expr::Array(_))
            && (matches!(payload, Ty::Var(_))
                || matches!(nominal_parts(&payload), Some((reference, [_]))
                if reference.module.package == PackageId::Builtin
                    && reference.module.path.is_empty() && reference.name == "Array"));
        if fresh_record || fresh_array {
            // A literal record/array is not itself an Option. Its fields still
            // need the contextual payload type before the outer Some layers
            // are solved. Do not apply this to an existing mutable alias.
            self.infer_checked_expr(env, expression, payload, return_type)
        } else {
            self.infer_expr(env, expression, return_type)
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
            expected_result,
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
        let destination_reachable =
            leading.is_none_or(|input| alder_ast::flow::expression(input).falls_through);
        // Obtain parameter context without consuming the pipe input first.
        // Runtime emission still evaluates that input before the destination.
        let function_type = self.with_reachability(destination_reachable, |this| {
            this.infer_expr(env, function, return_type.clone())
        })?;
        let mut args = Vec::with_capacity(arguments.len() + usize::from(leading.is_some()));
        if let Some(expected) = expected_result
            && let Ty::Fn(_, result) = self.prune(function_type.clone())
        {
            self.unify(*result, expected, region)?;
        }
        let accepts_error_tag = is_result_err_expr(function);
        let callee = match function.value {
            Expr::Var {
                reference:
                    ValueRef::TopLevel(name)
                    | ValueRef::Foreign {
                        reference: name, ..
                    },
                ..
            } => Some(name.name.to_owned()),
            Expr::Var {
                reference: ValueRef::Local(local),
                ..
            } => Some(local.text.to_owned()),
            Expr::Var {
                reference: ValueRef::TraitMethod { method, .. },
                ..
            } => Some(method.name.to_owned()),
            _ => None,
        };
        if accepts_error_tag {
            if let Some(argument) = leading {
                self.legal_tag_sites.push(argument.region);
            } else if let Some(argument) = arguments.first() {
                self.legal_tag_sites.push(argument.region);
            }
        }
        for (index, argument) in leading
            .into_iter()
            .chain(arguments.iter().copied())
            .enumerate()
        {
            let expected = match self.prune(function_type.clone()) {
                Ty::Fn(params, _) => params.get(args.len()).cloned(),
                _ => None,
            };
            // Check each known parameter before inferring the next argument.
            // Otherwise a later contextual literal can specialize shared type
            // variables before an earlier, already-typed argument is checked.
            let reachable = (leading.is_some() && index == 0) || destination_reachable;
            let actual = self
                .with_reachability(reachable, |this| {
                    if let Some(expected) = expected {
                        if this.option_spine(expected.clone()).0 > 0 {
                            let actual = this.infer_lift_input(
                                env,
                                argument,
                                expected.clone(),
                                return_type.clone(),
                            )?;
                            this.option_lifts.push(OptionLift {
                                actual,
                                expected: expected.clone(),
                                region: argument.region,
                                site: OptionLiftSite::Argument(use_id, args.len()),
                                expectation: Some(ExpectationKind::Argument {
                                    position: index + 1,
                                    callee: callee.clone(),
                                }),
                            });
                            Ok(expected)
                        } else {
                            this.infer_checked_expr(env, argument, expected, return_type.clone())
                        }
                    } else {
                        this.infer_expr(env, argument, return_type.clone())
                    }
                })
                .map_err(|error| {
                    error.expected_by(
                        ExpectationKind::Argument {
                            position: index + 1,
                            callee: callee.clone(),
                        },
                        None,
                    )
                })?;
            args.push(actual);
        }
        if let Ty::Fn(params, _) = self.prune(function_type.clone())
            && args.len() < params.len()
            && params[args.len()..].iter().all(|param| {
                matches!(
                    nominal_parts(&self.prune(param.clone())),
                    Some((reference, [_])) if reference.module.package == PackageId::Builtin
                        && reference.module.path.is_empty() && reference.name == "Option"
                )
            })
        {
            self.omitted_arguments
                .insert(use_id, params.len() - args.len());
            args.extend_from_slice(&params[args.len()..]);
        }
        if let Ty::Fn(params, _) = self.prune(function_type.clone())
            && args.len() != params.len()
        {
            return Err(Error {
                region,
                kind: ErrorKind::Arity {
                    expected: params.len(),
                    actual: args.len(),
                },
                expectation: None,
            }
            .expected_by(ExpectationKind::Call { callee }, None));
        }
        let result = self.fresh();
        let call_type = Ty::Fn(args, Box::new(result.clone()));
        let call_region = leading.map_or(region, |argument| argument.region);
        self.unify(call_type, function_type, call_region)
            .map_err(|error| error.expected_by(ExpectationKind::Call { callee }, None))?;
        // Arguments can close an instantiated overlay. Resolve its shape
        // before a caller reads or destructures the result, so projection
        // checks the final rightmost field type rather than an earlier operand.
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
        let mut reachable = true;
        for (index, step) in place.steps.iter().enumerate() {
            typ = match step {
                alder_ast::PlaceStep::Field(field) => {
                    // The final member stores a raw payload, even when reading
                    // that member would produce Option[T]. Intermediate members
                    // remain reads: an optional parent cannot be traversed as T.
                    let payload = if index + 1 == place.steps.len() {
                        match self.prune(typ.clone()) {
                            Ty::Record(fields, _) => fields.get(field.value).cloned(),
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
                alder_ast::PlaceStep::TupleIndex(index) => {
                    self.project_tuple(typ, index.value, index.region)?
                }
                alder_ast::PlaceStep::Index(index) => {
                    let item = self.fresh();
                    self.unify(typ, self.named("Array", vec![item.clone()]), region)?;
                    let index_type = self.with_reachability(reachable, |this| {
                        this.infer_expr(env, index, return_type.clone())
                    })?;
                    reachable &= alder_ast::flow::expression(index).falls_through;
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
                Some(typ) => Ok(typ.clone()),
                None if tail.is_some() => {
                    let result = self.fresh();
                    let fields = BTreeMap::from([(field, result.clone())]);
                    let fragment = self.open_record(fields);
                    self.unify(
                        *tail.expect("open tail"),
                        Ty::RecordRow(Box::new(fragment)),
                        region,
                    )?;
                    Ok(result)
                }
                None => Err(Error {
                    expectation: None,
                    region,
                    kind: ErrorKind::MissingField {
                        field: field.to_owned(),
                        available: fields.keys().map(|name| (*name).to_owned()).collect(),
                    },
                }),
            },
            Ty::Var(id) => {
                let result = self.fresh();
                let fields = BTreeMap::from([(field, result.clone())]);
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
                    )
                    .map_err(|error| error.expected_by(ExpectationKind::Condition, None))?;
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
        let mut tuple_shapes = Vec::new();
        let mut selected_shapes = BTreeSet::new();
        let pending_shapes = self.tuple_shapes.clone();
        let pending_overlays = self.record_overlays.clone();
        let pending = self.scheme_error_row_inclusions(&vars, outer_free);
        loop {
            let before = selected.len() + selected_overlays.len() + selected_shapes.len();
            for (index, shape) in pending_shapes.iter().enumerate() {
                if selected_shapes.contains(&index) {
                    continue;
                }
                let mut related = BTreeSet::new();
                self.free_vars(&shape.tuple, &mut related);
                for element in shape.elements.values() {
                    self.free_vars(element, &mut related);
                }
                if !related.is_disjoint(&vars) {
                    vars.extend(related);
                    selected_shapes.insert(index);
                    tuple_shapes.push(shape.clone());
                }
            }
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
            if before == selected.len() + selected_overlays.len() + selected_shapes.len() {
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
                tuple_shapes,
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
        for shape in self.tuple_shapes.clone() {
            self.free_vars(&shape.tuple, &mut protected);
            for element in shape.elements.values() {
                self.free_vars(element, &mut protected);
            }
        }
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
        for shape in &scheme.tuple_shapes {
            self.free_vars(&shape.tuple, &mut free);
            for element in shape.elements.values() {
                self.free_vars(element, &mut free);
            }
        }
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
        let shapes = scheme
            .tuple_shapes
            .iter()
            .map(|shape| SparseTupleShape {
                tuple: self.replace_vars(&shape.tuple, &replacements),
                length: shape.length,
                elements: shape
                    .elements
                    .iter()
                    .map(|(index, typ)| (*index, self.replace_vars(typ, &replacements)))
                    .collect(),
                region,
            })
            .collect::<Vec<_>>();
        self.tuple_shapes.extend(shapes);
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
        for shape in annotation.tuple_shapes {
            let tuple = self.from_ast(shape.tuple, &mut vars);
            let elements = shape
                .elements
                .iter()
                .map(|(index, typ)| (*index, self.from_ast(typ, &mut vars)))
                .collect();
            self.tuple_shapes.push(SparseTupleShape {
                tuple,
                length: shape.length,
                elements,
                region,
            });
        }
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
                            expectation: None,
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
            Type::Named {
                reference,
                args: [],
            } if self.database.error_group(*reference).is_some() => {
                self.convert_ast_error_type(typ, vars)
            }
            Type::Named { reference, args } => {
                let mut converted = args
                    .iter()
                    .enumerate()
                    .map(|(index, arg)| {
                        if is_builtin_result(*reference) && index == 1 {
                            self.convert_ast_error_type(arg, vars)
                        } else {
                            self.from_ast(arg, vars)
                        }
                    })
                    .collect::<Vec<_>>();
                if is_builtin_result(*reference) && converted.len() == 1 {
                    converted.push(self.fresh_error_row());
                }
                self.apply(Ty::Con(*reference), converted)
            }
            Type::Partial { constructor, slots } => Ty::Partial(
                *constructor,
                slots
                    .iter()
                    .enumerate()
                    .map(|(position, slot)| match slot {
                        TypeSlot::Hole(index) => TySlot::Hole(*index),
                        TypeSlot::Fixed(typ)
                            if is_builtin_result(*constructor) && position == 1 =>
                        {
                            TySlot::Fixed(self.convert_ast_error_type(typ, vars))
                        }
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
                    .map(|field| (field.name, self.from_ast(field.typ, vars)))
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
        let converted = match &typ.value {
            Type::Var { name, args: [] } => {
                if let Some(existing) = vars.get(name) {
                    if let Ty::Var(id) = existing {
                        self.variable_kinds[*id] = VariableKind::ErrorRow;
                    }
                    let existing = existing.clone();
                    self.error_kind_checks.push((existing.clone(), typ.region));
                    return existing;
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
                    if !self.expanding_error_groups.insert(*reference) {
                        self.annotation_error.get_or_insert_with(|| Error {
                            expectation: None,
                            region: typ.region,
                            kind: ErrorKind::RecursiveErrorGroup {
                                name: reference.name.to_owned(),
                            },
                        });
                        return Ty::Any;
                    }
                    let row = self.error_row_from_tags(tags, RowExtension::Closed, vars);
                    self.expanding_error_groups.remove(reference);
                    row
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
        };
        self.error_kind_checks.push((converted.clone(), typ.region));
        converted
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
                if matches!(*head, Ty::Con(reference) if is_builtin_result(reference))
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
                expectation: None,
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
                    expectation: None,
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
                    expectation: None,
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
                expectation: None,
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
                        expectation: None,
                        region: contract.region,
                        kind: ErrorKind::GenericSpecialization {
                            variable: name.to_owned(),
                            actual: self.render(resolved),
                        },
                    });
                };
                for shape in self.tuple_shapes.clone() {
                    if self.prune(shape.tuple) == Ty::Var(id) {
                        return Err(Error {
                            expectation: None,
                            region: contract.region,
                            kind: ErrorKind::GenericSpecialization {
                                variable: name.to_owned(),
                                actual: format!("tuple of length {}", shape.length),
                            },
                        });
                    }
                }
                if let Some(previous) = representatives.insert(id, name) {
                    return Err(Error {
                        expectation: None,
                        region: contract.region,
                        kind: ErrorKind::GenericSpecialization {
                            variable: name.to_owned(),
                            actual: previous.to_owned(),
                        },
                    });
                }
                if escaped.contains(&id) {
                    return Err(Error {
                        expectation: None,
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
        let middle = Ty::Record(BTreeMap::from([("marker", Ty::Unit)]), None);
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
        self.expanded_overlay_operands_from(overlay, &overlays).0
    }

    fn expanded_overlay_operands_from(
        &mut self,
        overlay: &RecordOverlay<'a>,
        overlays: &[RecordOverlay<'a>],
    ) -> (Vec<Ty<'a>>, bool) {
        let mut pending = overlay
            .operands
            .iter()
            .map(|operand| (operand.clone(), BTreeSet::new()))
            .collect::<Vec<_>>();
        let mut visited = BTreeSet::new();
        let mut expanded = Vec::new();
        let mut cyclic = false;
        while let Some((operand, path)) = pending.pop() {
            let operand = self.prune(operand);
            // Closed emptiness is the overlay identity. An open row with no
            // known fields is not empty: it may still overwrite any field.
            if matches!(&operand, Ty::Record(fields, None) if fields.is_empty()) {
                continue;
            }
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
                cyclic |= overlays.iter().enumerate().any(|(index, candidate)| {
                    path.contains(&index)
                        && matches!(&operand, Ty::Record(_, Some(_)))
                        && self.prune(candidate.result.clone()) == operand
                });
                // We visit right-to-left. An earlier identical input shape
                // cannot add a field type or presence alternative beyond its
                // later occurrence, including around intervening overwrites.
                if !expanded.contains(&operand) {
                    expanded.push(operand);
                }
            }
        }
        // The expansion is right-to-left. A later known field guarantees
        // that an earlier closed field contributes nothing to the result,
        // even across unknown rows. Keep open operands intact: their other
        // fields can still contribute. This only normalizes type relations,
        // never the evaluation of the source operands.
        let mut overwritten = BTreeSet::new();
        expanded.retain_mut(|operand| {
            if let Ty::Record(fields, None) = operand {
                fields.retain(|name, _| overwritten.insert(*name));
                !fields.is_empty()
            } else {
                // Expanded acyclic leaves are input records, so their known
                // fields are guaranteed even when their residual row is open.
                // Opaque cyclic producer fields can instead be obligations;
                // do not use those as proof of an overwrite.
                if !cyclic && let Ty::Record(fields, Some(_)) = operand {
                    overwritten.extend(fields.keys().copied());
                }
                true
            }
        });
        expanded.reverse();
        // Adjacent closed operands are one ordinary right-biased record.
        // Grouping explicit fields into a spread literal cannot distinguish
        // otherwise identical overlay contracts. Never merge across an open
        // operand: it may overwrite any of the preceding fields.
        let mut normalized = Vec::with_capacity(expanded.len());
        for operand in expanded {
            match (normalized.last_mut(), operand) {
                (Some(Ty::Record(previous, None)), Ty::Record(fields, None)) => {
                    previous.extend(fields);
                }
                (_, operand) => normalized.push(operand),
            }
        }
        (normalized, cyclic)
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
                                expectation: None,
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
            for (name, expected_type) in expected_fields {
                let mut selected = None;
                for operand in operands.iter().rev() {
                    let Ty::Record(fields, tail) = self.prune(operand.clone()) else {
                        break;
                    };
                    if let Some(typ) = fields.get(name) {
                        selected = Some(typ.clone());
                        break;
                    } else if let Some(tail) = tail {
                        let mut variables = BTreeSet::new();
                        self.free_vars(&tail, &mut variables);
                        if let Some(variable) = variables.iter().find_map(|id| universals.get(id)) {
                            return Err(Error {
                                expectation: None,
                                region: overlay.region,
                                kind: ErrorKind::GenericSpecialization {
                                    variable: (*variable).to_owned(),
                                    actual: format!(
                                        "a record row with a constrained `{name}` field"
                                    ),
                                },
                            });
                        }
                        break;
                    }
                }
                if let Some(typ) = selected {
                    let actual = Ty::Record(BTreeMap::from([(name, typ)]), None);
                    let expected = Ty::Record(BTreeMap::from([(name, expected_type)]), None);
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
            return Ok(joined);
        }
        self.unify(right.clone(), left.clone(), region)?;
        let joined = match (self.prune(left.clone()), self.prune(right.clone())) {
            (Ty::Record(mut fields, tail), Ty::Record(other, _)) => {
                for (name, typ) in other {
                    fields.entry(name).or_insert(typ);
                }
                Ty::Record(fields, tail)
            }
            (left, _) => left,
        };
        Ok(joined)
    }

    fn infer_expr_context(
        &mut self,
        env: &Env<'a>,
        expression: &'a Located<Expr<'a>>,
        return_type: Option<Ty<'a>>,
        expected: Option<ExprExpectation<'a>>,
    ) -> Result<Ty<'a>, Error> {
        match expected {
            Some(ExprExpectation::Exact(expected)) => {
                self.infer_checked_expr(env, expression, expected, return_type)
            }
            Some(ExprExpectation::LiftInput(expected)) => {
                self.infer_lift_input(env, expression, expected, return_type)
            }
            None => self.infer_expr(env, expression, return_type),
        }
    }

    fn infer_branch_context(
        &mut self,
        env: &Env<'a>,
        expression: &'a Located<Expr<'a>>,
        return_type: Option<Ty<'a>>,
        expected: Option<ExprExpectation<'a>>,
    ) -> Result<Ty<'a>, Error> {
        let region = expression.region;
        let actual = match &expression.value {
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
                    )
                    .map_err(|error| error.expected_by(ExpectationKind::Condition, None))?;
                    remaining &= alder_ast::flow::expression(branch.condition).falls_through;
                    let body = self.with_reachability(
                        remaining && !matches!(branch.condition.value, Expr::Bool(false)),
                        |this| {
                            this.infer_block_context(
                                &mut env.clone(),
                                branch.body,
                                return_type.clone(),
                                expected.clone(),
                            )
                        },
                    )?;
                    result = self
                        .join_values(result, body, branch.body.region)
                        .map_err(|error| error.expected_by(ExpectationKind::Branch, None))?;
                    remaining &= !matches!(branch.condition.value, Expr::Bool(true));
                }
                if let Some(final_else) = final_else {
                    let body = self.with_reachability(remaining, |this| {
                        this.infer_block_context(
                            &mut env.clone(),
                            final_else,
                            return_type,
                            expected.clone(),
                        )
                    })?;
                    result =
                        self.join_values(result, body, final_else.region)
                            .map_err(|error| {
                                error.expected_by(
                                    ExpectationKind::Branch,
                                    branches.first().map(|branch| branch.body.region),
                                )
                            })?;
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
                let mut remaining = alder_ast::flow::expression(scrutinee).falls_through;
                for arm in *arms {
                    let mut local = env.clone();
                    let mut alternative_reachable = remaining;
                    let mut matched = false;
                    for pattern in arm.patterns {
                        self.with_reachability(alternative_reachable, |this| {
                            this.infer_pattern_with_return(
                                &mut local,
                                pattern,
                                scrutinee_type.clone(),
                                false,
                                return_type.clone(),
                            )
                        })?;
                        let pattern_flow = alder_ast::flow::pattern(pattern);
                        matched |= alternative_reachable && pattern_flow.matches;
                        alternative_reachable &= pattern_flow.guarded(arm.guard).rejects;
                    }
                    if let Some(guard) = arm.guard {
                        let guard_type = self.with_reachability(matched, |this| {
                            this.infer_expr(&local, guard, return_type.clone())
                        })?;
                        self.unify(guard_type, self.named("Bool", Vec::new()), guard.region)?;
                    }
                    let body = self.with_reachability(
                        matched
                            && arm.guard.is_none_or(|guard| {
                                alder_ast::flow::expression(guard).falls_through
                                    && !matches!(guard.value, Expr::Bool(false))
                            }),
                        |this| {
                            this.infer_expr_context(
                                &local,
                                arm.body,
                                return_type.clone(),
                                expected.clone(),
                            )
                        },
                    )?;
                    result = self
                        .join_values(result, body, arm.body.region)
                        .map_err(|error| error.expected_by(ExpectationKind::Branch, None))?;
                    remaining = alternative_reachable;
                }
                Ok(self.prune(result))
            }
            _ => unreachable!("branch context only applies to if and match"),
        }?;
        if let Some(ExprExpectation::Exact(expected)) = expected
            && alder_ast::flow::expression(expression).falls_through
        {
            self.check_value(actual.clone(), expected, region)?;
        }
        Ok(self.prune(actual))
    }

    fn solve_record_initializers(&mut self) -> Result<(), Error> {
        for (actual, expected, region) in std::mem::take(&mut self.record_initializers) {
            let actual = self.default_record_fields(actual, expected.clone(), region)?;
            self.check_value(actual, expected, region)?;
        }
        Ok(())
    }

    fn default_record_fields(
        &mut self,
        actual: Ty<'a>,
        expected: Ty<'a>,
        region: Region,
    ) -> Result<Ty<'a>, Error> {
        let actual = self.prune(actual);
        let Ty::Record(mut fields, tail) = actual else {
            return Ok(actual);
        };
        let mut defaults = BTreeMap::new();
        if let Ty::Record(expected_fields, _) = self.prune(expected) {
            for (name, typ) in expected_fields {
                if fields.contains_key(name) {
                    continue;
                }
                // Omission is a None initializer, so it constrains an unknown
                // field type just like an explicitly supplied None would.
                // Universal contracts are still checked before publication.
                if matches!(self.prune(typ.clone()), Ty::Var(_)) {
                    let payload = self.fresh();
                    self.unify(typ.clone(), self.named("Option", vec![payload]), region)?;
                }
                if self.option_spine(typ.clone()).0 > 0 {
                    defaults.insert(name, typ);
                    self.omitted_record_fields
                        .entry(region)
                        .or_default()
                        .push(name);
                }
            }
        }
        if tail.is_some() && !defaults.is_empty() {
            // Codegen emits defaults before source fields/spreads. Preserve
            // that ordered overlay in the type contract as well: an unknown
            // input may overwrite None, but never has to provide the default.
            let result = self.open_record(BTreeMap::new());
            self.record_overlays.push(RecordOverlay {
                operands: vec![Ty::Record(defaults, None), Ty::Record(fields, tail)],
                result: result.clone(),
                region,
            });
            Ok(result)
        } else {
            fields.extend(defaults);
            Ok(Ty::Record(fields, tail))
        }
    }

    fn infer_checked_expr(
        &mut self,
        env: &Env<'a>,
        expression: &'a Located<Expr<'a>>,
        expected: Ty<'a>,
        return_type: Option<Ty<'a>>,
    ) -> Result<Ty<'a>, Error> {
        if matches!(expression.value, Expr::If { .. } | Expr::Match { .. }) {
            return self.infer_branch_context(
                env,
                expression,
                return_type,
                Some(ExprExpectation::Exact(expected)),
            );
        }
        if let Expr::Call {
            use_id,
            function,
            arguments,
        } = expression.value
            && is_option_some_expr(function)
            && (self.option_spine(expected.clone()).0 > 0
                || matches!(self.prune(expected.clone()), Ty::Var(_)))
        {
            return self.infer_call(
                env,
                CallInput {
                    region: expression.region,
                    use_id,
                    function,
                    arguments,
                    leading: None,
                    expected_result: Some(expected),
                },
                return_type,
            );
        }
        if let Expr::Block(block) = expression.value {
            return self.infer_block_with_expected(
                &mut env.clone(),
                block,
                return_type,
                Some(expected),
            );
        }
        if let Expr::Record(fields) = expression.value
            && (matches!(
                self.prune(expected.clone()),
                Ty::Var(_) | Ty::Record(_, Some(_))
            ) || fields
                .iter()
                .any(|field| matches!(field, RecordField::Spread(_))))
        {
            let actual = self.infer_record_fields(env, fields, return_type, true)?;
            if matches!(
                self.prune(expected.clone()),
                Ty::Var(_) | Ty::Record(_, Some(_))
            ) && let Ty::Record(actual_fields, _) = self.prune(actual.clone())
            {
                // Retain the literal's known fields while later constraints
                // establish its contextual shape. Validate it before
                // generalization; closed inputs close this tail, while open
                // spreads retain their checked overlay relationship.
                let shape = self.open_record(actual_fields);
                self.unify(expected.clone(), shape, expression.region)?;
                self.record_initializers
                    .push((actual, expected.clone(), expression.region));
                return Ok(self.prune(expected));
            }
            let actual = self.default_record_fields(actual, expected.clone(), expression.region)?;
            self.check_value(actual, expected.clone(), expression.region)?;
            return Ok(self.prune(expected));
        }
        if let Expr::Record(fields) = expression.value
            && fields
                .iter()
                .all(|field| matches!(field, RecordField::Field { .. }))
            && let Ty::Record(expected_fields, _) = self.prune(expected.clone())
        {
            let mut actual_fields = BTreeMap::new();
            let mut reachable = true;
            for field in fields {
                let RecordField::Field { name, value } = field else {
                    unreachable!()
                };
                let typ = if let Some(typ) = expected_fields.get(name.value) {
                    if self.option_spine(typ.clone()).0 > 0 {
                        let actual = self.with_reachability(reachable, |this| {
                            this.infer_lift_input(env, value, typ.clone(), return_type.clone())
                        })?;
                        self.option_lifts.push(OptionLift {
                            actual,
                            expected: typ.clone(),
                            region: value.region,
                            site: OptionLiftSite::Field(name.region),
                            expectation: None,
                        });
                        typ.clone()
                    } else {
                        self.with_reachability(reachable, |this| {
                            this.infer_checked_expr(env, value, typ.clone(), return_type.clone())
                        })?
                    }
                } else {
                    self.with_reachability(reachable, |this| {
                        this.infer_expr(env, value, return_type.clone())
                    })?
                };
                reachable &= alder_ast::flow::expression(value).falls_through;
                actual_fields.insert(name.value, typ);
            }
            // Defaults belong to fresh construction, never to unification of
            // an existing mutable record alias. Materialize them in codegen so
            // a later spread observes the same field as explicit None.
            let actual = self.default_record_fields(
                Ty::Record(actual_fields, None),
                expected.clone(),
                expression.region,
            )?;
            self.check_value(actual, expected.clone(), expression.region)?;
            return Ok(self.prune(expected));
        }
        // Fresh arrays have no pre-existing aliases. Check each element against
        // the annotation rather than treating construction as a conversion of
        // an already-shared invariant container.
        if let Expr::Array(items) = expression.value {
            if matches!(self.prune(expected.clone()), Ty::Var(_)) {
                let item = self.fresh();
                self.unify(
                    expected.clone(),
                    self.named("Array", vec![item]),
                    expression.region,
                )?;
            }
            let expected = self.prune(expected);
            if let Ty::App(head, args) = &expected
                && **head == self.named("Array", Vec::new())
                && args.len() == 1
            {
                let mut reachable = true;
                for item in items {
                    self.with_reachability(reachable, |this| {
                        this.infer_checked_expr(env, item, args[0].clone(), return_type.clone())
                    })?;
                    reachable &= alder_ast::flow::expression(item).falls_through;
                }
                return Ok(expected);
            }
            let actual = self.infer_expr(env, expression, return_type)?;
            self.check_value(actual, expected.clone(), expression.region)?;
            return Ok(self.prune(expected));
        }
        let actual = self.infer_expr(env, expression, return_type)?;
        // A freshly constructed Err introduces one error outcome, not an
        // invariant alias of an existing Result. Its tag must be included in
        // the contextual row; it need not enumerate every permitted outcome.
        if let Expr::Call { function, .. } = expression.value
            && is_result_err_expr(function)
            && self.result_parts(expected.clone()).is_some()
        {
            self.unify_return(actual, expected.clone(), expression.region)?;
            return Ok(self.prune(expected));
        }
        self.check_value(actual, expected.clone(), expression.region)?;
        Ok(self.prune(expected))
    }

    fn check_value(
        &mut self,
        actual: Ty<'a>,
        expected: Ty<'a>,
        region: Region,
    ) -> Result<(), Error> {
        self.unify(actual, expected, region)
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
                if !template.args.iter().zip(&args).all(|(template, goal)| {
                    match_type(template, goal, &mut bindings, self.database)
                }) {
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
                    .map(|(name, typ)| (name, self.normalize_type(typ)))
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
                    expectation: None,
                    region,
                    kind: ErrorKind::UnsupportedHigherKindedUnification,
                }));
            };
            if variable == head_var || !seen.insert(variable) {
                return Some(Err(Error {
                    expectation: None,
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
        mut left: BTreeMap<&'a str, Ty<'a>>,
        left_open: Option<Box<Ty<'a>>>,
        mut right: BTreeMap<&'a str, Ty<'a>>,
        right_open: Option<Box<Ty<'a>>>,
        region: Region,
    ) -> Result<(), Error> {
        for (name, left_type) in &left {
            match right.get(name) {
                Some(right_type) => {
                    self.unify(left_type.clone(), right_type.clone(), region)?;
                }
                None if right_open.is_none() => {
                    return Err(Error {
                        expectation: None,
                        region,
                        kind: ErrorKind::MissingField {
                            field: (*name).to_owned(),
                            available: right.keys().map(|name| (*name).to_owned()).collect(),
                        },
                    });
                }
                None => {}
            }
        }
        for name in right.keys() {
            if !left.contains_key(name) && left_open.is_none() {
                return Err(Error {
                    expectation: None,
                    region,
                    kind: ErrorKind::MissingField {
                        field: (*name).to_owned(),
                        available: left.keys().map(|name| (*name).to_owned()).collect(),
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
                            expectation: None,
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
                    expectation: None,
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
                expectation: None,
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
                fields.values().any(|typ| self.occurs(needle, typ))
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
                for typ in fields.values() {
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
                    .map(|(name, typ)| (*name, self.replace_vars(typ, replacements)))
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
        for shape in &scheme.tuple_shapes {
            self.collect_kind_arities(&shape.tuple, &mut arities);
            for element in shape.elements.values() {
                self.collect_kind_arities(element, &mut arities);
            }
        }
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
        let tuple_shapes = self
            .bump
            .alloc_slice_fill_iter(scheme.tuple_shapes.iter().map(|shape| {
                alder_ast::TupleShape {
                    tuple: self.to_ast(&shape.tuple, &mut names),
                    length: shape.length,
                    elements: self.bump.alloc_slice_fill_iter(
                        shape
                            .elements
                            .iter()
                            .map(|(index, typ)| (*index, self.to_ast(typ, &mut names))),
                    ),
                    region: shape.region,
                }
            }));
        let mut params = names.into_iter().collect::<Vec<_>>();
        params.retain(|(id, _)| scheme.quantified.contains(id));
        params.sort_by_key(|(_, name)| generated_type_name_rank(name));
        self.bump.alloc(Annotation {
            record_overlays,
            tuple_shapes,
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
                for typ in fields.values() {
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
                        |(index, (name, typ))| alder_ast::RecordTypeField {
                            index: index as u16,
                            name,
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
        let mut names = BTreeMap::new();
        Error {
            expectation: None,
            region,
            kind: ErrorKind::Mismatch {
                actual: self.diagnostic_type(actual, &mut names),
                expected: self.diagnostic_type(expected, &mut names),
            },
        }
    }

    fn diagnostic_type(
        &mut self,
        typ: Ty<'a>,
        names: &mut BTreeMap<usize, usize>,
    ) -> DiagnosticType {
        use DiagnosticType as D;
        match self.prune(typ) {
            Ty::Var(id) => {
                let next = names.len();
                D::Variable(*names.entry(id).or_insert(next))
            }
            Ty::Con(name) => D::Named(name.name.to_owned()),
            Ty::App(head, args) => D::Application(
                Box::new(self.diagnostic_type(*head, names)),
                args.into_iter()
                    .map(|arg| self.diagnostic_type(arg, names))
                    .collect(),
            ),
            Ty::Fn(args, ret) => D::Function(
                args.into_iter()
                    .map(|arg| self.diagnostic_type(arg, names))
                    .collect(),
                Box::new(self.diagnostic_type(*ret, names)),
            ),
            Ty::Unit => D::Unit,
            Ty::Tuple(items) => D::Tuple(
                items
                    .into_iter()
                    .map(|item| self.diagnostic_type(item, names))
                    .collect(),
            ),
            Ty::RecordRow(row) => self.diagnostic_type(*row, names),
            Ty::Record(fields, tail) => D::Record(
                fields
                    .into_iter()
                    .map(|(name, typ)| (name.to_owned(), self.diagnostic_type(typ, names)))
                    .collect(),
                tail.map(|tail| Box::new(self.diagnostic_type(*tail, names))),
            ),
            Ty::Partial(reference, slots) => D::Application(
                Box::new(D::Named(reference.name.to_owned())),
                slots
                    .into_iter()
                    .map(|slot| match slot {
                        TySlot::Hole(_) => D::Hole,
                        TySlot::Fixed(typ) => self.diagnostic_type(typ, names),
                    })
                    .collect(),
            ),
            Ty::Projection(trait_, args, assoc) => D::Projection(
                Box::new(D::Application(
                    Box::new(D::Named(trait_.0.name.to_owned())),
                    args.into_iter()
                        .map(|arg| self.diagnostic_type(arg, names))
                        .collect(),
                )),
                assoc.name.to_owned(),
            ),
            Ty::ErrorRow { tags, tail } => D::ErrorRow(
                tags.into_iter()
                    .map(|(name, args)| {
                        (
                            name.to_owned(),
                            args.into_iter()
                                .map(|arg| self.diagnostic_type(arg, names))
                                .collect(),
                        )
                    })
                    .collect(),
                tail.map(|tail| Box::new(self.diagnostic_type(*tail, names))),
            ),
            Ty::Any => D::Hole,
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
                    .map(|(name, typ)| format!("{}: {}", name, self.render(typ)))
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
