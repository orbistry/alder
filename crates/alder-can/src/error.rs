use alder_ast::{ModuleId, Namespace, QualifiedName};
use alder_region::Region;

#[derive(Clone, Debug)]
pub struct Error<'a> {
    pub region: Region,
    pub kind: ErrorKind<'a>,
}

impl<'a> Error<'a> {
    pub const fn new(region: Region, kind: ErrorKind<'a>) -> Self {
        Self { region, kind }
    }
}

#[derive(Clone, Debug)]
pub enum ErrorKind<'a> {
    Import(ImportError<'a>),
    Item(ItemError<'a>),
    Type(TypeError<'a>),
    Pattern(PatternError<'a>),
    Expr(ExprError<'a>),
    Stmt(StmtError<'a>),
    Attribute(AttributeError<'a>),
}

#[derive(Clone, Debug)]
pub enum NameError<'a> {
    Unknown {
        namespace: Namespace,
        qualifier: Option<&'a str>,
        name: &'a str,
        suggestions: &'a [&'a str],
    },
    Ambiguous {
        namespace: Namespace,
        name: &'a str,
        candidates: &'a [QualifiedName<'a>],
    },
    Private {
        owner: ModuleId<'a>,
        namespace: Namespace,
        name: &'a str,
    },
}

#[derive(Clone, Debug)]
pub enum ImportError<'a> {
    Name(NameError<'a>),
    NameNotFound {
        module: ModuleId<'a>,
        name: &'a str,
        available: &'a [&'a str],
    },
    AliasCollision {
        name: &'a str,
        first: Region,
    },
    ReexportPrivate {
        module: ModuleId<'a>,
        name: &'a str,
    },
}

#[derive(Clone, Debug)]
pub enum ItemError<'a> {
    DuplicateDefinition {
        namespace: Namespace,
        name: &'a str,
        first: Region,
    },
    RecursiveValue {
        name: &'a str,
        cycle: &'a [&'a str],
    },
    RecursiveAlias {
        name: &'a str,
        cycle: &'a [&'a str],
    },
    AnnotationTooShort {
        name: &'a str,
        annotated: usize,
        parameters: usize,
    },
}

#[derive(Clone, Debug)]
pub enum TypeError<'a> {
    Name(NameError<'a>),
    BadArity {
        name: &'a str,
        expected: usize,
        actual: usize,
    },
    DuplicateParameter {
        name: &'a str,
        first: Region,
    },
    DuplicateField {
        name: &'a str,
        first: Region,
    },
    DuplicateTag {
        name: &'a str,
        first: Region,
    },
    UnboundVariable {
        name: &'a str,
    },
    UnusedParameter {
        name: &'a str,
    },
}

#[derive(Clone, Debug)]
pub enum PatternError<'a> {
    Name(NameError<'a>),
    DuplicateBinding {
        name: &'a str,
        first: Region,
    },
    ConstructorArity {
        name: ConstructorDisplay<'a>,
        expected: usize,
        actual: usize,
    },
    ConstructorPayload {
        name: ConstructorDisplay<'a>,
        expected: &'static str,
        actual: &'static str,
    },
    DuplicateField {
        name: &'a str,
        first: Region,
    },
    PinOutsideMatch,
}

#[derive(Clone, Copy, Debug)]
pub struct ConstructorDisplay<'a> {
    pub enum_name: &'a str,
    pub variant: &'a str,
}

#[derive(Clone, Debug)]
pub enum ExprError<'a> {
    Name(NameError<'a>),
    UnqualifiedConstructor {
        enum_name: &'a str,
        variant: &'a str,
    },
    PlaceholderOutsideCall,
    PinOutsideQuery,
    AwaitRequiresTaskReturn,
    MacroUnavailable {
        name: &'a str,
    },
    DuplicateField {
        name: &'a str,
        first: Region,
    },
    NonAssociativeOperators {
        left: &'a str,
        right: &'a str,
    },
}

#[derive(Clone, Debug)]
pub enum StmtError<'a> {
    Name(NameError<'a>),
    ImmutableAssignment { name: &'a str, binding: Region },
    InvalidAssignmentTarget,
    BreakOutsideLoop,
    ContinueOutsideLoop,
    ReturnOutsideFunction,
}

#[derive(Clone, Debug)]
pub enum AttributeError<'a> {
    InvalidExtern { reason: &'a str },
    DeriveUnavailable,
    Unknown { name: &'a str },
    MacroUnavailable,
}
