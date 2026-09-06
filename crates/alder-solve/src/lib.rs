//! Alder type inference over the canonical AST.

mod inference;
mod option_levels;
mod pattern_matrix;
mod traits;

use std::collections::{BTreeMap, BTreeSet};

use alder_ast::{DictionaryKind, ImplId, MethodId, ModuleId, QualifiedName, TraitId, UseId};
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
    pub derived_fields: BTreeMap<DerivedFieldKey<'a>, Evidence<'a>>,
    /// Propagation expressions checked against Option rather than Result.
    pub option_tries: BTreeSet<Region>,
    /// Trailing None arguments inserted at each call, excluding dictionaries.
    pub omitted_arguments: BTreeMap<UseId, usize>,
    /// Contextual Some layers by call and source-argument index (pipe first).
    pub argument_lifts: BTreeMap<(UseId, usize), usize>,
    /// Contextual Some layers by record initializer field-name region.
    pub field_lifts: BTreeMap<Region, usize>,
    /// Omitted Option fields materialized as None at a record construction site.
    pub omitted_record_fields: BTreeMap<Region, Vec<&'a str>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DerivedFieldKey<'a> {
    pub implementation: ImplId<'a>,
    pub variant: u16,
    pub field: u16,
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
    IntrinsicContainer {
        intrinsic: Intrinsic,
        container: IntrinsicContainer,
        arguments: Vec<Evidence<'a>>,
    },
    StructuralEq {
        shape: StructuralEqShape<'a>,
        fields: Vec<Evidence<'a>>,
    },
    StructuralError {
        capability: StructuralErrorCapability,
        tags: Vec<(&'a str, Vec<Evidence<'a>>)>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructuralErrorCapability {
    Show,
    Json,
    Hash,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntrinsicContainer {
    Array,
    Option,
    Result,
}

#[derive(Clone, Debug)]
pub enum StructuralEqShape<'a> {
    Array,
    Option,
    Result,
    Tuple,
    Record(Vec<&'a str>),
    ErrorRow(Vec<(&'a str, usize)>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intrinsic {
    EqNumber,
    EqString,
    EqBool,
    EqBigInt,
    EqUnit,
    EqOrdering,
    OrdNumber,
    OrdString,
    OrdBigInt,
    OrdUnit,
    OrdOption,
    NumNumber,
    NumBigInt,
    FunctorArray,
    FunctorOption,
    FunctorResult,
    ShowKernel,
    HashKernel,
    JsonKernel,
    JsonNumber,
    JsonString,
    JsonBool,
    JsonBigInt,
    JsonUnit,
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
        args: Box<[alder_constrain::DiagnosticType]>,
        origin: Region,
        chain: Box<[ObligationFrame<'a>]>,
    },
    AmbiguousInstance {
        trait_: TraitId<'a>,
        args: Box<[alder_constrain::DiagnosticType]>,
        origin: Region,
        details: Box<AmbiguousInstanceDetails<'a>>,
    },
    UnsatisfiedBound {
        trait_: TraitId<'a>,
        args: Box<[alder_constrain::DiagnosticType]>,
        origin: Region,
        chain: Box<[ObligationFrame<'a>]>,
    },
    AmbiguousTypeVariable {
        trait_: TraitId<'a>,
        args: Box<[alder_constrain::DiagnosticType]>,
        origin: Region,
        chain: Box<[ObligationFrame<'a>]>,
    },
    InstanceCycle {
        trait_: TraitId<'a>,
        args: Box<[alder_constrain::DiagnosticType]>,
        origin: Region,
        chain: Box<[ObligationFrame<'a>]>,
    },
}

#[derive(Clone, Debug)]
pub struct AmbiguousInstanceDetails<'a> {
    pub candidates: Box<[ImplId<'a>]>,
    pub chain: Box<[ObligationFrame<'a>]>,
}

#[derive(Clone, Debug)]
pub struct ObligationFrame<'a> {
    pub trait_: TraitId<'a>,
    pub args: Box<[alder_constrain::DiagnosticType]>,
    pub required_by: Option<ImplId<'a>>,
}
