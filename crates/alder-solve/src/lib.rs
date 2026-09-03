//! Alder type inference over the canonical AST.

mod inference;
mod traits;

use std::collections::BTreeMap;

use alder_ast::{
    DictionaryKind, ImplId, MethodId, ModuleId, PackageId, QualifiedName, TraitId, UseId,
};
use alder_can::Annotations;
use alder_region::Region;

pub use inference::{run, solve};
pub use traits::{CoherenceError, InstanceHeader, TraitDatabase, TraitHeader, builtin_trait_id};

#[derive(Clone, Debug)]
pub struct SolveOutput<'a> {
    pub annotations: Annotations<'a>,
    pub schemes: Annotations<'a>,
    pub bindings: BTreeMap<alder_ast::QualifiedName<'a>, BindingEvidence<'a>>,
    pub uses: BTreeMap<UseId, UseAction<'a>>,
    pub impl_superclasses: BTreeMap<(ImplId<'a>, u16), Evidence<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct BindingEvidence<'a> {
    pub dictionary_params: &'a [alder_ast::TraitRef<'a>],
    pub abi: BindingAbi,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingAbi {
    PlainValue,
    DirectFunction,
    EvidenceFactory,
}

#[derive(Clone, Debug)]
pub enum UseAction<'a> {
    Reference {
        dictionaries: Vec<Evidence<'a>>,
        method: Option<MethodId<'a>>,
    },
    DirectCall {
        callee_use: UseId,
        dictionaries: Vec<Evidence<'a>>,
        target: Option<DirectTarget<'a>>,
    },
    IndirectCall,
    Operator {
        dictionary: Evidence<'a>,
    },
    Pin {
        dictionary: Evidence<'a>,
    },
    CompoundAssign {
        dictionary: Evidence<'a>,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum DirectTarget<'a> {
    Binding(QualifiedName<'a>),
    TraitMethod(MethodId<'a>),
}

#[derive(Clone, Debug)]
pub enum Evidence<'a> {
    Param(u16),
    ParamSuper {
        param: u16,
        slot: u16,
    },
    ParamSuperPath {
        param: u16,
        path: Vec<u16>,
    },
    SelfDictionary,
    Super(u16),
    SuperPath(Vec<u16>),
    Impl {
        impl_id: ImplId<'a>,
        module: ModuleId<'a>,
        symbol: &'a str,
        kind: DictionaryKind,
        arguments: Vec<Evidence<'a>>,
    },
    Intrinsic(Intrinsic),
    StructuralEq(Vec<Evidence<'a>>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intrinsic {
    EqNumber,
    EqString,
    EqBool,
    EqBigInt,
    EqUnit,
    OrdNumber,
    OrdString,
    OrdBigInt,
    NumNumber,
    NumBigInt,
    FunctorArray,
    FunctorOption,
    FunctorResult,
    ShowKernel,
    HashKernel,
    JsonKernel,
    ApplicativeArray,
    ApplicativeOption,
    ApplicativeResult,
    MonadArray,
    MonadOption,
    MonadResult,
    TraversableArray,
    TraversableOption,
    TraversableResult,
    IteratorArray,
}

#[derive(Clone, Debug)]
pub enum SolveError<'a> {
    Core(alder_constrain::Error),
    Coherence(CoherenceError<'a>),
    Trait(SolveTraitError<'a>),
}

#[derive(Clone, Debug)]
pub enum SolveTraitError<'a> {
    MissingInstance {
        trait_: TraitId<'a>,
        subject: &'a str,
        origin: Region,
    },
    AmbiguousInstance {
        trait_: TraitId<'a>,
        subject: &'a str,
        origin: Region,
        candidates: &'a [ImplId<'a>],
    },
    UnsatisfiedBound {
        trait_: TraitId<'a>,
        subject: &'a str,
        origin: Region,
    },
    InstanceCycle {
        trait_: TraitId<'a>,
        subject: &'a str,
        origin: Region,
    },
}

/// Render inference failures for users without exposing Rust's internal
/// `Debug` representation. The driver owns source snippets and filenames; this
/// layer supplies the stable diagnostic title, location, and help text.
pub fn format_errors(errors: &[SolveError<'_>]) -> String {
    errors
        .iter()
        .map(format_error)
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_error(error: &SolveError<'_>) -> String {
    match error {
        SolveError::Core(error) => {
            let message = match &error.kind {
                alder_constrain::ErrorKind::Mismatch { actual, expected } => {
                    format!("type mismatch: expected `{expected}`, found `{actual}`")
                }
                alder_constrain::ErrorKind::Arity { expected, actual } => {
                    format!("wrong number of arguments: expected {expected}, found {actual}")
                }
                alder_constrain::ErrorKind::MissingField { field } => {
                    format!("record has no field `{field}`")
                }
                alder_constrain::ErrorKind::AssocTypeMismatch {
                    assoc,
                    expected,
                    actual,
                } => format!(
                    "associated type `{assoc}` has conflicting equalities: expected `{expected}`, found `{actual}`"
                ),
                alder_constrain::ErrorKind::InfiniteType => "infinite type".to_owned(),
                alder_constrain::ErrorKind::UnsupportedHigherKindedUnification => {
                    "these higher-kinded types cannot be unified".to_owned()
                }
                alder_constrain::ErrorKind::InvalidAwait => {
                    "`.await` requires a Task value".to_owned()
                }
                alder_constrain::ErrorKind::InvalidTry => "`?` requires a Result value".to_owned(),
                alder_constrain::ErrorKind::ReturnMismatch => {
                    "return value does not match the function result".to_owned()
                }
            };
            at(error.region, message)
        }
        SolveError::Trait(error) => match error {
            SolveTraitError::MissingInstance {
                trait_,
                subject,
                origin,
            } => at(
                *origin,
                format!(
                    "no implementation of `{}[{subject}]` was found",
                    trait_.0.name
                ),
            ),
            SolveTraitError::AmbiguousInstance {
                trait_,
                subject,
                origin,
                candidates,
            } => at(
                *origin,
                format!(
                    "multiple implementations of `{}[{subject}]` match ({} candidates)",
                    trait_.0.name,
                    candidates.len()
                ),
            ),
            SolveTraitError::UnsatisfiedBound {
                trait_,
                subject,
                origin,
            } => at(
                *origin,
                format!(
                    "the generic type `{subject}` requires `{}`\nhelp: add a matching `where` bound, such as `where {subject}: {}`",
                    trait_.0.name, trait_.0.name
                ),
            ),
            SolveTraitError::InstanceCycle {
                trait_,
                subject,
                origin,
            } => at(
                *origin,
                format!(
                    "resolving `{}[{subject}]` forms an instance cycle",
                    trait_.0.name
                ),
            ),
        },
        SolveError::Coherence(error) => match error {
            CoherenceError::SuperclassCycle { traits } => format!(
                "trait superclass cycle: {}",
                traits
                    .iter()
                    .map(|trait_| trait_.0.name)
                    .collect::<Vec<_>>()
                    .join(" -> ")
            ),
            CoherenceError::OrphanImpl {
                trait_,
                subject,
                trait_package,
                type_package,
                ..
            } => format!(
                "orphan implementation `{}[{subject}]`: this package defines neither the trait ({}) nor the subject type ({})",
                trait_.0.name,
                package_name(*trait_package),
                type_package
                    .map(package_name)
                    .unwrap_or("no owning package")
            ),
            CoherenceError::OverlappingImpl { trait_, .. } => format!(
                "overlapping implementations of `{}` are not allowed",
                trait_.0.name
            ),
            CoherenceError::InvalidTermination { prerequisite, .. } => format!(
                "instance prerequisite `{}` does not structurally decrease",
                prerequisite.0.name
            ),
            CoherenceError::KindMismatch {
                parameter,
                expected_arity,
                actual_arity,
                ..
            } => format!(
                "trait argument {} has kind arity {actual_arity}, but arity {expected_arity} is required",
                parameter + 1
            ),
            CoherenceError::ProjectionCycle { assoc, .. } => format!(
                "associated type `{}` is defined in terms of itself",
                assoc.name
            ),
        },
    }
}

fn at(region: Region, message: String) -> String {
    format!(
        "{}:{}: {message}",
        region.start.line.max(1),
        region.start.column.max(1)
    )
}

fn package_name(package: PackageId<'_>) -> &'static str {
    match package {
        PackageId::Named(_) => "a dependency package",
        PackageId::Application => "the application",
        PackageId::ApplicationMember(_) => "an application workspace member",
        PackageId::Builtin => "the standard library",
    }
}
