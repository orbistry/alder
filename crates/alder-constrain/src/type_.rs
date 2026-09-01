//! Port of the data half of Elm's `Type.Type`: constraints, the inference
//! `Type` language, and unification variable descriptors.
//!
//! `toAnnotation` and `toErrorType` live in `alder-solve` (they are only
//! called by the solver and need `alder-can`'s canonical-type utilities).

use std::collections::BTreeMap;

use alder_ast::{Annotation, ModuleName};
use alder_region::{Located, Region};

use crate::error::{Category, Expected, PCategory, PExpected};
use crate::union_find::{UnionFind, Variable};

// CONSTRAINTS

/// Elm's `Type.Constraint`. Allocated in a bump arena, so collections are
/// slices, not owned containers.
#[derive(Debug)]
pub enum Constraint<'a> {
    True,
    SaveTheEnvironment,
    Equal(
        Region,
        Category<'a>,
        &'a Type<'a>,
        Expected<'a, &'a Type<'a>>,
    ),
    Local(Region, &'a str, Expected<'a, &'a Type<'a>>),
    Foreign(
        Region,
        &'a str,
        &'a Annotation<'a>,
        Expected<'a, &'a Type<'a>>,
    ),
    Pattern(
        Region,
        PCategory<'a>,
        &'a Type<'a>,
        PExpected<'a, &'a Type<'a>>,
    ),
    And(&'a [Constraint<'a>]),
    Let {
        rigid_vars: &'a [Variable],
        flex_vars: &'a [Variable],
        /// Name-sorted, mirroring Elm's `Map.Map Name (A.Located Type)`.
        header: &'a [(&'a str, Located<&'a Type<'a>>)],
        header_con: &'a Constraint<'a>,
        body_con: &'a Constraint<'a>,
    },
}

/// Elm's `exists`: a `CLet` binding only flex variables.
pub fn exists<'a>(
    bump: &'a bumpalo::Bump,
    flex_vars: &'a [Variable],
    constraint: Constraint<'a>,
) -> Constraint<'a> {
    Constraint::Let {
        rigid_vars: &[],
        flex_vars,
        header: &[],
        header_con: bump.alloc(constraint),
        body_con: bump.alloc(Constraint::True),
    }
}

// TYPE PRIMITIVES

/// Elm's `Type.FlatType`. Lives inside descriptors owned by the union-find
/// store (real heap, so owned containers are fine here).
#[derive(Clone, Debug)]
pub enum FlatType<'a> {
    App1(ModuleName<'a>, &'a str, Vec<Variable>),
    Fun1(Variable, Variable),
    EmptyRecord1,
    Record1(BTreeMap<&'a str, Variable>, Variable),
    Unit1,
    Tuple1(Variable, Variable, Option<Variable>),
}

/// Elm's `Type.Type`: the language the constraint generator writes types in.
#[derive(Clone, Copy, Debug)]
pub enum Type<'a> {
    PlaceHolder(&'a str),
    AliasN {
        home: ModuleName<'a>,
        name: &'a str,
        args: &'a [(&'a str, &'a Type<'a>)],
        real: &'a Type<'a>,
    },
    VarN(Variable),
    AppN {
        home: ModuleName<'a>,
        name: &'a str,
        args: &'a [&'a Type<'a>],
    },
    FunN(&'a Type<'a>, &'a Type<'a>),
    EmptyRecordN,
    /// Name-sorted, mirroring Elm's `Map.Map Name Type`.
    RecordN {
        fields: &'a [(&'a str, &'a Type<'a>)],
        ext: &'a Type<'a>,
    },
    UnitN,
    TupleN(&'a Type<'a>, &'a Type<'a>, Option<&'a Type<'a>>),
}

// DESCRIPTORS

#[derive(Clone, Debug)]
pub struct Descriptor<'a> {
    pub content: Content<'a>,
    pub rank: usize,
    pub mark: Mark,
    pub copy: Option<Variable>,
}

#[derive(Clone, Debug)]
pub enum Content<'a> {
    FlexVar(Option<&'a str>),
    FlexSuper(SuperType, Option<&'a str>),
    RigidVar(&'a str),
    RigidSuper(SuperType, &'a str),
    Structure(FlatType<'a>),
    Alias {
        home: ModuleName<'a>,
        name: &'a str,
        args: Vec<(&'a str, Variable)>,
        real: Variable,
    },
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuperType {
    Number,
    Comparable,
    Appendable,
    CompAppend,
}

pub fn make_descriptor(content: Content<'_>) -> Descriptor<'_> {
    Descriptor {
        content,
        rank: NO_RANK,
        mark: NO_MARK,
        copy: None,
    }
}

// RANKS

pub const NO_RANK: usize = 0;
pub const OUTERMOST_RANK: usize = 1;

// MARKS

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Mark(u32);

pub const NO_MARK: Mark = Mark(2);
pub const OCCURS_MARK: Mark = Mark(1);
/// Reserved in Elm for `getVarNames` visit tracking. Alder's `get_var_names`
/// uses a per-call seen set instead (see `alder-solve/src/annotation.rs`),
/// but the mark stays reserved so the mark space matches Elm's.
pub const GET_VAR_NAMES_MARK: Mark = Mark(0);

impl Mark {
    pub fn next(self) -> Mark {
        Mark(self.0 + 1)
    }
}

// BUILT-IN MODULES
//
// Like `alder-can`'s Bool handling, built-in homes are package-less module
// names pending a canonical core package (Elm keys these to `elm/core`).

pub const fn basics<'a>() -> ModuleName<'a> {
    ModuleName {
        package: None,
        name: "Basics",
    }
}

pub const fn list_home<'a>() -> ModuleName<'a> {
    ModuleName {
        package: None,
        name: "List",
    }
}

pub const fn string_home<'a>() -> ModuleName<'a> {
    ModuleName {
        package: None,
        name: "String",
    }
}

pub const fn char_home<'a>() -> ModuleName<'a> {
    ModuleName {
        package: None,
        name: "Char",
    }
}

// PRIMITIVE TYPES

pub const fn int<'a>() -> Type<'a> {
    Type::AppN {
        home: basics(),
        name: "Int",
        args: &[],
    }
}

pub const fn float<'a>() -> Type<'a> {
    Type::AppN {
        home: basics(),
        name: "Float",
        args: &[],
    }
}

pub const fn string<'a>() -> Type<'a> {
    Type::AppN {
        home: string_home(),
        name: "String",
        args: &[],
    }
}

pub const fn bool<'a>() -> Type<'a> {
    Type::AppN {
        home: basics(),
        name: "Bool",
        args: &[],
    }
}

// MAKE FLEX VARIABLES

pub fn mk_flex_var<'a>(uf: &mut UnionFind<'a>) -> Variable {
    uf.fresh(make_descriptor(unnamed_flex_var()))
}

pub fn mk_flex_number<'a>(uf: &mut UnionFind<'a>) -> Variable {
    uf.fresh(make_descriptor(unnamed_flex_super(SuperType::Number)))
}

pub const fn unnamed_flex_var<'a>() -> Content<'a> {
    Content::FlexVar(None)
}

pub const fn unnamed_flex_super<'a>(super_type: SuperType) -> Content<'a> {
    Content::FlexSuper(super_type, None)
}

// MAKE NAMED VARIABLES

pub fn name_to_flex<'a>(uf: &mut UnionFind<'a>, name: &'a str) -> Variable {
    let content = match to_super(name) {
        Some(super_type) => Content::FlexSuper(super_type, Some(name)),
        None => Content::FlexVar(Some(name)),
    };
    uf.fresh(make_descriptor(content))
}

pub fn name_to_rigid<'a>(uf: &mut UnionFind<'a>, name: &'a str) -> Variable {
    let content = match to_super(name) {
        Some(super_type) => Content::RigidSuper(super_type, name),
        None => Content::RigidVar(name),
    };
    uf.fresh(make_descriptor(content))
}

/// Elm's `Name.isNumberType` and friends: super powers come from the
/// variable's name prefix.
pub fn to_super(name: &str) -> Option<SuperType> {
    if name.starts_with("number") {
        Some(SuperType::Number)
    } else if name.starts_with("comparable") {
        Some(SuperType::Comparable)
    } else if name.starts_with("appendable") {
        Some(SuperType::Appendable)
    } else if name.starts_with("compappend") {
        Some(SuperType::CompAppend)
    } else {
        None
    }
}
