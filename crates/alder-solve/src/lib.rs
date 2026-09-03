//! Alder type inference over the canonical AST.

mod inference;
mod traits;

use std::collections::BTreeMap;

use alder_ast::{ImplId, MethodId, TraitId, UseId};
use alder_can::Annotations;
use alder_region::Region;

pub use inference::{run, solve};
pub use traits::{InstanceHeader, TraitDatabase, TraitHeader, builtin_trait_id};

#[derive(Clone, Debug)]
pub struct SolveOutput<'a> {
    pub annotations: Annotations<'a>,
    pub uses: BTreeMap<UseId, UseAction<'a>>,
}

#[derive(Clone, Debug)]
pub enum UseAction<'a> {
    Reference {
        dictionaries: Vec<Evidence<'a>>,
        method: Option<MethodId<'a>>,
    },
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

#[derive(Clone, Debug)]
pub enum Evidence<'a> {
    Param(u16),
    Impl {
        impl_id: ImplId<'a>,
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
