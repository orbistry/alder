use alder_region::{Located, Region};

#[derive(Debug)]
pub struct Module<'a> {
    pub name: Option<&'a Located<&'a str>>,
    pub exports: &'a Located<Exposing<'a>>,
    pub docs: &'a Docs<'a>,
    pub imports: &'a [&'a Import<'a>],
    pub values: &'a [&'a Located<Value<'a>>],
    pub unions: &'a [&'a Located<Union<'a>>],
    pub aliases: &'a [&'a Located<Alias<'a>>],
    pub binops: &'a [&'a Located<Infix<'a>>],
}

#[derive(Debug)]
pub struct Import<'a> {
    pub import: &'a Located<&'a str>,
    pub alias: Option<&'a str>,
    pub exposing: &'a Exposing<'a>,
}

#[derive(Debug)]
pub struct Value<'a> {
    pub name: &'a Located<&'a str>,
    pub arguments: &'a [&'a Located<Pattern<'a>>],
    pub body: &'a Located<Expr<'a>>,
    pub annotation: Option<&'a Located<Type<'a>>>,
}

// type Maybe a
//   = Just a
//   | Nothing
#[derive(Debug)]
pub struct Union<'a> {
    pub name: &'a Located<&'a str>,
    // type vars
    pub arguments: &'a [&'a Located<&'a str>],
    pub ctors: &'a [&'a Ctor<'a>],
}

#[derive(Debug)]
pub struct Ctor<'a> {
    pub name: &'a Located<&'a str>,
    pub arguments: &'a [&'a Located<Type<'a>>],
}

#[derive(Debug)]
pub struct Alias<'a> {
    pub name: &'a Located<&'a str>,
    // type vars
    pub arguments: &'a [&'a Located<&'a str>],
    pub typ: &'a Located<Type<'a>>,
}

#[derive(Debug)]
pub struct Infix<'a> {
    pub op: &'a str,
    pub associativity: Associativity,
    pub precedence: Precedence,
    pub name: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Associativity {
    Left,
    None,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Precedence(pub u16);

#[derive(Debug)]
pub enum Expr<'a> {
    Str(&'a str),
    Int(i128),
    Var {
        kind: VarType,
        name: &'a str,
    },
    VarQual {
        kind: VarType,
        module: &'a str,
        name: &'a str,
    },
    List(&'a [&'a Located<Expr<'a>>]),
    Op(&'a str),
    Negate(&'a Located<Expr<'a>>),
    BinOps {
        operands: &'a [&'a BinOpOperand<'a>],
        last: &'a Located<Expr<'a>>,
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
        branches: &'a [&'a IfBranch<'a>],
        final_else: &'a Located<Expr<'a>>,
    },
    Let {
        defs: &'a [&'a Located<Def<'a>>],
        body: &'a Located<Expr<'a>>,
    },
    Case {
        scrutinee: &'a Located<Expr<'a>>,
        arms: &'a [&'a CaseArm<'a>],
    },
    Accessor(&'a str),
    Access {
        record: &'a Located<Expr<'a>>,
        field: &'a Located<&'a str>,
    },
    Update {
        record: &'a Located<&'a str>,
        fields: &'a [&'a FieldAssign<'a>],
    },
    Record(&'a [&'a FieldAssign<'a>]),
    Unit,
    Tuple {
        first: &'a Located<Expr<'a>>,
        second: &'a Located<Expr<'a>>,
        rest: &'a [&'a Located<Expr<'a>>],
    },
}

#[derive(Debug)]
pub enum VarType {
    LowVar,
    CapVar,
}

#[derive(Debug)]
pub struct IfBranch<'a> {
    pub condition: &'a Located<Expr<'a>>,
    pub then_branch: &'a Located<Expr<'a>>,
}

/// An operand in a binary operator chain: expression followed by operator.
#[derive(Debug)]
pub struct BinOpOperand<'a> {
    pub expr: &'a Located<Expr<'a>>,
    pub op: &'a Located<&'a str>,
}

#[derive(Debug)]
pub enum Def<'a> {
    Define {
        name: &'a Located<&'a str>,
        args: &'a [&'a Located<Pattern<'a>>],
        body: &'a Located<Expr<'a>>,
        annotation: Option<&'a Located<Type<'a>>>,
    },
    Destruct {
        pattern: &'a Located<Pattern<'a>>,
        body: &'a Located<Expr<'a>>,
    },
}

#[derive(Debug)]
pub struct CaseArm<'a> {
    pub pattern: &'a Located<Pattern<'a>>,
    pub body: &'a Located<Expr<'a>>,
}

#[derive(Debug)]
pub struct FieldAssign<'a> {
    pub field: &'a Located<&'a str>,
    pub value: &'a Located<Expr<'a>>,
}

#[derive(Debug)]
pub enum Pattern<'a> {
    Anything,
    Var(&'a str),
    Record(&'a [&'a Located<&'a str>]),
    Alias {
        pattern: &'a Located<Pattern<'a>>,
        name: &'a Located<&'a str>,
    },
    Unit,
    Tuple {
        first: &'a Located<Pattern<'a>>,
        second: &'a Located<Pattern<'a>>,
        rest: &'a [&'a Located<Pattern<'a>>],
    },
    Ctor {
        region: Region,
        name: &'a str,
        args: &'a [&'a Located<Pattern<'a>>],
    },
    CtorQual {
        region: Region,
        module: &'a str,
        name: &'a str,
        args: &'a [&'a Located<Pattern<'a>>],
    },
    List(&'a [&'a Located<Pattern<'a>>]),
    Cons {
        head: &'a Located<Pattern<'a>>,
        tail: &'a Located<Pattern<'a>>,
    },
    Str(&'a str),
    Int(i128),
}

#[derive(Debug)]
pub enum Type<'a> {
    Lambda {
        from: &'a Located<Type<'a>>,
        to: &'a Located<Type<'a>>,
    },
    Var(&'a str),
    Type {
        region: Region,
        name: &'a str,
        args: &'a [&'a Located<Type<'a>>],
    },
    TypeQual {
        region: Region,
        module: &'a str,
        name: &'a str,
        args: &'a [&'a Located<Type<'a>>],
    },
    Record {
        fields: &'a [&'a FieldType<'a>],
        ext: Option<&'a Located<&'a str>>,
    },
    Unit,
    Tuple {
        first: &'a Located<Type<'a>>,
        second: &'a Located<Type<'a>>,
        rest: &'a [&'a Located<Type<'a>>],
    },
}

#[derive(Debug)]
pub struct FieldType<'a> {
    pub field: &'a Located<&'a str>,
    pub typ: &'a Located<Type<'a>>,
}

#[derive(Debug)]
pub enum Docs<'a> {
    NoDocs(Region),
    YesDocs {
        overview: &'a Comment<'a>,
        comments: &'a [&'a (&'a str, &'a Comment<'a>)],
    },
}

#[derive(Debug)]
pub struct Comment<'a>(pub &'a Snippet<'a>);

#[derive(Debug)]
pub struct Snippet<'a> {
    pub data: &'a [u8], // already the relevant slice
    // offset: usize,
    // length: usize,
    pub off_row: u16,
    pub off_col: u16,
}

#[derive(Debug)]
pub enum Exposing<'a> {
    Open,
    Explicit(&'a [&'a Exposed<'a>]),
}

#[derive(Debug)]
pub enum Exposed<'a> {
    Lower(&'a Located<&'a str>),
    Upper {
        name: &'a Located<&'a str>,
        privacy: Privacy,
    },
    Operator {
        region: Region,
        op: &'a str,
    },
}

#[derive(Debug)]
pub enum Privacy {
    Public(Region),
    Private,
}
