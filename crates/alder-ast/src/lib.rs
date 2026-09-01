use alder_region::{Located, Region};

pub use alder_source::{Associativity, Docs, Precedence};

pub type FreeVars<'a> = &'a [&'a str];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PackageName<'a> {
    pub author: &'a str,
    pub project: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ModuleName<'a> {
    pub package: Option<PackageName<'a>>,
    pub name: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct QualifiedName<'a> {
    pub home: ModuleName<'a>,
    pub name: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ConstructorName<'a> {
    pub home: ModuleName<'a>,
    pub union: &'a str,
    pub name: &'a str,
}

#[derive(Debug)]
pub struct Module<'a> {
    pub name: ModuleName<'a>,
    pub exports: Exports<'a>,
    pub docs: &'a Docs<'a>,
    pub decls: &'a Decls<'a>,
    pub unions: &'a [&'a Located<Union<'a>>],
    pub aliases: &'a [&'a Located<Alias<'a>>],
    pub binops: &'a [&'a Located<Binop<'a>>],
}

#[derive(Debug)]
pub enum Decls<'a> {
    Declare {
        definition: &'a Def<'a>,
        next: &'a Decls<'a>,
    },
    DeclareRec {
        definition: &'a Def<'a>,
        following: &'a [&'a Def<'a>],
        next: &'a Decls<'a>,
    },
    Empty,
}

#[derive(Debug)]
pub enum Def<'a> {
    Def {
        name: &'a Located<&'a str>,
        args: &'a [&'a Located<Pattern<'a>>],
        body: &'a Located<Expr<'a>>,
    },
    TypedDef {
        name: &'a Located<&'a str>,
        free_vars: FreeVars<'a>,
        args: &'a [TypedPattern<'a>],
        body: &'a Located<Expr<'a>>,
        typ: &'a Located<Type<'a>>,
    },
}

#[derive(Debug)]
pub struct TypedPattern<'a> {
    pub pattern: &'a Located<Pattern<'a>>,
    pub typ: &'a Located<Type<'a>>,
}

#[derive(Debug)]
pub struct Union<'a> {
    pub name: &'a Located<&'a str>,
    pub parameters: &'a [&'a str],
    pub ctors: &'a [&'a Ctor<'a>],
    pub alternatives: u16,
    pub options: CtorOpts,
}

#[derive(Debug)]
pub struct Ctor<'a> {
    pub name: &'a str,
    pub index: u16,
    pub arity: u16,
    pub arguments: &'a [&'a Located<Type<'a>>],
}

#[derive(Debug)]
pub struct Alias<'a> {
    pub name: &'a Located<&'a str>,
    pub parameters: &'a [&'a str],
    pub typ: &'a Located<Type<'a>>,
}

#[derive(Debug)]
pub struct Binop<'a> {
    pub symbol: &'a str,
    pub associativity: Associativity,
    pub precedence: Precedence,
    pub function: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CtorOpts {
    Normal,
    Enum,
    Unbox,
}

#[derive(Debug)]
pub enum Expr<'a> {
    VarLocal(&'a str),
    VarTopLevel(QualifiedName<'a>),
    /// Mirrors Elm's `Can.VarForeign home name annotation`. The annotation
    /// comes from the defining module's interface, which is only produced
    /// after that module has been type-solved.
    VarForeign {
        reference: QualifiedName<'a>,
        annotation: &'a Annotation<'a>,
    },
    VarConstructor {
        options: CtorOpts,
        reference: ConstructorName<'a>,
        index: u16,
        annotation: &'a Annotation<'a>,
    },
    /// Mirrors Elm's `Can.VarOperator op home name annotation`.
    VarOperator {
        symbol: &'a str,
        reference: QualifiedName<'a>,
        annotation: &'a Annotation<'a>,
    },
    Str(&'a str),
    Int(i128),
    List(&'a [&'a Located<Expr<'a>>]),
    Negate(&'a Located<Expr<'a>>),
    /// Mirrors Elm's `Can.Binop op home name annotation left right`.
    Binop {
        symbol: &'a str,
        reference: QualifiedName<'a>,
        annotation: &'a Annotation<'a>,
        left: &'a Located<Expr<'a>>,
        right: &'a Located<Expr<'a>>,
    },
    Lambda {
        parameters: &'a [&'a Located<Pattern<'a>>],
        body: &'a Located<Expr<'a>>,
    },
    Call {
        function: &'a Located<Expr<'a>>,
        arguments: &'a [&'a Located<Expr<'a>>],
    },
    If {
        branches: &'a [IfBranch<'a>],
        final_else: &'a Located<Expr<'a>>,
    },
    Let {
        definition: &'a Def<'a>,
        body: &'a Located<Expr<'a>>,
    },
    LetRec {
        definitions: &'a [&'a Def<'a>],
        body: &'a Located<Expr<'a>>,
    },
    LetDestruct {
        pattern: &'a Located<Pattern<'a>>,
        value: &'a Located<Expr<'a>>,
        body: &'a Located<Expr<'a>>,
    },
    Case {
        scrutinee: &'a Located<Expr<'a>>,
        branches: &'a [CaseBranch<'a>],
    },
    Accessor(&'a str),
    Access {
        record: &'a Located<Expr<'a>>,
        field: &'a Located<&'a str>,
    },
    Update {
        record: &'a str,
        base: &'a Located<Expr<'a>>,
        fields: &'a [FieldUpdate<'a>],
    },
    Record(&'a [FieldValue<'a>]),
    Unit,
    Tuple {
        first: &'a Located<Expr<'a>>,
        second: &'a Located<Expr<'a>>,
        rest: &'a [&'a Located<Expr<'a>>],
    },
}

#[derive(Debug)]
pub struct IfBranch<'a> {
    pub condition: &'a Located<Expr<'a>>,
    pub then_branch: &'a Located<Expr<'a>>,
}

#[derive(Debug)]
pub struct CaseBranch<'a> {
    pub pattern: &'a Located<Pattern<'a>>,
    pub body: &'a Located<Expr<'a>>,
}

#[derive(Debug)]
pub struct FieldUpdate<'a> {
    pub field: &'a Located<&'a str>,
    pub value: &'a Located<Expr<'a>>,
}

#[derive(Debug)]
pub struct FieldValue<'a> {
    pub field: &'a Located<&'a str>,
    pub value: &'a Located<Expr<'a>>,
}

#[derive(Debug)]
pub enum Pattern<'a> {
    Anything,
    Var(&'a str),
    Record(&'a [&'a str]),
    Alias {
        pattern: &'a Located<Pattern<'a>>,
        name: &'a str,
    },
    Unit,
    Tuple {
        first: &'a Located<Pattern<'a>>,
        second: &'a Located<Pattern<'a>>,
        rest: &'a [&'a Located<Pattern<'a>>],
    },
    List(&'a [&'a Located<Pattern<'a>>]),
    Cons {
        head: &'a Located<Pattern<'a>>,
        tail: &'a Located<Pattern<'a>>,
    },
    Constructor(PatternCtor<'a>),
    Bool {
        union: &'a Union<'a>,
        value: bool,
    },
    Str(&'a str),
    Int(i128),
}

#[derive(Debug)]
pub struct PatternCtor<'a> {
    pub reference: ConstructorName<'a>,
    pub union: &'a Union<'a>,
    pub index: u16,
    pub arguments: &'a [PatternCtorArg<'a>],
    pub options: CtorOpts,
    pub alternatives: u16,
}

#[derive(Debug)]
pub struct PatternCtorArg<'a> {
    pub index: u16,
    pub typ: &'a Located<Type<'a>>,
    pub pattern: &'a Located<Pattern<'a>>,
}

#[derive(Debug)]
pub struct Annotation<'a> {
    pub free_vars: FreeVars<'a>,
    pub typ: &'a Located<Type<'a>>,
}

#[derive(Debug)]
pub enum Type<'a> {
    Lambda {
        from: &'a Located<Type<'a>>,
        to: &'a Located<Type<'a>>,
    },
    Var(&'a str),
    Named {
        reference: QualifiedName<'a>,
        args: &'a [&'a Located<Type<'a>>],
    },
    Record {
        fields: &'a [FieldType<'a>],
        ext: Option<&'a str>,
    },
    Unit,
    Tuple {
        first: &'a Located<Type<'a>>,
        second: &'a Located<Type<'a>>,
        rest: &'a [&'a Located<Type<'a>>],
    },
    Alias {
        reference: QualifiedName<'a>,
        arguments: &'a [AliasArgument<'a>],
        target: AliasType<'a>,
    },
}

#[derive(Debug)]
pub enum AliasType<'a> {
    Open(&'a Located<Type<'a>>),
    Filled(&'a Located<Type<'a>>),
}

#[derive(Debug)]
pub struct AliasArgument<'a> {
    pub name: &'a str,
    pub typ: &'a Located<Type<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct FieldType<'a> {
    pub index: u16,
    pub field: &'a str,
    pub typ: &'a Located<Type<'a>>,
}

#[derive(Debug)]
pub enum Exports<'a> {
    Everything(Region),
    Explicit(&'a [&'a Located<Export<'a>>]),
}

#[derive(Debug)]
pub enum Export<'a> {
    Value(&'a str),
    Binop(&'a str),
    Alias(&'a str),
    UnionOpen(&'a str),
    UnionClosed(&'a str),
}
