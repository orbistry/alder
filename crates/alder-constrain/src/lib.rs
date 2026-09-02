//! Alder-native input contract for type inference.
//!
//! Canonicalization has already resolved every name, so constraint generation
//! preserves the canonical module and lets `alder-solve` walk it with the type
//! environment. Keeping this as a separate crate retains the compiler phase
//! boundary without carrying Elm's binary-function and fixed-tuple constraint
//! vocabulary into Alder.

use alder_ast::Module;
use alder_region::Region;

#[derive(Debug)]
pub struct Constraints<'a> {
    pub module: &'a Module<'a>,
}

#[derive(Debug, Default)]
pub struct UnionFind;

impl UnionFind {
    pub const fn new() -> Self {
        Self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub region: Region,
    pub kind: ErrorKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Mismatch { actual: String, expected: String },
    Arity { expected: usize, actual: usize },
    MissingField { field: String },
    InfiniteType,
    InvalidAwait,
    InvalidTry,
    ReturnMismatch,
}

pub fn constrain<'a>(
    _bump: &'a bumpalo::Bump,
    _uf: &mut UnionFind,
    module: &'a Module<'a>,
) -> Constraints<'a> {
    Constraints { module }
}
