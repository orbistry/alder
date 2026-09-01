//! Port of the data types from Elm's `Reporting.Error.Type`: what the
//! constraint generator records about *why* two types must match, and the
//! type errors the solver produces when they don't.
//!
//! `toReport` rendering is deferred along with the rest of error reporting.

use alder_ast::FieldUpdate;
use alder_region::Region;

use crate::error_type::ErrorType;

// ERRORS

#[derive(Debug)]
pub enum Error<'a> {
    BadExpr(
        Region,
        Category<'a>,
        &'a ErrorType<'a>,
        Expected<'a, &'a ErrorType<'a>>,
    ),
    BadPattern(
        Region,
        PCategory<'a>,
        &'a ErrorType<'a>,
        PExpected<'a, &'a ErrorType<'a>>,
    ),
    InfiniteType {
        region: Region,
        name: &'a str,
        overall_type: &'a ErrorType<'a>,
    },
}

// EXPRESSION EXPECTATIONS

#[derive(Clone, Copy, Debug)]
pub enum Expected<'a, T> {
    NoExpectation(T),
    FromContext(Region, Context<'a>, T),
    FromAnnotation(&'a str, usize, SubContext, T),
}

/// Indexes are zero-based, mirroring Elm's `Index.ZeroBased`.
#[derive(Clone, Copy, Debug)]
pub enum Context<'a> {
    ListEntry(usize),
    Negate,
    OpLeft(&'a str),
    OpRight(&'a str),
    IfCondition,
    IfBranch(usize),
    CaseBranch(usize),
    CallArity(MaybeName<'a>, usize),
    CallArg(MaybeName<'a>, usize),
    RecordAccess {
        record_region: Region,
        maybe_name: Option<&'a str>,
        field_region: Region,
        field: &'a str,
    },
    RecordUpdateKeys(&'a str, &'a [FieldUpdate<'a>]),
    RecordUpdateValue(&'a str),
    Destructure,
}

#[derive(Clone, Copy, Debug)]
pub enum SubContext {
    TypedIfBranch(usize),
    TypedCaseBranch(usize),
    TypedBody,
}

#[derive(Clone, Copy, Debug)]
pub enum MaybeName<'a> {
    FuncName(&'a str),
    CtorName(&'a str),
    OpName(&'a str),
    NoName,
}

/// Elm's `Category`, without the `Float`, `Char`, `Shader`, and `Effects`
/// cases: alder-ast has no such expressions.
#[derive(Clone, Copy, Debug)]
pub enum Category<'a> {
    List,
    Number,
    String,
    If,
    Case,
    CallResult(MaybeName<'a>),
    Lambda,
    Accessor(&'a str),
    Access(&'a str),
    Record,
    Tuple,
    Unit,
    Local(&'a str),
    Foreign(&'a str),
}

// PATTERN EXPECTATIONS

#[derive(Clone, Copy, Debug)]
pub enum PExpected<'a, T> {
    NoExpectation(T),
    FromContext(Region, PContext<'a>, T),
}

#[derive(Clone, Copy, Debug)]
pub enum PContext<'a> {
    TypedArg(&'a str, usize),
    CaseMatch(usize),
    CtorArg(&'a str, usize),
    ListEntry(usize),
    Tail,
}

/// Elm's `PCategory`, without the `PChr` case: alder-ast has no char
/// patterns.
#[derive(Clone, Copy, Debug)]
pub enum PCategory<'a> {
    Record,
    Unit,
    Tuple,
    List,
    Ctor(&'a str),
    Int,
    Str,
    Bool,
}

// HELPERS

impl<'a, T> Expected<'a, T> {
    /// Elm's `typeReplace`.
    pub fn type_replace<U>(&self, tipe: U) -> Expected<'a, U> {
        match self {
            Expected::NoExpectation(_) => Expected::NoExpectation(tipe),
            Expected::FromContext(region, context, _) => {
                Expected::FromContext(*region, *context, tipe)
            }
            Expected::FromAnnotation(name, arity, context, _) => {
                Expected::FromAnnotation(name, *arity, *context, tipe)
            }
        }
    }
}

impl<'a, T> PExpected<'a, T> {
    /// Elm's `ptypeReplace`.
    pub fn type_replace<U>(&self, tipe: U) -> PExpected<'a, U> {
        match self {
            PExpected::NoExpectation(_) => PExpected::NoExpectation(tipe),
            PExpected::FromContext(region, context, _) => {
                PExpected::FromContext(*region, *context, tipe)
            }
        }
    }
}
