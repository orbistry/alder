//! Syntax error types for the Alder parser.
//!
//! Modeled on Elm's `Reporting/Error/Syntax.hs`: one enum per construct,
//! nested through `&'a` so a leaf error carries its full context.
//! `Expr`, `Pattern`, `Type` … here are ERROR types, not AST types.

use crate::keyword::{Keyword, SqlWord};
use crate::{Col, Row};
use alder_source::{AssignOp, BinOp};

// ============================================================================
// Top level
// ============================================================================

#[derive(Debug)]
pub enum Error<'a> {
    ParseError(&'a Module<'a>),
}

#[derive(Debug)]
pub enum Module<'a> {
    Item(&'a Item<'a>, Row, Col),
    /// A second item on the same line as the previous one (§2.1 rule 3).
    SameLine(Row, Col),
    /// Something that is not an item start after the last item (e.g. a stray `}`).
    BadEnd(Row, Col),
}

// ============================================================================
// Items
// ============================================================================

#[derive(Debug)]
pub enum Item<'a> {
    /// Not an item keyword.
    Start(Row, Col),
    /// `pub` followed by something that is not an item.
    AfterPub(Row, Col),
    /// `;` where an item was expected — items are separated by line breaks, not `;`.
    Semicolon(Row, Col),
    Attribute(&'a Attribute<'a>, Row, Col),
    Import(&'a Import<'a>, Row, Col),
    Fn(&'a Fn<'a>, Row, Col),
    Let(&'a Let<'a>, Row, Col),
    TypeAlias(&'a TypeAlias<'a>, Row, Col),
    Enum(&'a Enum<'a>, Row, Col),
    Trait(&'a Trait<'a>, Row, Col),
    Impl(&'a Impl<'a>, Row, Col),
    ErrorDecl(&'a ErrorDecl<'a>, Row, Col),
    Component(&'a Component<'a>, Row, Col),
    Table(&'a Table<'a>, Row, Col),
    Schema(&'a Schema<'a>, Row, Col),
    Macro(Macro, Row, Col),
    Comptime(&'a Block<'a>, Row, Col),
    Test(&'a Test<'a>, Row, Col),
    Tests(&'a Tests<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Attribute<'a> {
    /// `#` not followed by `[`.
    Open(Row, Col),
    Name(Row, Col),
    Arg(&'a Expr<'a>, Row, Col),
    /// Expected `,` or `)`.
    ArgEnd(Row, Col),
    /// Expected `]`.
    End(Row, Col),
    /// Attribute followed by EOF or `}`.
    Dangling(Row, Col),
}

#[derive(Debug)]
pub enum Import<'a> {
    Path(&'a ModulePath, Row, Col),
    /// After `.`: expected `{` or `*`.
    Tail(Row, Col),
    /// Inside `{ }`: expected a name.
    Name(Row, Col),
    /// `as` inside `{ }` not followed by a name.
    NameAlias(Row, Col),
    /// Expected `,` or `}`.
    NamesEnd(Row, Col),
    /// `as` not followed by a lowercase name.
    Alias(Row, Col),
    /// `pub import @x/y` without `.{ … }` or `.*`.
    PubNeedsNames(Row, Col),
    /// Bare `import @alder/test`: the last segment is a reserved word, so it cannot
    /// be bound — write `as name` or `.{ … }`. Position of the segment.
    ReservedBinding(Keyword, Row, Col),
    /// Bare `import ~`: no segment to bind — write `as name`, `.{ … }` or `.*`.
    RootOnly(Row, Col),
}

/// Segments are keyword-insensitive (`raw_lower`, §2.4): only their shape can fail.
#[derive(Debug)]
pub enum ModulePath {
    /// Expected `@` or `~`.
    Start(Row, Col),
    Author(Row, Col),
    Slash(Row, Col),
    Package(Row, Col),
    /// `/` not followed by a lowercase name.
    Segment(Row, Col),
}

#[derive(Debug)]
pub enum Fn<'a> {
    Name(Row, Col),
    Params(&'a Params<'a>, Row, Col),
    Ret(&'a Type<'a>, Row, Col),
    Where(&'a Where<'a>, Row, Col),
    Body(&'a Block<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Params<'a> {
    /// Expected `(`.
    Open(Row, Col),
    Pattern(&'a Pattern<'a>, Row, Col),
    /// Type after `:`.
    Type(&'a Type<'a>, Row, Col),
    /// Expected `,` or `)`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum Where<'a> {
    /// Expected a lowercase type variable.
    Var(Row, Col),
    /// Expected `:` or `.Assoc ==`.
    Colon(Row, Col),
    /// Expected a trait path.
    Bound(Row, Col),
    AssocName(Row, Col),
    AssocEq(Row, Col),
    Type(&'a Type<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Let<'a> {
    Pattern(&'a Pattern<'a>, Row, Col),
    Type(&'a Type<'a>, Row, Col),
    Equals(Row, Col),
    Body(&'a Expr<'a>, Row, Col),
}

#[derive(Debug)]
pub enum TypeAlias<'a> {
    Name(Row, Col),
    Params(&'a TypeParams, Row, Col),
    /// After `=`.
    Body(&'a Type<'a>, Row, Col),
}

#[derive(Debug)]
pub enum TypeParams {
    /// Expected `[` — reported only by callers that require parameters (`trait`).
    Open(Row, Col),
    Var(Row, Col),
    /// Expected `,` or `]`.
    End(Row, Col),
    /// `[]`
    Empty(Row, Col),
}

#[derive(Debug)]
pub enum Enum<'a> {
    Name(Row, Col),
    Params(&'a TypeParams, Row, Col),
    Open(Row, Col),
    /// Expected an uppercase variant name.
    Variant(Row, Col),
    VariantArg(&'a Type<'a>, Row, Col),
    VariantArgEnd(Row, Col),
    VariantRecord(&'a TRecord<'a>, Row, Col),
    /// `Rect { r | width: Number }` — record payloads take no extension. Position of `r`.
    VariantRecordExt(Row, Col),
    /// Expected `,` or `}`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum Trait<'a> {
    Name(Row, Col),
    Params(&'a TypeParams, Row, Col),
    Where(&'a Where<'a>, Row, Col),
    Open(Row, Col),
    /// Expected `type`, `fn` or `}`.
    Item(Row, Col),
    /// A second item on the same line as the previous one (§2.1 rule 3).
    SameLine(Row, Col),
    /// `;` after an item — trait items are separated by line breaks, not `;`.
    Semicolon(Row, Col),
    AssocType(Row, Col),
    /// `type Item = …` inside a trait.
    AssocTypeHasBody(Row, Col),
    Fn(&'a Fn<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Impl<'a> {
    /// Expected a trait path.
    Trait(Row, Col),
    /// `::` in the trait path not followed by a name (`impl Show::[User]`).
    PathMember(Row, Col),
    /// Expected `[`.
    Open(Row, Col),
    Arg(&'a Type<'a>, Row, Col),
    ArgEnd(Row, Col),
    Where(&'a Where<'a>, Row, Col),
    BodyOpen(Row, Col),
    /// Expected `type`, `fn` or `}`.
    Item(Row, Col),
    /// A second item on the same line as the previous one (§2.1 rule 3).
    SameLine(Row, Col),
    /// `;` after an item — impl items are separated by line breaks, not `;`.
    Semicolon(Row, Col),
    AssocType(Row, Col),
    AssocEquals(Row, Col),
    AssocBody(&'a Type<'a>, Row, Col),
    Fn(&'a Fn<'a>, Row, Col),
}

#[derive(Debug)]
pub enum ErrorDecl<'a> {
    Name(Row, Col),
    Open(Row, Col),
    Tag(&'a TagVariant<'a>, Row, Col),
    /// Expected `,` or `}`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum TagVariant<'a> {
    /// `:` not followed by a lowercase name.
    Name(Row, Col),
    Arg(&'a Type<'a>, Row, Col),
    ArgEnd(Row, Col),
}

#[derive(Debug)]
pub enum Component<'a> {
    Name(Row, Col),
    Params(&'a Params<'a>, Row, Col),
    Body(&'a Block<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Table<'a> {
    Name(Row, Col),
    Open(Row, Col),
    /// Expected a column name.
    Column(Row, Col),
    Colon(Row, Col),
    Builder(&'a Expr<'a>, Row, Col),
    ModifierArg(&'a Expr<'a>, Row, Col),
    ModifierArgEnd(Row, Col),
    End(Row, Col),
}

#[derive(Debug)]
pub enum Schema<'a> {
    Name(Row, Col),
    /// `from` not followed by a table name.
    From(Row, Col),
    Open(Row, Col),
    /// Expected `pick`, a field name, or `}`.
    Item(Row, Col),
    PickName(Row, Col),
    Colon(Row, Col),
    Type(&'a Type<'a>, Row, Col),
    Rule(Row, Col),
    RuleArg(&'a Expr<'a>, Row, Col),
    RuleArgEnd(Row, Col),
    End(Row, Col),
}

#[derive(Debug)]
pub enum Macro {
    Name(Row, Col),
    /// Expected `(` after the macro name.
    ParamsOpen(Row, Col),
    Param(Row, Col),
    ParamEnd(Row, Col),
    /// `{` expected, or raw body problem.
    Body(RawTokens, Row, Col),
}

#[derive(Debug)]
pub enum Test<'a> {
    /// `test` not followed by a string.
    Name(Row, Col),
    NameString(StringError, Row, Col),
    Body(&'a Block<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Tests<'a> {
    Open(Row, Col),
    Item(&'a Item<'a>, Row, Col),
    /// A second item on the same line as the previous one (§2.1 rule 3).
    SameLine(Row, Col),
    End(Row, Col),
}

// ============================================================================
// Blocks and statements
// ============================================================================

#[derive(Debug)]
pub enum Block<'a> {
    Open(Row, Col),
    Stmt(&'a Stmt<'a>, Row, Col),
    /// A second statement on the same line as the previous one.
    SameLine(Row, Col),
    /// `{ name: …` in block position — probably a record; wrap it in parentheses.
    LooksLikeRecord(Row, Col),
    /// Expected a statement or `}`.
    End(Row, Col),
    /// Nested past `MAX_NESTING` (§10.44); the block's `{`.
    TooDeep(Row, Col),
}

#[derive(Debug)]
pub enum Stmt<'a> {
    Let(&'a Let<'a>, Row, Col),
    /// `use` not followed by a path.
    Use(Row, Col),
    /// A `::` or `.` member after the provider path (`use Db::x`,
    /// `use Db.insert(u)`): `use` names a provider, not a member.
    UseMember(Row, Col),
    For(&'a For<'a>, Row, Col),
    While(&'a While<'a>, Row, Col),
    Return(&'a Expr<'a>, Row, Col),
    Break(&'a Expr<'a>, Row, Col),
    Assert(&'a Expr<'a>, Row, Col),
    Expr(&'a Expr<'a>, Row, Col),
    /// Left side of the assignment operator is not a place
    /// (renderer: for `/=` mention `!=`).
    AssignTarget(AssignOp, Row, Col),
    AssignValue(&'a Expr<'a>, Row, Col),
    /// Statements are not `;`-terminated.
    Semicolon(Row, Col),
}

#[derive(Debug)]
pub enum Provide<'a> {
    Name(Row, Col),
    Equals(Row, Col),
    Value(&'a Expr<'a>, Row, Col),
    Body(&'a Block<'a>, Row, Col),
}

#[derive(Debug)]
pub enum For<'a> {
    Pattern(&'a Pattern<'a>, Row, Col),
    In(Row, Col),
    Iter(&'a Expr<'a>, Row, Col),
    Body(&'a Block<'a>, Row, Col),
}

#[derive(Debug)]
pub enum While<'a> {
    Condition(&'a Expr<'a>, Row, Col),
    Body(&'a Block<'a>, Row, Col),
}

// ============================================================================
// Expressions
// ============================================================================

#[derive(Debug)]
pub enum Expr<'a> {
    Start(Row, Col),
    /// A reserved word where an expression was expected (`else`, `match` …).
    Reserved(Keyword, Row, Col),
    /// Inside `query { }`: a SQL word used as a value (`where limit > 3`).
    SqlKeyword(SqlWord, Row, Col),
    Number(Number, Row, Col),
    String(StringError, Row, Col),
    Template(&'a Template<'a>, Row, Col),
    TaggedTemplate(&'a Template<'a>, Row, Col),
    Array(&'a Array<'a>, Row, Col),
    Tuple(&'a Tuple<'a>, Row, Col),
    Record(&'a Record<'a>, Row, Col),
    RecordCtor(&'a Record<'a>, Row, Col),
    Block(&'a Block<'a>, Row, Col),
    Lambda(&'a Lambda<'a>, Row, Col),
    If(&'a If<'a>, Row, Col),
    Match(&'a Match<'a>, Row, Col),
    Loop(&'a Block<'a>, Row, Col),
    Provide(&'a Provide<'a>, Row, Col),
    Call(&'a Call<'a>, Row, Col),
    Index(&'a Index<'a>, Row, Col),
    Tag(&'a Tag<'a>, Row, Col),
    State(&'a State<'a>, Row, Col),
    Style(&'a Style<'a>, Row, Col),
    Query(&'a Query<'a>, Row, Col),
    Markup(&'a Markup<'a>, Row, Col),
    MacroCall(RawTokens, Row, Col),
    /// `::` not followed by a name.
    PathMember(Row, Col),
    /// `.` not followed by a field name, digits or `await`.
    Access(Row, Col),
    /// Missing operand after `-` or `!` (or `^` inside `query { }`, via
    /// `pinned_value`): `postfix()` failed with `Start` at the operand
    /// position. Any other operand error propagates unchanged (§6.0).
    Unary(Row, Col),
    /// `^` outside `query { }` and patterns.
    PinOutsideQuery(Row, Col),
    /// `_` anywhere but as a whole call argument.
    Placeholder(Row, Col),
    OperatorReserved(BadOperator, Row, Col),
    /// Operator with no right operand: `unary()` failed with `Start` at the
    /// operand position. Any other operand error propagates unchanged (§6.0).
    OperatorRight(BinOp, Row, Col),
    /// `</` in expression position.
    UnexpectedClose(Row, Col),
    /// Nested past `MAX_NESTING` (§10.44); the expression's first byte.
    TooDeep(Row, Col),
}

#[derive(Debug)]
pub enum Template<'a> {
    /// Position of the opening backtick.
    Endless(Row, Col),
    Escape(Escape, Row, Col),
    /// `${}`
    HoleEmpty(Row, Col),
    HoleExpr(&'a Expr<'a>, Row, Col),
    /// `${ expr` not followed by `}`.
    HoleEnd(Row, Col),
}

#[derive(Debug)]
pub enum Array<'a> {
    Expr(&'a Expr<'a>, Row, Col),
    /// Expected `,` or `]`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum Tuple<'a> {
    Expr(&'a Expr<'a>, Row, Col),
    /// Expected `,` or `)`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum Record<'a> {
    /// Expected a field name or `..`.
    Field(Row, Col),
    Spread(&'a Expr<'a>, Row, Col),
    Expr(&'a Expr<'a>, Row, Col),
    /// Expected `,` or `}`.
    End(Row, Col),
    /// `{ x = 1 }` (Elm habit).
    EqualsNotColon(Row, Col),
}

#[derive(Debug)]
pub enum Lambda<'a> {
    Params(&'a Params<'a>, Row, Col),
    Ret(&'a Type<'a>, Row, Col),
    Body(&'a Expr<'a>, Row, Col),
    Block(&'a Block<'a>, Row, Col),
    /// `fn() 1 += 2`: the left side of the assignment body is not a place
    /// (renderer: for `/=` mention `!=`). Position is the target's start.
    AssignTarget(AssignOp, Row, Col),
    /// `fn() x +=` with no value.
    AssignValue(&'a Expr<'a>, Row, Col),
}

#[derive(Debug)]
pub enum If<'a> {
    Condition(&'a Expr<'a>, Row, Col),
    Then(&'a Block<'a>, Row, Col),
    /// `if x then` (Elm habit).
    ThenKeyword(Row, Col),
    /// `else` not followed by `if` or `{`.
    ElseBranchStart(Row, Col),
    Else(&'a Block<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Match<'a> {
    Scrutinee(&'a Expr<'a>, Row, Col),
    /// Expected `{` (renderer hint if `of` is found).
    Open(Row, Col),
    Arm(&'a Arm<'a>, Row, Col),
    /// Expected `,`, a pattern, or `}`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum Arm<'a> {
    Pattern(&'a Pattern<'a>, Row, Col),
    Guard(&'a Expr<'a>, Row, Col),
    /// Expected `=>` (renderer hint if `->` is found).
    Arrow(Row, Col),
    Body(&'a Expr<'a>, Row, Col),
    Block(&'a Block<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Call<'a> {
    Arg(&'a Expr<'a>, Row, Col),
    /// Expected `,` or `)`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum Index<'a> {
    Expr(&'a Expr<'a>, Row, Col),
    /// Expected `]`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum Tag<'a> {
    /// `:` not followed by a lowercase name.
    Name(Row, Col),
    Arg(&'a Expr<'a>, Row, Col),
    End(Row, Col),
}

#[derive(Debug)]
pub enum State<'a> {
    /// `state` not followed by `(`.
    Open(Row, Col),
    Expr(&'a Expr<'a>, Row, Col),
    End(Row, Col),
}

#[derive(Debug)]
pub enum Style<'a> {
    Open(Row, Col),
    Key(Row, Col),
    KeyString(StringError, Row, Col),
    Colon(Row, Col),
    Value(&'a Expr<'a>, Row, Col),
    Dimension(Number, Row, Col),
    Nested(&'a Style<'a>, Row, Col),
    /// Expected `,` or `}`.
    End(Row, Col),
    /// Nested past `MAX_NESTING` (§10.44); the block's `{`.
    TooDeep(Row, Col),
}

#[derive(Debug)]
pub enum Query<'a> {
    Open(Row, Col),
    /// Expected `select`, `insert`, `update`, or `delete`.
    Verb(Row, Col),
    Select(&'a Select<'a>, Row, Col),
    Insert(&'a Insert<'a>, Row, Col),
    Update(&'a Update<'a>, Row, Col),
    Delete(&'a Delete<'a>, Row, Col),
    /// A clause that is out of order or repeated (`where` after `orderBy`, a second
    /// `limit`). Carries `Clause`, not `SqlWord`: `where` is a `Keyword`, not a SQL word.
    ClauseOrder(Clause, Row, Col),
    /// Expected a clause or `}`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum Select<'a> {
    /// Expected `{` or `*`.
    Projection(Row, Col),
    ProjectionExpr(&'a Expr<'a>, Row, Col),
    ProjectionEnd(Row, Col),
    From(Row, Col),
    Table(TableRef, Row, Col),
    Join(&'a Join<'a>, Row, Col),
    Where(&'a Expr<'a>, Row, Col),
    GroupBy(&'a Expr<'a>, Row, Col),
    OrderBy(&'a Expr<'a>, Row, Col),
    Limit(&'a Expr<'a>, Row, Col),
    Offset(&'a Expr<'a>, Row, Col),
}

#[derive(Debug)]
pub enum TableRef {
    Name(Row, Col),
    /// `as` not followed by a name.
    Alias(Row, Col),
}

#[derive(Debug)]
pub enum Join<'a> {
    /// `left` / `inner` not followed by `join`.
    Keyword(Row, Col),
    Table(TableRef, Row, Col),
    On(Row, Col),
    Condition(&'a Expr<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Insert<'a> {
    Into(Row, Col),
    Table(Row, Col),
    Values(Row, Col),
    /// `values` operand is not `^…`.
    Pin(Row, Col),
    Value(&'a Expr<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Update<'a> {
    Table(Row, Col),
    Set(Row, Col),
    /// `set` not followed by `{`.
    RecordOpen(Row, Col),
    Record(&'a Record<'a>, Row, Col),
    Where(&'a Expr<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Delete<'a> {
    From(Row, Col),
    Table(Row, Col),
    Where(&'a Expr<'a>, Row, Col),
}

// ============================================================================
// Markup
// ============================================================================

#[derive(Debug)]
pub enum Markup<'a> {
    /// `<` not followed by an element name.
    Name(Row, Col),
    Attr(&'a Attr<'a>, Row, Col),
    /// Expected an attribute, `>` or `/>`.
    TagEnd(Row, Col),
    Child(&'a Child<'a>, Row, Col),
    /// `</` not followed by a name.
    CloseName(Row, Col),
    CloseMismatch {
        expected: &'a str,
        found: &'a str,
        row: Row,
        col: Col,
    },
    /// `</name` not followed by `>`.
    CloseEnd(Row, Col),
    /// EOF before the closing tag; position of the opening tag.
    Unclosed {
        name: &'a str,
        row: Row,
        col: Col,
    },
    /// EOF before `</>`.
    FragmentUnclosed(Row, Col),
}

#[derive(Debug)]
pub enum Attr<'a> {
    /// `=` not followed by a string or `{`.
    Value(Row, Col),
    String(StringError, Row, Col),
    Expr(&'a Expr<'a>, Row, Col),
    /// Expected `}`.
    ExprEnd(Row, Col),
}

#[derive(Debug)]
pub enum Child<'a> {
    /// `{}`
    HoleEmpty(Row, Col),
    Hole(&'a Expr<'a>, Row, Col),
    HoleEnd(Row, Col),
    /// A bare `}` in text (write `{"}"}`).
    StrayBrace(Row, Col),
    Element(&'a Markup<'a>, Row, Col),
    If(&'a DirIf<'a>, Row, Col),
    For(&'a DirFor<'a>, Row, Col),
    Match(&'a DirMatch<'a>, Row, Col),
    /// `@word` that is not if/for/match.
    UnknownDirective(Row, Col),
    /// `@else` with no preceding `@if`.
    StrayElse(Row, Col),
    /// `@empty` with no preceding `@for`.
    StrayEmpty(Row, Col),
    /// `let` / `use` inside a child block.
    Stmt(&'a Stmt<'a>, Row, Col),
    /// Nested past `MAX_NESTING` (§10.44); the child's first byte.
    TooDeep(Row, Col),
}

#[derive(Debug)]
pub enum DirIf<'a> {
    Condition(&'a Expr<'a>, Row, Col),
    Body(&'a ChildBlock<'a>, Row, Col),
    /// `@else` not followed by `if` or `{`.
    ElseBranchStart(Row, Col),
    Else(&'a ChildBlock<'a>, Row, Col),
}

#[derive(Debug)]
pub enum DirFor<'a> {
    Pattern(&'a Pattern<'a>, Row, Col),
    In(Row, Col),
    Iter(&'a Expr<'a>, Row, Col),
    /// `;` not followed by `key`.
    Key(Row, Col),
    KeyExpr(&'a Expr<'a>, Row, Col),
    Body(&'a ChildBlock<'a>, Row, Col),
    Empty(&'a ChildBlock<'a>, Row, Col),
}

#[derive(Debug)]
pub enum DirMatch<'a> {
    Scrutinee(&'a Expr<'a>, Row, Col),
    Open(Row, Col),
    Pattern(&'a Pattern<'a>, Row, Col),
    Guard(&'a Expr<'a>, Row, Col),
    Arrow(Row, Col),
    Body(&'a Child<'a>, Row, Col),
    /// Bare text after `=>` (would swallow the next arm); wrap it in `{ }`.
    BareText(Row, Col),
    Block(&'a ChildBlock<'a>, Row, Col),
    /// Expected `,`, a pattern, or `}`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum ChildBlock<'a> {
    Open(Row, Col),
    Item(&'a Child<'a>, Row, Col),
    End(Row, Col),
}

// ============================================================================
// Patterns
// ============================================================================

#[derive(Debug)]
pub enum Pattern<'a> {
    Start(Row, Col),
    Reserved(Keyword, Row, Col),
    /// Inside `query { }`: a SQL word used as a binding (`select`, `limit`).
    SqlKeyword(SqlWord, Row, Col),
    Number(Number, Row, Col),
    String(StringError, Row, Col),
    Pin(&'a Expr<'a>, Row, Col),
    /// `::` not followed by a name.
    PathMember(Row, Col),
    /// `Foo::bar` — a value path where a pattern was expected; pin it
    /// (`^Foo::bar`) to compare against its value. Position of the path start.
    PathVar(Row, Col),
    Ctor(&'a PCtor<'a>, Row, Col),
    Tag(&'a PCtor<'a>, Row, Col),
    /// `:` not followed by a lowercase name.
    TagName(Row, Col),
    Tuple(&'a PTuple<'a>, Row, Col),
    Array(&'a PArray<'a>, Row, Col),
    Record(&'a PRecord<'a>, Row, Col),
    /// `as` not followed by a lowercase name.
    Alias(Row, Col),
    /// `_foo` (name, width).
    WildcardNotVar(&'a str, usize, Row, Col),
    /// Nested past `MAX_NESTING` (§10.44); the pattern's first byte.
    TooDeep(Row, Col),
}

#[derive(Debug)]
pub enum PCtor<'a> {
    Arg(&'a Pattern<'a>, Row, Col),
    /// Expected `,` or `)`.
    End(Row, Col),
    Record(&'a PRecord<'a>, Row, Col),
}

#[derive(Debug)]
pub enum PTuple<'a> {
    Pattern(&'a Pattern<'a>, Row, Col),
    End(Row, Col),
}

#[derive(Debug)]
pub enum PArray<'a> {
    Pattern(&'a Pattern<'a>, Row, Col),
    /// `..` must be last.
    RestNotLast(Row, Col),
    /// `..` followed by a reserved word (`[..type]`) or, in `query { }`, a SQL word.
    RestName(Row, Col),
    End(Row, Col),
}

#[derive(Debug)]
pub enum PRecord<'a> {
    /// Expected a field name or `..`.
    Field(Row, Col),
    Pattern(&'a Pattern<'a>, Row, Col),
    RestNotLast(Row, Col),
    End(Row, Col),
}

// ============================================================================
// Types
// ============================================================================

#[derive(Debug)]
pub enum Type<'a> {
    Start(Row, Col),
    Reserved(Keyword, Row, Col),
    PathMember(Row, Col),
    Args(&'a TArgs<'a>, Row, Col),
    Fn(&'a TFn<'a>, Row, Col),
    Tuple(&'a TTuple<'a>, Row, Col),
    Record(&'a TRecord<'a>, Row, Col),
    ErrorRow(&'a TErrorRow<'a>, Row, Col),
    /// Nested past `MAX_NESTING` (§10.44); the type's first byte.
    TooDeep(Row, Col),
}

#[derive(Debug)]
pub enum TArgs<'a> {
    Type(&'a Type<'a>, Row, Col),
    /// `Array[]`
    Empty(Row, Col),
    /// Expected `,` or `]`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum TFn<'a> {
    Open(Row, Col),
    Param(&'a Type<'a>, Row, Col),
    ParamEnd(Row, Col),
    Arrow(Row, Col),
    Ret(&'a Type<'a>, Row, Col),
}

#[derive(Debug)]
pub enum TTuple<'a> {
    Type(&'a Type<'a>, Row, Col),
    End(Row, Col),
}

#[derive(Debug)]
pub enum TRecord<'a> {
    Field(Row, Col),
    /// Expected `:` or `?:` (or `|` after the first name).
    Colon(Row, Col),
    Type(&'a Type<'a>, Row, Col),
    /// `{ r | }` with no fields.
    ExtField(Row, Col),
    End(Row, Col),
}

#[derive(Debug)]
pub enum TErrorRow<'a> {
    /// `[` followed by neither `:tag`, a row variable, nor `]` (`[1]`, `[|]`, `[` at EOF).
    Start(Row, Col),
    Tag(&'a TagVariant<'a>, Row, Col),
    /// `|` not followed by a tag or a variable.
    Ext(Row, Col),
    /// Expected `|` or `]`.
    End(Row, Col),
}

// ============================================================================
// Leaves (no lifetimes)
// ============================================================================

#[derive(Debug)]
pub enum StringError {
    Endless,
    Newline,
    Escape(Escape),
}

#[derive(Debug)]
pub enum Escape {
    Unknown,
    BadUnicodeFormat(u16),
    BadUnicodeCode(u16),
    BadUnicodeLength {
        width: u16,
        expected: i32,
        actual: i32,
    },
}

#[derive(Debug)]
pub enum Number {
    /// `123abc`
    End,
    /// `1.` / `1.x`
    Dot,
    /// `1e` / `1e+`
    Exponent,
    /// `0x` / `0xG`
    HexDigit,
    /// `007`
    NoLeadingZero,
    /// `1.5n`
    BigIntFraction,
}

#[derive(Debug)]
pub enum RawTokens {
    /// Unmatched closer (the byte found).
    Unbalanced(u8),
    /// EOF before the matching closer.
    Endless,
    String(StringError),
    /// Not at the expected opener (`(` for `name!(`, `{` for a macro body);
    /// nothing consumed. The wrapping variant carries the position.
    Open,
}

/// The `select` clauses in their required order (the derived `Ord` is that
/// order); payload of `Query::ClauseOrder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Clause {
    From,
    Join,
    Where,
    GroupBy,
    OrderBy,
    Limit,
    Offset,
}

impl Clause {
    pub const fn as_str(self) -> &'static str {
        match self {
            Clause::From => "from",
            Clause::Join => "join",
            Clause::Where => "where",
            Clause::GroupBy => "groupBy",
            Clause::OrderBy => "orderBy",
            Clause::Limit => "limit",
            Clause::Offset => "offset",
        }
    }
}

#[derive(Debug)]
pub enum BadOperator {
    /// `->` (hint: `=>` in match arms, `fn(A) -> B` in types)
    Arrow,
    /// `|` (hint: `||`, or `|` only between match patterns)
    Bar,
    /// `++` (hint: `Array.concat`, templates)
    PlusPlus,
    /// `::` (hint: paths only, no cons)
    DoubleColon,
    /// `..` (hint: spread only inside records/patterns)
    DotDot,
    /// `<|`
    PipeLeft,
    /// `>>`
    ComposeRight,
    /// `<<`
    ComposeLeft,
    /// `^` (hint: pins only in `query { }` and patterns; no power operator)
    Caret,
}
