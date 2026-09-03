//! Alder-native input contract for type inference.
//!
//! Canonicalization has already resolved every name, so constraint generation
//! preserves the canonical module and lets `alder-solve` walk it with the type
//! environment. Keeping this as a separate crate retains the compiler phase
//! boundary without carrying Elm's binary-function and fixed-tuple constraint
//! vocabulary into Alder.

use alder_ast::{MethodId, Module, UseId};
use alder_region::Region;

#[derive(Debug)]
pub struct Constraints<'a> {
    pub module: &'a Module<'a>,
    pub requirement_seeds: &'a [RequirementSeed<'a>],
}

#[derive(Clone, Copy, Debug)]
pub struct RequirementSeed<'a> {
    pub use_id: UseId,
    pub kind: RequirementKind<'a>,
    pub region: Region,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequirementKind<'a> {
    TraitMethod(MethodId<'a>),
    Eq,
    Ord,
    Num,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub region: Region,
    pub kind: ErrorKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Mismatch {
        actual: String,
        expected: String,
    },
    Arity {
        expected: usize,
        actual: usize,
    },
    MissingField {
        field: String,
    },
    AssocTypeMismatch {
        assoc: String,
        expected: String,
        actual: String,
    },
    InfiniteType,
    UnsupportedHigherKindedUnification,
    InvalidAwait,
    InvalidTry,
    ReturnMismatch,
    NonExhaustiveErrorMatch {
        missing: Vec<String>,
        open: bool,
    },
    ImpossibleErrorPattern {
        tag: String,
    },
    InvalidErrorTagPlacement,
}

pub fn constrain<'a>(bump: &'a bumpalo::Bump, module: &'a Module<'a>) -> Constraints<'a> {
    let requirement_seeds = requirements::collect(bump, module);
    Constraints {
        module,
        requirement_seeds,
    }
}

mod requirements;
