//! Alder type inference over the canonical AST.

mod inference;
mod traits;

use std::collections::BTreeMap;

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
    SelfDictionary,
    Super(u16),
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
