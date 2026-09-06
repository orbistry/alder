//! Alder-native input contract for type inference.
//!
//! Canonicalization has already resolved every name, so constraint generation
//! preserves the canonical module and lets `alder-solve` walk it with the type
//! environment. Keeping this as a separate crate retains the compiler phase
//! boundary without carrying Elm's binary-function and fixed-tuple constraint
//! vocabulary into Alder.

use alder_ast::{MethodId, Module, UseId};
use alder_region::Region;

/// An owned snapshot of a diagnostic type, independent of the inference arena.
/// Variable indices are dense, comparison-local names, never solver IDs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiagnosticType {
    Variable(usize),
    NamedVariable(String),
    Named(String),
    Application(Box<Self>, Vec<Self>),
    Function(Vec<Self>, Box<Self>),
    Unit,
    Tuple(Vec<Self>),
    TupleShape(u64),
    Record(Vec<(String, Self)>, Option<Box<Self>>),
    ErrorRow(Vec<(String, Vec<Self>)>, Option<Box<Self>>),
    Projection(Box<Self>, String),
    Hole,
}

impl std::fmt::Display for DiagnosticType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fn separated<T: std::fmt::Display>(
            f: &mut std::fmt::Formatter<'_>,
            items: &[T],
        ) -> std::fmt::Result {
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{item}")?;
            }
            Ok(())
        }
        match self {
            Self::Variable(index) if *index < 26 => write!(f, "{}", (b'a' + *index as u8) as char),
            Self::Variable(index) => write!(f, "t{index}"),
            Self::NamedVariable(name) => f.write_str(name),
            Self::Named(name) => f.write_str(name),
            Self::Application(head, args) => {
                write!(f, "{head}[")?;
                separated(f, args)?;
                write!(f, "]")
            }
            Self::Function(args, result) => {
                write!(f, "fn(")?;
                separated(f, args)?;
                write!(f, ") {result}")
            }
            Self::Unit => write!(f, "()"),
            Self::Tuple(items) => {
                write!(f, "(")?;
                separated(f, items)?;
                write!(f, ")")
            }
            Self::TupleShape(length) => write!(f, "tuple of length {length}"),
            Self::Record(fields, tail) => {
                write!(f, "{{ ")?;
                if let Some(tail) = tail {
                    write!(f, "{tail} | ")?;
                }
                for (index, (name, typ)) in fields.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{name}: {typ}")?;
                }
                write!(f, " }}")
            }
            Self::ErrorRow(tags, tail) => {
                write!(f, "[")?;
                for (index, (name, args)) in tags.iter().enumerate() {
                    if index > 0 {
                        write!(f, " | ")?;
                    }
                    write!(f, ":{name}")?;
                    if !args.is_empty() {
                        write!(f, "(")?;
                        separated(f, args)?;
                        write!(f, ")")?;
                    }
                }
                if let Some(tail) = tail {
                    if !tags.is_empty() {
                        write!(f, " | ")?;
                    }
                    write!(f, "{tail}")?;
                }
                write!(f, "]")
            }
            Self::Projection(head, assoc) => write!(f, "{head}::{assoc}"),
            Self::Hole => write!(f, "_"),
        }
    }
}

/// The restriction that contradicts a declaration's universal type promise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GenericRestriction {
    Type(DiagnosticType),
    SameVariable(String),
    ResultRow(String),
    RecordField(String),
}

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
    pub expectation: Option<Box<Expectation>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expectation {
    pub kind: ExpectationKind,
    /// A requirement in this source module, never a foreign module's span.
    pub origin: Option<Region>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExpectationKind {
    Annotation,
    Argument {
        position: usize,
        callee: Option<String>,
    },
    Call {
        callee: Option<String>,
    },
    Condition,
    Branch,
    ArrayElement {
        position: usize,
    },
    Pattern,
    Assignment,
    Return,
    Await,
    Propagation,
}

impl Error {
    /// Keep the innermost explanation: an invalid condition inside an argument
    /// should not be relabeled as a failure of the argument's parameter type.
    pub fn expected_by(mut self, kind: ExpectationKind, origin: Option<Region>) -> Self {
        if self.expectation.is_none() {
            self.expectation = Some(Box::new(Expectation { kind, origin }));
        }
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    AmbiguousOptionLifting,
    RecursiveErrorGroup {
        name: String,
    },
    InvalidResultErrorType {
        actual: String,
    },
    Mismatch {
        actual: DiagnosticType,
        expected: DiagnosticType,
    },
    Arity {
        expected: usize,
        actual: usize,
    },
    MissingField {
        field: String,
        available: Vec<String>,
    },
    RecordFieldsMismatch {
        actual: DiagnosticType,
        expected: DiagnosticType,
    },
    TupleIndexOutOfBounds {
        index: u32,
        length: usize,
    },
    AssocTypeMismatch {
        assoc: String,
        expected: String,
        actual: String,
    },
    InfiniteType {
        equation: Option<Box<(DiagnosticType, DiagnosticType)>>,
    },
    UnsupportedHigherKindedUnification,
    InvalidAwait,
    InvalidTry,
    ReturnMismatch,
    MissingReturn {
        expected: String,
    },
    GenericSpecialization {
        variable: String,
        restriction: GenericRestriction,
    },
    GenericEscape {
        variable: String,
    },
    UnresolvedSharedExport {
        name: String,
    },
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
