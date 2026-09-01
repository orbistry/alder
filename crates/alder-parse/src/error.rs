//! Syntax error types for the Alder parser.
//!
//! Ported from Elm's `Reporting/Error/Syntax.hs`.
//! These types form a nested hierarchy that enables high-quality error messages.
//!
//! Note: `Type`, `Expr`, `Pattern` here are ERROR types describing parse failures,
//! not AST types. They are allocated in the arena like everything else.

use crate::{Col, Row};

// =============================================================================
// Top-level Error
// =============================================================================

#[derive(Debug)]
pub enum Error<'a> {
    ModuleNameUnspecified(&'a str),
    ModuleNameMismatch {
        expected: &'a str,
        actual: &'a str,
        row: Row,
        col: Col,
    },
    UnexpectedPort {
        row: Row,
        col: Col,
    },
    NoPorts {
        row: Row,
        col: Col,
    },
    NoPortsInPackage {
        name: &'a str,
        row: Row,
        col: Col,
    },
    NoPortModulesInPackage {
        row: Row,
        col: Col,
    },
    NoEffectsOutsideKernel {
        row: Row,
        col: Col,
    },
    ParseError(&'a Module<'a>),
}

// =============================================================================
// Module Errors
// =============================================================================

#[derive(Debug)]
pub enum Module<'a> {
    Space(Space, Row, Col),
    BadEnd(Row, Col),
    Problem(Row, Col),
    Name(Row, Col),
    Exposing(&'a Exposing, Row, Col),
    PortProblem(Row, Col),
    PortName(Row, Col),
    PortExposing(&'a Exposing, Row, Col),
    Effect(Row, Col),
    FreshLine(Row, Col),
    ImportStart(Row, Col),
    ImportName(Row, Col),
    ImportAs(Row, Col),
    ImportAlias(Row, Col),
    ImportExposing(Row, Col),
    ImportExposingList(&'a Exposing, Row, Col),
    ImportEnd(Row, Col),
    ImportIndentName(Row, Col),
    ImportIndentAlias(Row, Col),
    ImportIndentExposingList(Row, Col),
    Infix(Row, Col),
    Declarations(&'a Decl<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Exposing {
    Space(Space, Row, Col),
    Start(Row, Col),
    Value(Row, Col),
    Operator(Row, Col),
    OperatorReserved(BadOperator, Row, Col),
    OperatorRightParen(Row, Col),
    TypePrivacy(Row, Col),
    End(Row, Col),
    IndentEnd(Row, Col),
    IndentValue(Row, Col),
}

// =============================================================================
// Declaration Errors
// =============================================================================

#[derive(Debug)]
pub enum Decl<'a> {
    Start(Row, Col),
    Space(Space, Row, Col),
    Port(&'a Port<'a>, Row, Col),
    Type(&'a DeclType<'a>, Row, Col),
    Def(&'a str, &'a DeclDef<'a>, Row, Col),
    FreshLineAfterDocComment(Row, Col),
}

#[derive(Debug)]
pub enum DeclDef<'a> {
    Space(Space, Row, Col),
    Equals(Row, Col),
    Type(&'a Type<'a>, Row, Col),
    Arg(&'a Pattern<'a>, Row, Col),
    Body(&'a Expr<'a>, Row, Col),
    NameRepeat(Row, Col),
    NameMatch(&'a str, Row, Col),
    IndentType(Row, Col),
    IndentEquals(Row, Col),
    IndentBody(Row, Col),
}

#[derive(Debug)]
pub enum Port<'a> {
    Space(Space, Row, Col),
    Name(Row, Col),
    Colon(Row, Col),
    Type(&'a Type<'a>, Row, Col),
    IndentName(Row, Col),
    IndentColon(Row, Col),
    IndentType(Row, Col),
}

#[derive(Debug)]
pub enum DeclType<'a> {
    Space(Space, Row, Col),
    Name(Row, Col),
    Alias(&'a TypeAlias<'a>, Row, Col),
    Union(&'a CustomType<'a>, Row, Col),
    IndentName(Row, Col),
}

#[derive(Debug)]
pub enum TypeAlias<'a> {
    Space(Space, Row, Col),
    Name(Row, Col),
    Equals(Row, Col),
    Body(&'a Type<'a>, Row, Col),
    IndentEquals(Row, Col),
    IndentBody(Row, Col),
}

#[derive(Debug)]
pub enum CustomType<'a> {
    Space(Space, Row, Col),
    Name(Row, Col),
    Equals(Row, Col),
    Bar(Row, Col),
    Variant(Row, Col),
    VariantArg(&'a Type<'a>, Row, Col),
    IndentEquals(Row, Col),
    IndentBar(Row, Col),
    IndentAfterBar(Row, Col),
    IndentAfterEquals(Row, Col),
}

// =============================================================================
// Expression Errors
// =============================================================================

#[derive(Debug)]
pub enum Expr<'a> {
    Let(&'a Let<'a>, Row, Col),
    Case(&'a Case<'a>, Row, Col),
    If(&'a If<'a>, Row, Col),
    List(&'a List<'a>, Row, Col),
    Record(&'a Record<'a>, Row, Col),
    Tuple(&'a Tuple<'a>, Row, Col),
    Func(&'a Func<'a>, Row, Col),
    Dot(Row, Col),
    Access(Row, Col),
    OperatorRight(&'a str, Row, Col),
    OperatorReserved(BadOperator, Row, Col),
    Start(Row, Col),
    Char(Char, Row, Col),
    String(StringError, Row, Col),
    Number(Number, Row, Col),
    Space(Space, Row, Col),
    EndlessShader(Row, Col),
    ShaderProblem(Row, Col),
    IndentOperatorRight(&'a str, Row, Col),
}

#[derive(Debug)]
pub enum Record<'a> {
    Open(Row, Col),
    End(Row, Col),
    Field(Row, Col),
    Equals(Row, Col),
    Expr(&'a Expr<'a>, Row, Col),
    Space(Space, Row, Col),
    IndentOpen(Row, Col),
    IndentEnd(Row, Col),
    IndentField(Row, Col),
    IndentEquals(Row, Col),
    IndentExpr(Row, Col),
}

#[derive(Debug)]
pub enum Tuple<'a> {
    Expr(&'a Expr<'a>, Row, Col),
    Space(Space, Row, Col),
    End(Row, Col),
    OperatorClose(Row, Col),
    OperatorReserved(BadOperator, Row, Col),
    IndentExpr1(Row, Col),
    IndentExprN(Row, Col),
    IndentEnd(Row, Col),
}

#[derive(Debug)]
pub enum List<'a> {
    Space(Space, Row, Col),
    Open(Row, Col),
    Expr(&'a Expr<'a>, Row, Col),
    End(Row, Col),
    IndentOpen(Row, Col),
    IndentEnd(Row, Col),
    IndentExpr(Row, Col),
}

#[derive(Debug)]
pub enum Func<'a> {
    Space(Space, Row, Col),
    Arg(&'a Pattern<'a>, Row, Col),
    Body(&'a Expr<'a>, Row, Col),
    Arrow(Row, Col),
    IndentArg(Row, Col),
    IndentArrow(Row, Col),
    IndentBody(Row, Col),
}

#[derive(Debug)]
pub enum Case<'a> {
    Space(Space, Row, Col),
    Of(Row, Col),
    Pattern(&'a Pattern<'a>, Row, Col),
    Arrow(Row, Col),
    Expr(&'a Expr<'a>, Row, Col),
    Branch(&'a Expr<'a>, Row, Col),
    IndentOf(Row, Col),
    IndentExpr(Row, Col),
    IndentPattern(Row, Col),
    IndentArrow(Row, Col),
    IndentBranch(Row, Col),
    PatternAlignment(u16, Row, Col),
}

#[derive(Debug)]
pub enum If<'a> {
    Space(Space, Row, Col),
    Then(Row, Col),
    Else(Row, Col),
    ElseBranchStart(Row, Col),
    Condition(&'a Expr<'a>, Row, Col),
    ThenBranch(&'a Expr<'a>, Row, Col),
    ElseBranch(&'a Expr<'a>, Row, Col),
    IndentCondition(Row, Col),
    IndentThen(Row, Col),
    IndentThenBranch(Row, Col),
    IndentElseBranch(Row, Col),
    IndentElse(Row, Col),
}

#[derive(Debug)]
pub enum Let<'a> {
    Space(Space, Row, Col),
    In(Row, Col),
    DefAlignment(u16, Row, Col),
    DefName(Row, Col),
    Def(&'a str, &'a Def<'a>, Row, Col),
    Destruct(&'a Destruct<'a>, Row, Col),
    Body(&'a Expr<'a>, Row, Col),
    IndentDef(Row, Col),
    IndentIn(Row, Col),
    IndentBody(Row, Col),
}

#[derive(Debug)]
pub enum Def<'a> {
    Space(Space, Row, Col),
    Type(&'a Type<'a>, Row, Col),
    NameRepeat(Row, Col),
    NameMatch(&'a str, Row, Col),
    Arg(&'a Pattern<'a>, Row, Col),
    Equals(Row, Col),
    Body(&'a Expr<'a>, Row, Col),
    IndentEquals(Row, Col),
    IndentType(Row, Col),
    IndentBody(Row, Col),
    Alignment(u16, Row, Col),
}

#[derive(Debug)]
pub enum Destruct<'a> {
    Space(Space, Row, Col),
    Pattern(&'a Pattern<'a>, Row, Col),
    Equals(Row, Col),
    Body(&'a Expr<'a>, Row, Col),
    IndentEquals(Row, Col),
    IndentBody(Row, Col),
}

// =============================================================================
// Pattern Errors
// =============================================================================

#[derive(Debug)]
pub enum Pattern<'a> {
    Record(&'a PRecord, Row, Col),
    Tuple(&'a PTuple<'a>, Row, Col),
    List(&'a PList<'a>, Row, Col),
    Start(Row, Col),
    Char(Char, Row, Col),
    String(StringError, Row, Col),
    Number(Number, Row, Col),
    Float(u16, Row, Col),
    Alias(Row, Col),
    WildcardNotVar(&'a str, i32, Row, Col),
    Space(Space, Row, Col),
    IndentStart(Row, Col),
    IndentAlias(Row, Col),
}

#[derive(Debug)]
pub enum PRecord {
    Open(Row, Col),
    End(Row, Col),
    Field(Row, Col),
    Space(Space, Row, Col),
    IndentOpen(Row, Col),
    IndentEnd(Row, Col),
    IndentField(Row, Col),
}

#[derive(Debug)]
pub enum PTuple<'a> {
    Open(Row, Col),
    End(Row, Col),
    Expr(&'a Pattern<'a>, Row, Col),
    Space(Space, Row, Col),
    IndentEnd(Row, Col),
    IndentExpr1(Row, Col),
    IndentExprN(Row, Col),
}

#[derive(Debug)]
pub enum PList<'a> {
    Open(Row, Col),
    End(Row, Col),
    Expr(&'a Pattern<'a>, Row, Col),
    Space(Space, Row, Col),
    IndentOpen(Row, Col),
    IndentEnd(Row, Col),
    IndentExpr(Row, Col),
}

// =============================================================================
// Type Errors
// =============================================================================

#[derive(Debug)]
pub enum Type<'a> {
    Record(&'a TRecord<'a>, Row, Col),
    Tuple(&'a TTuple<'a>, Row, Col),
    Start(Row, Col),
    Space(Space, Row, Col),
    IndentStart(Row, Col),
}

#[derive(Debug)]
pub enum TRecord<'a> {
    Open(Row, Col),
    End(Row, Col),
    Field(Row, Col),
    Colon(Row, Col),
    Type(&'a Type<'a>, Row, Col),
    Space(Space, Row, Col),
    IndentOpen(Row, Col),
    IndentField(Row, Col),
    IndentColon(Row, Col),
    IndentType(Row, Col),
    IndentEnd(Row, Col),
}

#[derive(Debug)]
pub enum TTuple<'a> {
    Open(Row, Col),
    End(Row, Col),
    Type(&'a Type<'a>, Row, Col),
    Space(Space, Row, Col),
    IndentType1(Row, Col),
    IndentTypeN(Row, Col),
    IndentEnd(Row, Col),
}

// =============================================================================
// Literal Errors (no lifetimes - leaf types)
// =============================================================================

#[derive(Debug)]
pub enum Char {
    Endless,
    Escape(Escape),
    NotString(u16),
}

#[derive(Debug)]
pub enum StringError {
    EndlessSingle,
    EndlessMulti,
    Escape(Escape),
}

#[derive(Debug)]
pub enum Escape {
    Unknown,
    BadUnicodeFormat(u16),
    BadUnicodeCode(u16),
    BadUnicodeLength {
        code: u16,
        expected: i32,
        actual: i32,
    },
}

#[derive(Debug)]
pub enum Number {
    End,
    Dot(i32),
    HexDigit,
    NoLeadingZero,
}

// =============================================================================
// Misc (no lifetimes - leaf types)
// =============================================================================

#[derive(Debug)]
pub enum Space {
    HasTab,
    EndlessMultiComment,
}

#[derive(Debug)]
pub enum BadOperator {
    Dot,
    Pipe,
    Arrow,
    Equals,
    HasType,
}
