use alder_ast::{ImplId, ImplOrigin, ItemKind, Module, ModuleId, PackageId};
use alder_can::{
    AttributeError, ErrorKind as CanErrorKind, ExprError, ImportError, ItemError, NameError,
    PatternError, StmtError, TypeError, WarningKind,
};
use alder_region::Region;
use alder_report::{Diagnostic, Source};
use alder_solve::{CoherenceError, SolveError, SolveTraitError};

mod type_names;

#[cfg(test)]
mod syntax_tests;

pub fn source_failure(source: Source, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(source, message)
        .with_code("alder::driver::source")
        .with_primary_label(Region::one(), "module source")
}

mod syntax;
pub use syntax::parse;

pub fn canonicalize(source: Source, error: &alder_can::Error<'_>) -> Diagnostic {
    let (code, message, help, secondary) = match &error.kind {
        CanErrorKind::Import(error) => import_error(error),
        CanErrorKind::Item(error) => item_error(error),
        CanErrorKind::Type(error) => type_error(error),
        CanErrorKind::Pattern(error) => pattern_error(error),
        CanErrorKind::Expr(error) => expression_error(error),
        CanErrorKind::Stmt(error) => statement_error(error),
        CanErrorKind::Attribute(error) => attribute_error(error),
    };
    let primary_label = if matches!(error.kind, CanErrorKind::Type(TypeError::OrphanImpl(_))) {
        "this package owns neither side of the implementation"
    } else {
        "reported here"
    };
    let mut diagnostic = Diagnostic::error(source, message)
        .with_code(format!("alder::canonicalize::{code}"))
        .with_primary_label(error.region, primary_label);
    if let Some((region, label)) = secondary {
        diagnostic = diagnostic.with_secondary_label(region, label);
    }
    if let Some(help) = help {
        diagnostic = diagnostic.with_help(help);
    }
    diagnostic
}

pub fn warning(source: Source, warning: &alder_can::Warning<'_>) -> Diagnostic {
    let (code, message) = match warning.kind {
        WarningKind::UnusedImport { name: "*" } => (
            "unused_import",
            "no names from this wildcard import are referenced directly".to_owned(),
        ),
        WarningKind::UnusedImport { name } => {
            ("unused_import", format!("unused import binding `{name}`"))
        }
        WarningKind::UnusedBinding { name, .. } => {
            ("unused_binding", format!("unused binding `{name}`"))
        }
        WarningKind::UnusedTypeParameter { name } => (
            "unused_type_parameter",
            format!("unused type parameter `{name}`"),
        ),
    };
    let diagnostic = Diagnostic::warning(source, message)
        .with_code(format!("alder::warning::{code}"))
        .with_primary_label(warning.region, "not used");
    if let WarningKind::UnusedBinding { form, .. } = warning.kind {
        diagnostic.with_help(match form {
            alder_can::BindingForm::Declaration => "this private declaration is not reachable from an export, entry point, or evaluated initializer; remove it only if it is not needed",
            alder_can::BindingForm::Pattern => "if this binding is intentionally unused, discard it with `_`; keep any initializer whose effects are needed",
            alder_can::BindingForm::ArrayRest => "use an unnamed rest pattern `..` if the remaining elements are intentionally unused",
            alder_can::BindingForm::Alias => "remove the unused `as` binding, keeping the underlying pattern and any needed initializer effects",
        })
    } else if matches!(warning.kind, WarningKind::UnusedImport { .. }) {
        diagnostic.with_help("this imported name is not referenced directly; before removing the import, check whether the module is needed for initialization effects or trait instances")
    } else {
        diagnostic
    }
}

pub fn solve(
    source: Source,
    module: &Module<'_>,
    interfaces: &[alder_ast::Interface<'_>],
    error: &SolveError<'_>,
) -> Diagnostic {
    match error {
        SolveError::Core(error) => {
            constrain(source, &type_names::localize(module, interfaces, error))
        }
        SolveError::Trait(error) => trait_error(source, module, interfaces, error),
        SolveError::Coherence(error) => coherence(source, module, error),
    }
}

pub fn codegen(source: Source, error: &alder_codegen::Error) -> Diagnostic {
    Diagnostic::error(source, error.message)
        .with_code("alder::codegen")
        .with_primary_label(error.region, "code generation failed here")
}

fn constrain(source: Source, error: &alder_constrain::Error) -> Diagnostic {
    use alder_constrain::ErrorKind;
    match &error.kind {
        ErrorKind::InvalidResultErrorType { actual } => {
            return Diagnostic::error(
                source,
                format!("Result needs an error row, but this type is `{actual}`"),
            )
            .with_code("alder::type::invalid_result_error_type")
            .with_primary_label(error.region, "this is not an error row or named error group")
            .with_help("use a tagged error row such as `[:failed(String)]`, or declare a named error group");
        }
        ErrorKind::RecursiveErrorGroup { name } => {
            return Diagnostic::error(
                source,
                format!("the error group `{name}` expands back into itself"),
            )
            .with_code("alder::type::recursive_error_group")
            .with_primary_label(error.region, "this reference creates a recursive error row")
            .with_help("error groups describe structural rows; use an enum for a recursive payload instead");
        }
        ErrorKind::TupleIndexOutOfBounds { index, length } => {
            let help = if *length == 0 {
                "this tuple has no elements to access".to_owned()
            } else {
                format!(
                    "tuple indices start at zero; valid indices are 0 through {}",
                    length - 1
                )
            };
            return Diagnostic::error(
                source,
                format!("index {index} is outside this {length}-element tuple"),
            )
            .with_code("alder::type::tuple_index_out_of_bounds")
            .with_primary_label(error.region, "there is no element at this index")
            .with_help(help);
        }
        ErrorKind::MissingReturn { expected } => {
            let diagnostic = Diagnostic::error(source, format!("this function can finish without returning `{expected}`"))
                .with_code("alder::type::missing_return")
                .with_primary_label(error.region, "a path reaches the end without a value")
                .with_help("add a final expression or return a value on every path; a while or for loop may run zero times");
            return with_expectation_origin(diagnostic, error);
        }
        ErrorKind::AmbiguousOptionLifting => {
            return Diagnostic::error(source, "I cannot choose consistent Option wrapping for these values")
                .with_code("alder::type::ambiguous_option_lifting")
                .with_primary_label(error.region, "the intended Option layers are ambiguous here")
                .with_help("add a type annotation or explicit Some wrappers to make the intended Option layers clear");
        }
        ErrorKind::UnresolvedSharedExport { name } => {
            return Diagnostic::error(source, format!("the shared type of `{name}` is not determined"))
                .with_code("alder::type::unresolved_shared_export")
                .with_primary_label(error.region, "this export cannot be independently instantiated by each importing module")
                .with_help("give the shared value a concrete type annotation, or export a function that creates a fresh value on each call");
        }
        ErrorKind::GenericSpecialization {
            variable,
            restriction,
        } => {
            use alder_constrain::GenericRestriction as R;
            let (requirement, help) = match restriction {
                R::Type(actual) => (
                    format!("the signature promises an independent `{variable}`, but the body requires `{actual}`"),
                    format!("the caller chooses `{variable}` for each call; the body must work for every type allowed by the declared bounds, not just `{actual}`. Check the operation or value that imposes this restriction; a trait implementation must preserve the trait's generic contract"),
                ),
                R::SameVariable(other) => (
                    format!("the body requires `{variable}` and `{other}` to be the same type"),
                    format!("`{variable}` and `{other}` are independently chosen by the caller. Separate names do not promise equal types; check the values that the body combines or returns"),
                ),
                R::ResultRow(row) => (
                    format!("the body ties `{variable}` to the declared result row `{row}`"),
                    "independently named record rows may contain different fields and field types. The result cannot promise that two independent rows are equal".to_owned(),
                ),
                R::RecordField(field) => (
                    format!("the body constrains field `{field}` inside the independent row `{variable}`"),
                    format!("the declared row does not promise a particular type for `{field}`. Check which spread operand supplies this field and the result type it must satisfy"),
                ),
            };
            return Diagnostic::error(
                source,
                format!("this implementation does not work for every `{variable}`"),
            )
            .with_code("alder::type::generic_specialization")
            .with_primary_label(error.region, requirement)
            .with_help(help);
        }
        ErrorKind::GenericEscape { variable } => {
            return Diagnostic::error(
                source,
                format!("generic type `{variable}` is tied to a value outside this function"),
            )
            .with_code("alder::type::generic_escape")
            .with_primary_label(
                error.region,
                "this signature cannot be independently instantiated",
            )
            .with_help(
                "each call may choose a different type, but the captured value has one shared type across calls. Check the captured value and the operation that ties it to this signature; an annotation cannot make shared storage independently polymorphic",
            );
        }
        ErrorKind::NonExhaustiveMatch { missing } => {
            return Diagnostic::error(source, "this match does not cover every possible value")
                .with_code("alder::type::non_exhaustive_match")
                .with_primary_label(error.region, format!("uncovered patterns include {}", missing.join(", ")))
                .with_help("add the missing cases or a fallback arm; guarded patterns and pins do not guarantee coverage");
        }
        ErrorKind::RefutableBindingPattern { missing } => {
            return Diagnostic::error(source, "this binding pattern can fail")
                .with_code("alder::type::refutable_binding_pattern")
                .with_primary_label(
                    error.region,
                    format!("this pattern does not cover {}", missing.join(", ")),
                )
                .with_help(
                    "bind the value to a name, then use an exhaustive match to handle its cases",
                );
        }
        ErrorKind::RedundantPattern { covering } => {
            if covering.is_empty() {
                return Diagnostic::error(source, "this pattern cannot match a value of this type")
                    .with_code("alder::type::redundant_pattern")
                    .with_primary_label(error.region, "this case is impossible")
                    .with_help("remove this impossible case");
            }
            let mut diagnostic = Diagnostic::error(source, "this pattern is already covered")
                .with_code("alder::type::redundant_pattern")
                .with_primary_label(error.region, "earlier patterns already handle every value this can match")
                .with_help("remove this pattern or put the more specific case before the covering patterns");
            for region in covering.iter().take(4) {
                diagnostic = diagnostic.with_secondary_label(*region, "earlier coverage");
            }
            return diagnostic;
        }
        ErrorKind::ImpossibleErrorPattern { tag } => {
            return Diagnostic::error(
                source,
                format!("`:{tag}` is not part of this closed error row"),
            )
            .with_code("alder::type::impossible_error_pattern")
            .with_primary_label(error.region, "this pattern can never match")
            .with_help("check the tag spelling and the scrutinee's declared error row; use a pattern for a tag that this row permits");
        }
        ErrorKind::InvalidErrorTagPlacement => {
            return Diagnostic::error(source, "error tags are only values inside `Err`")
                .with_code("alder::type::invalid_error_tag_placement")
                .with_primary_label(error.region, "this tag is used as an ordinary value")
                .with_help("construct a Result error with `Err(:tag(...))`");
        }
        _ => {}
    }
    let (code, message) = match &error.kind {
        ErrorKind::Mismatch { actual, expected } => (
            "type_mismatch",
            format!("type mismatch: expected `{expected}`, found `{actual}`"),
        ),
        ErrorKind::Arity { expected, actual } => (
            "arity",
            format!("wrong number of arguments: expected {expected}, found {actual}"),
        ),
        ErrorKind::MissingField { field, .. } => {
            ("missing_field", format!("record has no field `{field}`"))
        }
        ErrorKind::RecordFieldsMismatch { actual, expected } => (
            "record_fields_mismatch",
            format!("record fields do not match: expected `{expected}`, found `{actual}`"),
        ),
        ErrorKind::AssocTypeMismatch {
            assoc,
            expected,
            actual,
        } => (
            "associated_type_mismatch",
            format!(
                "associated type `{assoc}` has conflicting equalities: expected `{expected}`, found `{actual}`"
            ),
        ),
        ErrorKind::InfiniteType { equation } => (
            "infinite_type",
            match equation {
                Some(equation) => format!(
                    "infinite type: `{}` would need to equal `{}`",
                    equation.0, equation.1
                ),
                None => "infinite type: a structural type would contain itself".to_owned(),
            },
        ),
        ErrorKind::UnsupportedHigherKindedUnification => (
            "higher_kinded_unification",
            "these higher-kinded types cannot be unified".to_owned(),
        ),
        ErrorKind::InvalidAwait => ("invalid_await", "`.await` requires a Task value".to_owned()),
        ErrorKind::InvalidTry => (
            "invalid_try",
            "`?` requires a Result or Option value and a matching return context".to_owned(),
        ),
        ErrorKind::ReturnMismatch => (
            "return_mismatch",
            "return value does not match the function result".to_owned(),
        ),
        ErrorKind::NonExhaustiveMatch { .. }
        | ErrorKind::RefutableBindingPattern { .. }
        | ErrorKind::RedundantPattern { .. }
        | ErrorKind::InvalidResultErrorType { .. }
        | ErrorKind::RecursiveErrorGroup { .. }
        | ErrorKind::TupleIndexOutOfBounds { .. }
        | ErrorKind::MissingReturn { .. }
        | ErrorKind::AmbiguousOptionLifting
        | ErrorKind::GenericSpecialization { .. }
        | ErrorKind::GenericEscape { .. }
        | ErrorKind::UnresolvedSharedExport { .. }
        | ErrorKind::ImpossibleErrorPattern { .. }
        | ErrorKind::InvalidErrorTagPlacement => unreachable!("handled above"),
    };
    let label = error
        .expectation
        .as_ref()
        .map(|expectation| {
            use alder_constrain::ExpectationKind as E;
            match &expectation.kind {
                E::Annotation => "this value does not match its annotation".to_owned(),
                E::AssociatedEquality => {
                    "this associated-type requirement conflicts with an earlier equality".to_owned()
                }
                E::Argument { position, callee } => match callee {
                    Some(callee) => {
                        format!("argument {position} of `{callee}` has an incompatible type")
                    }
                    None => format!("argument {position} has an incompatible type"),
                },
                E::Condition => "this condition must be Bool".to_owned(),
                E::Call { callee } => match callee {
                    Some(callee) => {
                        format!("this call to `{callee}` is incompatible with its signature")
                    }
                    None => "this call is incompatible with the function's signature".to_owned(),
                },
                E::Branch => "this branch has an incompatible result type".to_owned(),
                E::ArrayElement { position } => {
                    format!("array element {position} has an incompatible type")
                }
                E::Pattern => "this pattern does not match the value's type".to_owned(),
                E::Assignment => "this assignment must preserve the target's type".to_owned(),
                E::Return => "this return value has an incompatible type".to_owned(),
                E::Await => "await requires a Task value".to_owned(),
                E::Propagation => {
                    "this propagation requires compatible Option or Result types".to_owned()
                }
            }
        })
        .unwrap_or_else(|| match error.kind {
            ErrorKind::MissingField { .. } => "this field is not present in the record".to_owned(),
            _ => "these types are incompatible".to_owned(),
        });
    let mut diagnostic = Diagnostic::error(source, message)
        .with_code(format!("alder::type::{code}"))
        .with_primary_label(error.region, label);
    diagnostic = with_expectation_origin(diagnostic, error);
    if matches!(error.kind, ErrorKind::InfiniteType { .. }) {
        diagnostic = diagnostic.with_help("these requirements form a cycle: expanding the type would keep nesting it inside itself. Check the highlighted use and the types it connects; adding an annotation cannot make this structural cycle finite");
    }
    if let ErrorKind::RecordFieldsMismatch {
        actual: alder_constrain::DiagnosticType::Record(actual, actual_tail),
        expected: alder_constrain::DiagnosticType::Record(expected, expected_tail),
    } = &error.kind
    {
        let missing = expected
            .iter()
            .filter(|(name, _)| {
                actual_tail.is_none() && !actual.iter().any(|(actual, _)| actual == name)
            })
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        let extra = actual
            .iter()
            .filter(|(name, _)| {
                expected_tail.is_none() && !expected.iter().any(|(expected, _)| expected == name)
            })
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        let mut differences = Vec::new();
        if !missing.is_empty() {
            differences.push(format!("missing fields: `{}`", missing.join("`, `")));
        }
        if !extra.is_empty() {
            differences.push(format!("unexpected fields: `{}`", extra.join("`, `")));
        }
        if missing.len() == 1
            && extra.len() == 1
            && let Some(candidate) = nearest_field(&extra[0], &missing)
        {
            differences.push(format!(
                "did you mean `{candidate}` instead of `{}`?",
                extra[0]
            ));
        }
        diagnostic = diagnostic.with_help(differences.join("; "));
    }
    if let ErrorKind::MissingField { field, available } = &error.kind {
        let fields = available
            .iter()
            .take(8)
            .map(|name| format!("`{name}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let listing = if available.is_empty() {
            "this record has no fields".to_owned()
        } else {
            format!(
                "available fields: {fields}{}",
                if available.len() > 8 { ", …" } else { "" }
            )
        };
        let candidate = nearest_field(field, available);
        diagnostic = diagnostic.with_help(match candidate {
            Some(candidate) => format!("did you mean `{candidate}`? {listing}"),
            None => listing,
        });
    }
    diagnostic
}

fn with_expectation_origin(
    mut diagnostic: Diagnostic,
    error: &alder_constrain::Error,
) -> Diagnostic {
    if let Some(expectation) = &error.expectation
        && let Some(origin) = expectation.origin
        && origin != error.region
    {
        use alder_constrain::ExpectationKind as E;
        let label = match expectation.kind {
            E::Annotation => "the declared type",
            E::AssociatedEquality => "the earlier associated-type requirement",
            E::Return => "the declared return type",
            E::ArrayElement { .. } => "an earlier array element",
            E::Branch => "another branch in this expression",
            _ => "a related type requirement",
        };
        diagnostic = diagnostic.with_secondary_label(origin, label);
    }
    diagnostic
}

/// Suggest only a uniquely closest existing field, never a fabricated name.
fn nearest_field<'a>(field: &str, available: &'a [String]) -> Option<&'a str> {
    let mut ranked = available
        .iter()
        .map(|candidate| {
            let chars = candidate.chars().collect::<Vec<_>>();
            let mut row = (0..=chars.len()).collect::<Vec<_>>();
            for (index, character) in field.chars().enumerate() {
                let mut diagonal = row[0];
                row[0] = index + 1;
                for (column, other) in chars.iter().enumerate() {
                    let previous = row[column + 1];
                    row[column + 1] = (row[column] + 1)
                        .min(previous + 1)
                        .min(diagonal + usize::from(character != *other));
                    diagonal = previous;
                }
            }
            (row[chars.len()], candidate.as_str())
        })
        .collect::<Vec<_>>();
    ranked.sort_unstable();
    let (distance, candidate) = *ranked.first()?;
    let limit = if field.chars().count() <= 3 { 1 } else { 2 };
    (distance <= limit && ranked.get(1).is_none_or(|(next, _)| *next > distance))
        .then_some(candidate)
}

fn trait_error(
    source: Source,
    module: &Module<'_>,
    interfaces: &[alder_ast::Interface<'_>],
    error: &SolveTraitError<'_>,
) -> Diagnostic {
    let (trait_, args, chain) = match error {
        SolveTraitError::MissingInstance {
            trait_,
            args,
            chain,
            ..
        }
        | SolveTraitError::UnsatisfiedBound {
            trait_,
            args,
            chain,
            ..
        }
        | SolveTraitError::AmbiguousTypeVariable {
            trait_,
            args,
            chain,
            ..
        }
        | SolveTraitError::InstanceCycle {
            trait_,
            args,
            chain,
            ..
        } => (*trait_, args.as_ref(), chain.as_ref()),
        SolveTraitError::AmbiguousInstance {
            trait_,
            args,
            details,
            ..
        } => (*trait_, args.as_ref(), details.chain.as_ref()),
    };
    let mut goals = std::iter::once(trait_goal_type(trait_, args))
        .chain(
            chain
                .iter()
                .map(|frame| trait_goal_type(frame.trait_, &frame.args)),
        )
        .collect::<Vec<_>>();
    type_names::localize_types(module, interfaces, goals.iter_mut());
    let goal = &goals[0];
    let chain_goals = &goals[1..];
    match error {
        SolveTraitError::MissingInstance {
            origin,
            ..
        } => Diagnostic::error(
            source,
            format!("no implementation of `{goal}` was found"),
        )
        .with_code("alder::trait::missing_instance")
        .with_primary_label(*origin, "this use needs trait evidence")
        .with_help(with_obligation_chain(
            if trait_ == alder_solve::builtin_trait_id("Eq")
                && matches!(args, [alder_constrain::DiagnosticType::Function(..)])
            {
                "functions cannot be compared for equality; compare the data that represents what you need to distinguish instead".to_owned()
            } else {
                format!("check the operand type and the implementations available for `{goal}`; a new implementation must satisfy Alder's implementation-head and ownership rules")
            },
            chain,
            chain_goals,
        )),
        SolveTraitError::AmbiguousInstance {
            origin,
            details,
            ..
        } => {
            let candidates = &details.candidates;
            let mut diagnostic = Diagnostic::error(
                source,
                format!(
                    "multiple implementations of `{goal}` match ({} candidates)",
                    candidates.len()
                ),
            )
            .with_code("alder::trait::ambiguous_instance")
            .with_primary_label(*origin, "the implementation cannot be selected here");
            for (index, candidate) in candidates.iter().enumerate() {
                if let Some(region) = local_impl_region(module, *candidate) {
                    diagnostic = diagnostic.with_secondary_label(
                        region,
                        format!("candidate implementation {}", index + 1),
                    );
                }
            }
            let candidates = candidates
                .iter()
                .enumerate()
                .map(|(index, candidate)| {
                    let availability = if local_impl_region(module, *candidate).is_none() {
                        " (source unavailable)"
                    } else {
                        ""
                    };
                    format!(
                        "  {}. {}{availability}",
                        index + 1,
                        impl_description(*candidate)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            diagnostic.with_help(with_obligation_chain(
                format!(
                    "the matching implementations must be made unambiguous; a type annotation cannot choose between implementations of the same concrete goal. Candidates:\n{candidates}"
                ),
                chain,
                chain_goals,
            ))
        }
        SolveTraitError::UnsatisfiedBound {
            origin,
            ..
        } => Diagnostic::error(
            source,
            format!("the generic contract does not provide `{goal}`"),
        )
        .with_code("alder::trait::unsatisfied_bound")
        .with_primary_label(*origin, "this use requires a bound")
        .with_help(with_obligation_chain(
            format!(
                "this operation requires `{goal}` in the generic contract; an implementation method cannot require stronger bounds than its trait method"
            ),
            chain,
            chain_goals,
        )),
        SolveTraitError::AmbiguousTypeVariable {
            origin,
            ..
        } => Diagnostic::error(
            source,
            format!(
                "cannot determine the types required by `{goal}`"
            ),
        )
        .with_code("alder::trait::ambiguous_type_variable")
        .with_primary_label(*origin, "this trait use has no determining type")
        .with_help(with_obligation_chain(
            "add a type annotation that fixes the operand type".to_owned(),
            chain,
            chain_goals,
        )),
        SolveTraitError::InstanceCycle {
            origin,
            ..
        } => Diagnostic::error(
            source,
            format!(
                "resolving `{goal}` forms an instance cycle"
            ),
        )
        .with_code("alder::trait::instance_cycle")
        .with_primary_label(*origin, "instance resolution returns to this requirement")
        .with_help(with_obligation_chain(
            "make the instance prerequisites structurally decrease".to_owned(),
            chain,
            chain_goals,
        )),
    }
}

fn trait_goal_type(
    trait_: alder_ast::TraitId<'_>,
    args: &[alder_constrain::DiagnosticType],
) -> alder_constrain::DiagnosticType {
    use alder_constrain::DiagnosticType as D;
    let head = if trait_.0.module.package == alder_ast::PackageId::Builtin
        && trait_.0.module.path.is_empty()
    {
        D::Named(trait_.0.name.to_owned())
    } else {
        D::NamedReference(Box::new(trait_.0.into()))
    };
    D::Application(Box::new(head), args.to_vec())
}

fn with_obligation_chain(
    help: String,
    chain: &[alder_solve::ObligationFrame<'_>],
    goals: &[alder_constrain::DiagnosticType],
) -> String {
    if chain.len() < 2 {
        return help;
    }
    let chain = chain
        .iter()
        .zip(goals)
        .map(|(frame, goal)| {
            let goal = goal.to_string();
            frame.required_by.map_or(goal.clone(), |implementation| {
                format!("{goal}, required by {}", impl_description(implementation))
            })
        })
        .collect::<Vec<_>>()
        .join("\n  -> ");
    format!("{help}\n\nobligation chain:\n  {chain}")
}

fn coherence(source: Source, module: &Module<'_>, error: &CoherenceError<'_>) -> Diagnostic {
    coherence_report(source, module, error, true)
}

pub(crate) fn dependency_coherence(module: &Module<'_>, error: &CoherenceError<'_>) -> Diagnostic {
    let source = Source::new("dependency trait registry", "");
    let modules = error
        .modules()
        .into_iter()
        .map(|module| format!("`{}`", module_name(module)))
        .collect::<Vec<_>>()
        .join(", ");
    coherence_report(source.clone(), module, error, false).with_related(Diagnostic::advice(
        source,
        format!("these definitions come from dependency modules: {modules}"),
    ))
}

fn coherence_report(
    source: Source,
    module: &Module<'_>,
    error: &CoherenceError<'_>,
    show_spans: bool,
) -> Diagnostic {
    let (code, message, primary, primary_label, secondary, help) = match error {
        CoherenceError::SuperclassCycle { traits } => (
            "superclass_cycle",
            format!(
                "trait superclass cycle: {}",
                traits
                    .iter()
                    .map(|trait_| trait_.0.name)
                    .collect::<Vec<_>>()
                    .join(" -> ")
            ),
            traits
                .iter()
                .rev()
                .find_map(|trait_| local_trait_region(module, *trait_))
                .unwrap_or_else(Region::one),
            "this trait participates in the superclass cycle",
            None,
            Some("remove one of the superclass constraints in this cycle".to_owned()),
        ),
        CoherenceError::NamedErrorGroupImpl { implementation, group } => (
            "named_error_group_impl",
            format!("cannot define a custom trait implementation for error group `{}`", group.name),
            impl_region(module, *implementation),
            "this error group names a structural row, not a distinct type",
            None,
            Some("define an enum wrapper if you need a distinct type with custom trait behavior".to_owned()),
        ),
        CoherenceError::OrphanImpl {
            implementation,
            trait_,
            subject,
            trait_package,
            type_package,
        } => (
            "orphan_impl",
            format!(
                "orphan implementation `{}[{subject}]`: this package defines neither the trait ({}) nor the subject type ({})",
                trait_.0.name,
                package_name(*trait_package),
                type_package
                    .map(package_name)
                    .unwrap_or("no owning package")
            ),
            impl_region(module, *implementation),
            "this package owns neither side of the implementation",
            None,
            Some(
                "define either the trait or subject type in this package, or move the impl to a package that does"
                    .to_owned(),
            ),
        ),
        CoherenceError::OverlappingImpl {
            first,
            second,
            trait_,
        } => {
            let first_region = local_impl_region(module, *first);
            let second_region = local_impl_region(module, *second);
            let other_module = if first_region.is_some() { second.module } else { first.module };
            let mut help = "remove one impl or introduce a distinct wrapper type; Alder has no specialization".to_owned();
            if show_spans && (first_region.is_none() || second_region.is_none()) {
                help.push_str(&format!(
                    "; the other implementation is in module `{}`",
                    module_name(other_module),
                ));
            }
            (
                "overlapping_impl",
                format!(
                    "overlapping implementations of `{}` are not allowed",
                    trait_.0.name
                ),
                second_region.or(first_region).unwrap_or_else(Region::one),
                if first_region.is_some() && second_region.is_some() {
                    "this implementation overlaps the first"
                } else {
                    "this implementation overlaps one in another module"
                },
                first_region.zip(second_region).map(|(region, _)| (region, "first implementation is here")),
                Some(help),
            )
        },
        CoherenceError::InvalidTermination {
            implementation,
            prerequisite,
        } => (
            "invalid_termination",
            format!(
                "instance prerequisite `{}` does not structurally decrease",
                prerequisite.0.name
            ),
            impl_region(module, *implementation),
            "this prerequisite does not get structurally smaller",
            None,
            Some("make every recursive instance prerequisite structurally smaller than its impl head".to_owned()),
        ),
        CoherenceError::KindMismatch {
            implementation,
            parameter,
            expected_arity,
            actual_arity,
        } => (
            "kind_mismatch",
            format!(
                "trait argument {} must be {}, but this is {}",
                parameter + 1,
                kind_description(*expected_arity),
                kind_description(*actual_arity)
            ),
            impl_region(module, *implementation),
            "this trait argument has the wrong kind",
            None,
            Some("use a type constructor with the arity required by the trait".to_owned()),
        ),
        CoherenceError::ProjectionCycle {
            implementation,
            chain,
        } => (
            "projection_cycle",
            format!(
                "associated type cycle: {} -> {}",
                chain
                    .iter()
                    .map(|assoc| assoc.name)
                    .collect::<Vec<_>>()
                    .join(" -> "),
                chain.first().map_or("associated type", |assoc| assoc.name)
            ),
            impl_region(module, *implementation),
            "these associated type definitions form a cycle",
            None,
            Some("make at least one associated type resolve to a non-cyclic type".to_owned()),
        ),
    };
    let mut diagnostic =
        Diagnostic::error(source, message).with_code(format!("alder::trait::{code}"));
    if show_spans {
        diagnostic = diagnostic.with_primary_label(primary, primary_label);
    }
    if show_spans && let Some((region, label)) = secondary {
        diagnostic = diagnostic.with_secondary_label(region, label);
    }
    if let Some(help) = help {
        diagnostic = diagnostic.with_help(help);
    }
    diagnostic
}

fn kind_description(arity: u16) -> String {
    match arity {
        0 => "a concrete type".to_owned(),
        1 => "a one-argument type constructor".to_owned(),
        arity => format!("a {arity}-argument type constructor"),
    }
}

type CanDetails = (
    &'static str,
    String,
    Option<String>,
    Option<(Region, &'static str)>,
);

fn name_error(error: &NameError<'_>) -> CanDetails {
    match error {
        NameError::Unknown {
            qualifier,
            name,
            suggestions,
            ..
        } => {
            let qualified = qualifier.map_or_else(|| (*name).to_owned(), |q| format!("{q}.{name}"));
            let help = (!suggestions.is_empty()).then(|| {
                format!(
                    "did you mean {}?",
                    suggestions
                        .iter()
                        .map(|s| format!("`{s}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            });
            (
                "unknown_name",
                format!("unknown name `{qualified}`"),
                help,
                None,
            )
        }
        NameError::Ambiguous {
            name, candidates, ..
        } => (
            "ambiguous_name",
            format!("`{name}` is ambiguous ({} candidates)", candidates.len()),
            Some("qualify the name with its module".to_owned()),
            None,
        ),
        NameError::Private { name, .. } => {
            ("private_name", format!("`{name}` is private"), None, None)
        }
    }
}

fn import_error(error: &ImportError<'_>) -> CanDetails {
    match error {
        ImportError::Name(error) => name_error(error),
        ImportError::ModuleNotFound { module } => (
            "import_module_not_found",
            format!("cannot find imported module `{}`", module_name(*module)),
            Some(match module.package {
                PackageId::Builtin => "bare paths refer only to bundled standard-library modules; use `~/` for a module in this package".to_owned(),
                _ => "check the module path and that its package is available to this build".to_owned(),
            }),
            None,
        ),
        ImportError::NameNotFound {
            name, available, ..
        } => (
            "import_name_not_found",
            format!("the imported module does not export `{name}`"),
            (!available.is_empty()).then(|| format!("available names: {}", available.join(", "))),
            None,
        ),
        ImportError::AliasCollision { name, first } => (
            "import_alias_collision",
            format!("import name `{name}` is already bound"),
            None,
            Some((*first, "first binding is here")),
        ),
        ImportError::ReexportPrivate { name, .. } => (
            "reexport_private",
            format!("cannot re-export private name `{name}`"),
            None,
            None,
        ),
    }
}

fn item_error(error: &ItemError<'_>) -> CanDetails {
    match error {
        ItemError::DuplicateDefinition { name, first, .. } => (
            "duplicate_definition",
            format!("`{name}` is defined more than once"),
            None,
            Some((*first, "first definition is here")),
        ),
        ItemError::RecursiveValue { name, cycle } => (
            "recursive_value",
            format!(
                "value `{name}` is recursively defined through {}",
                cycle.join(" -> ")
            ),
            None,
            None,
        ),
        ItemError::RecursiveAlias { name, cycle } => (
            "recursive_alias",
            format!(
                "type alias `{name}` is recursive through {}",
                cycle.join(" -> ")
            ),
            None,
            None,
        ),
        ItemError::AnnotationTooShort {
            name,
            annotated,
            parameters,
        } => (
            "annotation_too_short",
            format!(
                "annotation for `{name}` covers {annotated} arguments but the function has {parameters}"
            ),
            None,
            None,
        ),
    }
}

fn type_error(error: &TypeError<'_>) -> CanDetails {
    match error {
        TypeError::Name(error) => name_error(error),
        TypeError::RecursiveAlias { name } => (
            "recursive_alias",
            format!("type alias `{name}` refers back to itself"),
            Some("an alias must expand to a finite type; use an enum to represent recursive data".to_owned()),
            None,
        ),
        TypeError::InvalidAliasArgument => (
            "invalid_alias_argument",
            "this alias argument cannot be used in the required type or row position".to_owned(),
            Some("check the argument's kind and any overlapping row fields".to_owned()),
            None,
        ),
        TypeError::BadArity {
            name,
            expected,
            actual,
        } => (
            "type_arity",
            format!("type `{name}` expects {expected} arguments, found {actual}"),
            None,
            None,
        ),
        TypeError::DuplicateParameter { name, first } => duplicate("type parameter", name, *first),
        TypeError::DuplicateField { name, first } => duplicate("record field", name, *first),
        TypeError::DuplicateTag { name, first } => duplicate("tag", name, *first),
        TypeError::UnboundVariable { name } => (
            "unbound_type_variable",
            format!("unbound type variable `{name}`"),
            None,
            None,
        ),
        TypeError::UnusedParameter { name } => (
            "unused_type_parameter",
            format!("unused type parameter `{name}`"),
            None,
            None,
        ),
        TypeError::MissingAnnotation { name, position } => {
            let message = match *position {
                "parameter" => format!("every parameter of `{name}` needs a type annotation"),
                "return type" => format!("`{name}` needs a return type annotation"),
                position => format!("`{name}` needs a type annotation for its {position}"),
            };
            ("missing_annotation", message, None, None)
        }
        TypeError::UnknownImplItem {
            trait_name,
            name,
            item_kind,
        } => (
            "unknown_impl_item",
            format!("{item_kind} `{name}` is not a member of trait `{trait_name}`"),
            None,
            None,
        ),
        TypeError::MissingImplItem {
            trait_name,
            name,
            item_kind,
        } => (
            "missing_impl_item",
            format!("impl of `{trait_name}` is missing {item_kind} `{name}`"),
            None,
            None,
        ),
        TypeError::UnknownAssocType { name } => (
            "unknown_associated_type",
            format!("unknown associated type `{name}`"),
            None,
            None,
        ),
        TypeError::AmbiguousAssocType { name, traits } => (
            "ambiguous_associated_type",
            format!(
                "associated type `{name}` is declared by {} traits",
                traits.len()
            ),
            Some("qualify the associated type through its trait".to_owned()),
            None,
        ),
        TypeError::OrphanImpl(details) => (
            "orphan_impl",
            format!(
                "orphan implementation of `{trait_name}[{subject}]`: this package owns neither `{trait_name}` ({}) nor `{subject}` ({})",
                package_name(details.trait_package),
                details.type_package
                    .map(package_name)
                    .unwrap_or("no owning package"),
                trait_name = details.trait_name,
                subject = details.subject,
            ),
            Some(
                "define either the trait or subject type in this package, or move the impl to a package that does"
                    .to_owned(),
            ),
            None,
        ),
        TypeError::InvalidHole => (
            "invalid_type_hole",
            "`_` is only allowed in a partially-applied impl head".to_owned(),
            None,
            None,
        ),
    }
}

fn pattern_error(error: &PatternError<'_>) -> CanDetails {
    match error {
        PatternError::AlternativeBindings { expected, actual } => (
            "alternative_bindings",
            "match alternatives must bind the same names".to_owned(),
            Some(format!(
                "the first alternative binds [{}]; this one binds [{}]",
                expected.join(", "),
                actual.join(", ")
            )),
            None,
        ),
        PatternError::Name(error) => name_error(error),
        PatternError::DuplicateBinding { name, first } => {
            duplicate("pattern binding", name, *first)
        }
        PatternError::ConstructorArity {
            name,
            expected,
            actual,
        } => (
            "constructor_arity",
            format!(
                "constructor `{}::{}` expects {expected} values, found {actual}",
                name.enum_name, name.variant
            ),
            None,
            None,
        ),
        PatternError::ConstructorPayload {
            name,
            expected,
            actual,
        } => (
            "constructor_payload",
            format!(
                "constructor `{}::{}` has a {expected} payload, not {actual}",
                name.enum_name, name.variant
            ),
            None,
            None,
        ),
        PatternError::DuplicateField { name, first } => duplicate("pattern field", name, *first),
        PatternError::PinOutsideMatch => (
            "pin_outside_match",
            "pin patterns are only allowed in `match`".to_owned(),
            None,
            None,
        ),
    }
}

fn expression_error(error: &ExprError<'_>) -> CanDetails {
    match error {
        ExprError::Name(error) => name_error(error),
        ExprError::UnqualifiedConstructor { enum_name, variant } => (
            "unqualified_constructor",
            format!("constructor `{variant}` must be written `{enum_name}::{variant}`"),
            None,
            None,
        ),
        ExprError::PlaceholderOutsideCall => (
            "placeholder_outside_call",
            "`_` placeholders are only allowed in call arguments".to_owned(),
            None,
            None,
        ),
        ExprError::PinOutsideQuery => (
            "pin_outside_query",
            "expression pins are only allowed in queries".to_owned(),
            None,
            None,
        ),
        ExprError::AwaitOutsideAsync => (
            "await_outside_async",
            "`.await` needs an enclosing async body".to_owned(),
            Some("use `async fn` or an `async { ... }` block".to_owned()),
            None,
        ),
        ExprError::MacroUnavailable { name } => (
            "macro_unavailable",
            format!("macro `{name}` is not available yet"),
            None,
            None,
        ),
        ExprError::DuplicateField { name, first } => duplicate("record field", name, *first),
        ExprError::NonAssociativeOperators { left, right } => (
            "non_associative_operators",
            format!("operators `{left}` and `{right}` cannot be chained without parentheses"),
            None,
            None,
        ),
    }
}

fn statement_error(error: &StmtError<'_>) -> CanDetails {
    match error {
        StmtError::Name(error) => name_error(error),
        StmtError::NonAssignableBinding { name, binding } => (
            "non_assignable_binding",
            format!("`{name}` does not name an assignable binding"),
            Some("use a local `let` binding when you need to replace a value".to_owned()),
            Some((*binding, "binding declared here")),
        ),
        StmtError::InvalidAssignmentTarget => (
            "invalid_assignment_target",
            "invalid assignment target".to_owned(),
            None,
            None,
        ),
        StmtError::BreakOutsideLoop => (
            "break_outside_loop",
            "`break` is only allowed inside a loop".to_owned(),
            None,
            None,
        ),
        StmtError::ContinueOutsideLoop => (
            "continue_outside_loop",
            "`continue` is only allowed inside a loop".to_owned(),
            None,
            None,
        ),
        StmtError::ReturnOutsideFunction => (
            "return_outside_function",
            "`return` is only allowed inside a function".to_owned(),
            None,
            None,
        ),
    }
}

fn attribute_error(error: &AttributeError<'_>) -> CanDetails {
    match error {
        AttributeError::InvalidExtern { reason } => (
            "invalid_extern",
            format!("invalid extern attribute: {reason}"),
            None,
            None,
        ),
        AttributeError::InvalidDerive { reason } => (
            "invalid_derive",
            format!("invalid derive attribute: {reason}"),
            None,
            None,
        ),
        AttributeError::Unknown { name } => (
            "unknown_attribute",
            format!("unknown attribute `{name}`"),
            None,
            None,
        ),
        AttributeError::MacroUnavailable => (
            "attribute_macro_unavailable",
            "attribute macros are not available yet".to_owned(),
            None,
            None,
        ),
    }
}

fn duplicate(kind: &'static str, name: &str, first: Region) -> CanDetails {
    (
        "duplicate",
        format!("duplicate {kind} `{name}`"),
        None,
        Some((first, "first declared here")),
    )
}

fn impl_region(module: &Module<'_>, implementation: ImplId<'_>) -> Region {
    local_impl_region(module, implementation).unwrap_or_else(Region::one)
}

fn local_trait_region(module: &Module<'_>, trait_: alder_ast::TraitId<'_>) -> Option<Region> {
    (trait_.0.module == module.id).then_some(())?;
    module.items.iter().find_map(|item| match item.value.kind {
        ItemKind::Trait(declaration) if declaration.id == trait_ => Some(item.region),
        _ => None,
    })
}

fn local_impl_region(module: &Module<'_>, implementation: ImplId<'_>) -> Option<Region> {
    if implementation.module != module.id {
        return None;
    }
    // Origins retain source ordinals, but canonical items omit imports (and
    // header-only modules omit value declarations). Generated implementations
    // also carry their originating declaration's region, so identity lookup
    // works for source implementations and derives alike.
    module.items.iter().find_map(|item| match item.value.kind {
        ItemKind::Impl(declaration) if declaration.id == implementation => Some(item.region),
        _ => None,
    })
}

fn impl_description(implementation: ImplId<'_>) -> String {
    let module = module_name(implementation.module);
    if implementation.module.package == PackageId::Builtin {
        return "a standard-library implementation".to_owned();
    }
    match implementation.origin {
        ImplOrigin::Source { .. } => format!("a source implementation in `{module}`"),
        ImplOrigin::Derived { .. } => format!("a derived implementation in `{module}`"),
        ImplOrigin::AutomaticEq { .. } => {
            format!("an automatic Eq implementation in `{module}`")
        }
        ImplOrigin::Builtin { .. } => "a standard-library implementation".to_owned(),
    }
}

fn package_name(package: PackageId<'_>) -> &'static str {
    match package {
        PackageId::Named(_) => "a dependency package",
        PackageId::Application => "the application",
        PackageId::ApplicationMember(_) => "an application workspace member",
        PackageId::Builtin => "the standard library",
    }
}

fn module_name(module: ModuleId<'_>) -> String {
    let path = module.path.join("/");
    match module.package {
        PackageId::Named(package) if path.is_empty() => {
            format!("{}/{}", package.author, package.project)
        }
        PackageId::Named(package) => format!("{}/{}/{path}", package.author, package.project),
        PackageId::Application if path.is_empty() => "the application root".to_owned(),
        PackageId::Application => path,
        PackageId::ApplicationMember(member) if path.is_empty() => member.to_owned(),
        PackageId::ApplicationMember(member) => format!("{member}/{path}"),
        PackageId::Builtin if path.is_empty() => "the standard library".to_owned(),
        PackageId::Builtin => format!("standard library/{path}"),
    }
}
