//! Source AST for Alder — what `alder-parse` produces before canonicalization.
//! See docs/parser-internals.md §3 for the layout conventions.

use alder_region::{Located, Region};

/// An identifier with its region. Stored inline everywhere.
pub type Name<'a> = Located<&'a str>;

// ============================================================================
// Module and items
// ============================================================================

#[derive(Debug)]
pub struct Module<'a> {
    pub items: &'a [&'a Located<Item<'a>>],
}

impl<'a> Module<'a> {
    /// Top-level imports (including `pub import` re-exports). Imports inside
    /// `tests { }` are only reachable through `ItemKind::Tests`.
    pub fn imports(&self) -> impl Iterator<Item = &'a Import<'a>> + '_ {
        self.items.iter().filter_map(|item| match &item.value.kind {
            ItemKind::Import(import) => Some(*import),
            _ => None,
        })
    }
}

#[derive(Debug)]
pub struct Item<'a> {
    pub attributes: &'a [Located<Attribute<'a>>],
    pub visibility: Visibility,
    pub kind: ItemKind<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    Private,
    Pub(Region),
}

/// `#[derive(Show, Eq)]` → args `[Path(Show), Path(Eq)]`; `#[extern("m", "n")]` → two `Str`s.
#[derive(Clone, Copy, Debug)]
pub struct Attribute<'a> {
    pub name: Name<'a>,
    pub args: &'a [&'a Located<Expr<'a>>],
}

#[derive(Debug)]
pub enum ItemKind<'a> {
    Import(&'a Import<'a>),
    /// `body: None` is a bodiless declaration; canonicalization requires `#[extern]`.
    Fn(&'a FnDecl<'a>),
    /// Includes `let card = style { … }`.
    Let(&'a LetDecl<'a>),
    TypeAlias(&'a TypeAlias<'a>),
    /// `type Name` with no `=`; canonicalization requires `#[extern]`.
    OpaqueType(Name<'a>),
    Enum(&'a EnumDecl<'a>),
    Trait(&'a TraitDecl<'a>),
    Impl(&'a ImplDecl<'a>),
    Error(&'a ErrorDecl<'a>),
    Component(&'a ComponentDecl<'a>),
    Table(&'a TableDecl<'a>),
    Schema(&'a SchemaDecl<'a>),
    Macro(&'a MacroDecl<'a>),
    Comptime(&'a Located<Block<'a>>),
    Test(&'a TestDecl<'a>),
    Tests(&'a [&'a Located<Item<'a>>]),
}

#[derive(Debug)]
pub struct Import<'a> {
    pub path: Located<ModulePath<'a>>,
    pub tail: ImportTail<'a>,
}

#[derive(Clone, Copy, Debug)]
pub struct ModulePath<'a> {
    pub root: ModuleRoot<'a>,
    /// Segments after the root: `@alder/http/client` → `["client"]`; `~/db/users` → `["db", "users"]`.
    /// Keyword-insensitive, like `author` / `package` (`@alder/test` is legal; §2.4).
    pub segments: &'a [Name<'a>],
}

#[derive(Clone, Copy, Debug)]
pub enum ModuleRoot<'a> {
    /// `@author/package`
    Package { author: Name<'a>, package: Name<'a> },
    /// `~`
    Local(Region),
}

#[derive(Clone, Copy, Debug)]
pub enum ImportTail<'a> {
    /// `import @alder/http` — binds the last segment (the package name when there
    /// are no segments). The parser rejects the two forms with nothing legal to
    /// bind: `import ~` (`Import::RootOnly`) and a reserved last segment such as
    /// `import @alder/test` (`Import::ReservedBinding(Test)`).
    Module,
    /// `as h`
    Alias(Name<'a>),
    /// `.{ get, Request as Req }`
    Names(&'a [ImportName<'a>]),
    /// `.*`
    All(Region),
}

#[derive(Clone, Copy, Debug)]
pub struct ImportName<'a> {
    pub name: Name<'a>,
    pub alias: Option<Name<'a>>,
}

#[derive(Debug)]
pub struct FnDecl<'a> {
    pub name: Name<'a>,
    pub params: &'a [Param<'a>],
    pub ret: Option<&'a Located<Type<'a>>>,
    pub where_clause: &'a [Constraint<'a>],
    /// `None` for bodiless functions (extern, trait signatures).
    pub body: Option<&'a Located<Block<'a>>>,
}

#[derive(Clone, Copy, Debug)]
pub struct Param<'a> {
    pub mutable: Option<Region>,
    pub pattern: &'a Located<Pattern<'a>>,
    pub annotation: Option<&'a Located<Type<'a>>>,
}

#[derive(Clone, Copy, Debug)]
pub enum Constraint<'a> {
    /// `a: Show + Eq`
    Bound {
        var: Name<'a>,
        bounds: &'a [Path<'a>],
    },
    /// `i.Item == Number`
    AssocEq {
        var: Name<'a>,
        assoc: Name<'a>,
        typ: &'a Located<Type<'a>>,
    },
}

/// `let [mut] pattern [: Type] = expr` — shared by items and statements.
#[derive(Debug)]
pub struct LetDecl<'a> {
    pub mutable: Option<Region>,
    pub pattern: &'a Located<Pattern<'a>>,
    pub annotation: Option<&'a Located<Type<'a>>>,
    pub value: &'a Located<Expr<'a>>,
}

#[derive(Debug)]
pub struct TypeAlias<'a> {
    pub name: Name<'a>,
    pub params: &'a [Name<'a>],
    pub typ: &'a Located<Type<'a>>,
}

#[derive(Debug)]
pub struct EnumDecl<'a> {
    pub name: Name<'a>,
    pub params: &'a [Name<'a>],
    pub variants: &'a [Variant<'a>],
}

#[derive(Clone, Copy, Debug)]
pub struct Variant<'a> {
    pub name: Name<'a>,
    pub payload: VariantPayload<'a>,
}

#[derive(Clone, Copy, Debug)]
pub enum VariantPayload<'a> {
    Unit,
    Tuple(&'a [&'a Located<Type<'a>>]),
    /// `Rect { width: Number }` — no `r |` extension (`Enum::VariantRecordExt`).
    Record(&'a [FieldType<'a>]),
}

#[derive(Debug)]
pub struct TraitDecl<'a> {
    pub name: Name<'a>,
    pub params: &'a [Name<'a>],
    pub where_clause: &'a [Constraint<'a>],
    pub items: &'a [TraitItem<'a>],
}

#[derive(Clone, Copy, Debug)]
pub enum TraitItem<'a> {
    AssocType(Name<'a>),
    /// `body: None` = required, `Some` = default body.
    Fn(&'a FnDecl<'a>),
}

#[derive(Debug)]
pub struct ImplDecl<'a> {
    pub trait_: Path<'a>,
    pub args: &'a [&'a Located<Type<'a>>],
    pub where_clause: &'a [Constraint<'a>],
    pub items: &'a [ImplItem<'a>],
}

#[derive(Clone, Copy, Debug)]
pub enum ImplItem<'a> {
    AssocType {
        name: Name<'a>,
        typ: &'a Located<Type<'a>>,
    },
    Fn(&'a FnDecl<'a>),
}

#[derive(Debug)]
pub struct ErrorDecl<'a> {
    pub name: Name<'a>,
    pub tags: &'a [TagVariant<'a>],
}

/// `:tag` or `:tag(T1, T2)` — `error` groups and error-row types.
#[derive(Clone, Copy, Debug)]
pub struct TagVariant<'a> {
    /// Without the leading `:`.
    pub name: Name<'a>,
    pub args: &'a [&'a Located<Type<'a>>],
}

#[derive(Debug)]
pub struct ComponentDecl<'a> {
    /// `Counter`, or route-file `page` (either case is accepted).
    pub name: Name<'a>,
    pub params: &'a [Param<'a>],
    pub body: &'a Located<Block<'a>>,
}

#[derive(Debug)]
pub struct TableDecl<'a> {
    pub name: Name<'a>,
    pub columns: &'a [Column<'a>],
}

#[derive(Clone, Copy, Debug)]
pub struct Column<'a> {
    pub name: Name<'a>,
    pub builder: &'a Located<Expr<'a>>,
    pub modifiers: &'a [Modifier<'a>],
}

/// `primaryKey`, `default(now)`, `min(3)` — table modifiers and schema rules.
#[derive(Clone, Copy, Debug)]
pub struct Modifier<'a> {
    pub name: Name<'a>,
    pub args: &'a [&'a Located<Expr<'a>>],
}

#[derive(Debug)]
pub struct SchemaDecl<'a> {
    pub name: Name<'a>,
    pub from: Option<Name<'a>>,
    pub items: &'a [SchemaItem<'a>],
}

#[derive(Clone, Copy, Debug)]
pub enum SchemaItem<'a> {
    Pick(&'a [Name<'a>]),
    Field {
        name: Name<'a>,
        typ: Option<&'a Located<Type<'a>>>,
        rules: &'a [Modifier<'a>],
    },
}

#[derive(Debug)]
pub struct MacroDecl<'a> {
    pub name: Name<'a>,
    pub params: &'a [Name<'a>],
    /// Raw balanced body text between the braces (M5 defines quote/unquote).
    pub body: Located<&'a str>,
}

#[derive(Debug)]
pub struct TestDecl<'a> {
    pub name: Located<&'a str>,
    pub body: &'a Located<Block<'a>>,
}

// ============================================================================
// Blocks and statements
// ============================================================================

#[derive(Debug)]
pub struct Block<'a> {
    pub stmts: &'a [&'a Located<Stmt<'a>>],
    /// Trailing expression = the block's value.
    pub tail: Option<&'a Located<Expr<'a>>>,
}

#[derive(Debug)]
pub enum Stmt<'a> {
    Let(&'a LetDecl<'a>),
    Use(Path<'a>),
    /// A statement in M1, so a trailing `provide … { … }` leaves the enclosing block
    /// without a `tail` (web.md's `handle` relies on it having one — §10.40, M2 decides).
    Provide {
        name: Path<'a>,
        value: &'a Located<Expr<'a>>,
        body: &'a Located<Block<'a>>,
    },
    Assign {
        place: &'a Located<Place<'a>>,
        op: Located<AssignOp>,
        value: &'a Located<Expr<'a>>,
    },
    For {
        pattern: &'a Located<Pattern<'a>>,
        iter: &'a Located<Expr<'a>>,
        body: &'a Located<Block<'a>>,
    },
    While {
        condition: &'a Located<Expr<'a>>,
        body: &'a Located<Block<'a>>,
    },
    Return(Option<&'a Located<Expr<'a>>>),
    Break(Option<&'a Located<Expr<'a>>>),
    Continue,
    /// `assert expr` — compiler-known (power-assert).
    Assert(&'a Located<Expr<'a>>),
    Expr(&'a Located<Expr<'a>>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
}

impl AssignOp {
    pub const fn as_str(self) -> &'static str {
        match self {
            AssignOp::Set => "=",
            AssignOp::Add => "+=",
            AssignOp::Sub => "-=",
            AssignOp::Mul => "*=",
            AssignOp::Div => "/=",
        }
    }
}

/// `lower_ident { '.' lower_ident | '.' digits | '[' expr ']' }`
#[derive(Debug)]
pub struct Place<'a> {
    pub root: Name<'a>,
    pub steps: &'a [PlaceStep<'a>],
}

#[derive(Clone, Copy, Debug)]
pub enum PlaceStep<'a> {
    Field(Name<'a>),
    TupleIndex(Located<u32>),
    Index(&'a Located<Expr<'a>>),
}

// ============================================================================
// Expressions
// ============================================================================

/// `Upper { '::' Upper }` — enum name, constructor path, trait path, component name,
/// or a module-style receiver (`Array` in `Array.map`); canonicalization decides.
#[derive(Clone, Copy, Debug)]
pub struct Path<'a> {
    /// At least one segment.
    pub segments: &'a [Name<'a>],
}

impl<'a> Path<'a> {
    pub fn region(&self) -> Region {
        let first = &self.segments[0].region;
        let last = &self.segments[self.segments.len() - 1].region;
        Region::span_across(first, last)
    }
}

/// A number literal: the JS value and its source spelling (`0xFF` → 255, "0xFF").
#[derive(Clone, Copy, Debug)]
pub struct NumberLit<'a> {
    pub value: f64,
    pub text: &'a str,
}

#[derive(Clone, Copy, Debug)]
pub enum Expr<'a> {
    // ---- literals
    Number(NumberLit<'a>),
    /// Digits without the trailing `n`.
    BigInt(&'a str),
    Str(&'a str),
    Bool(bool),
    Template(&'a [TemplatePart<'a>]),
    TaggedTemplate {
        tag: &'a Located<Expr<'a>>,
        parts: &'a [TemplatePart<'a>],
    },
    Unit,
    // ---- names
    Var(&'a str),
    /// `Some`, `Option::Some`, `Shape`, `Array` (in `Array.map`).
    Path(Path<'a>),
    /// `Show::show`
    PathVar {
        path: Path<'a>,
        name: Name<'a>,
    },
    /// `:not_found(id)`; `args` empty for a bare `:timeout`.
    Tag {
        name: Name<'a>,
        args: &'a [&'a Located<Expr<'a>>],
    },
    /// `_` as a whole call argument.
    Placeholder,
    // ---- aggregates
    Array(&'a [&'a Located<Expr<'a>>]),
    Tuple {
        first: &'a Located<Expr<'a>>,
        second: &'a Located<Expr<'a>>,
        rest: &'a [&'a Located<Expr<'a>>],
    },
    Record(&'a [RecordField<'a>]),
    /// `Shape::Rect { width: 1, height: 2 }`
    RecordCtor {
        path: Path<'a>,
        fields: &'a [RecordField<'a>],
    },
    // ---- postfix
    Call {
        function: &'a Located<Expr<'a>>,
        arguments: &'a [&'a Located<Expr<'a>>],
    },
    Access {
        record: &'a Located<Expr<'a>>,
        field: Name<'a>,
    },
    TupleAccess {
        tuple: &'a Located<Expr<'a>>,
        index: Located<u32>,
    },
    Index {
        target: &'a Located<Expr<'a>>,
        index: &'a Located<Expr<'a>>,
    },
    Await(&'a Located<Expr<'a>>),
    /// `expr?`
    Try(&'a Located<Expr<'a>>),
    // ---- prefix
    Negate(&'a Located<Expr<'a>>),
    Not(&'a Located<Expr<'a>>),
    /// `^expr` inside `query { }`. The operand is a whole postfix chain:
    /// `^user.id` pins `user.id`, `^f(x)` pins the call (§10.20).
    Pin(&'a Located<Expr<'a>>),
    // ---- operators: flat chain, precedence resolved in canonicalization (as Elm)
    BinOps {
        operands: &'a [BinOpOperand<'a>],
        last: &'a Located<Expr<'a>>,
    },
    // ---- control
    Block(&'a Located<Block<'a>>),
    Lambda(&'a Lambda<'a>),
    If {
        branches: &'a [IfBranch<'a>],
        final_else: Option<&'a Located<Block<'a>>>,
    },
    Match {
        scrutinee: &'a Located<Expr<'a>>,
        arms: &'a [MatchArm<'a>],
    },
    Loop(&'a Located<Block<'a>>),
    // ---- framework
    State(&'a Located<Expr<'a>>),
    Style(&'a Style<'a>),
    Query(&'a Query<'a>),
    Markup(&'a Markup<'a>),
    /// `name!( … )` — raw balanced token text until M5.
    MacroCall {
        name: Name<'a>,
        tokens: Located<&'a str>,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum TemplatePart<'a> {
    Text(&'a str),
    Expr(&'a Located<Expr<'a>>),
}

#[derive(Clone, Copy, Debug)]
pub enum RecordField<'a> {
    /// `name: value`, or shorthand `name` (`value == None`).
    Field {
        name: Name<'a>,
        value: Option<&'a Located<Expr<'a>>>,
    },
    /// `..expr`
    Spread(&'a Located<Expr<'a>>),
}

#[derive(Clone, Copy, Debug)]
pub struct BinOpOperand<'a> {
    pub expr: &'a Located<Expr<'a>>,
    pub op: Located<BinOp>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
    /// `??`
    Coalesce,
    /// `|>`
    Pipe,
    /// `in` — query blocks only
    In,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Associativity {
    Left,
    None,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Precedence(pub u16);

impl BinOp {
    /// Fixed table; alder-can's binop resolution reads this instead of `infix`
    /// declarations. Higher binds tighter.
    pub const fn precedence(self) -> (Precedence, Associativity) {
        use Associativity::*;
        match self {
            BinOp::Pipe => (Precedence(0), Left),
            BinOp::Coalesce => (Precedence(1), Right),
            BinOp::Or => (Precedence(2), Left),
            BinOp::And => (Precedence(3), Left),
            BinOp::Eq
            | BinOp::NotEq
            | BinOp::Lt
            | BinOp::LtEq
            | BinOp::Gt
            | BinOp::GtEq
            | BinOp::In => (Precedence(4), None),
            BinOp::Add | BinOp::Sub => (Precedence(6), Left),
            BinOp::Mul | BinOp::Div | BinOp::Rem => (Precedence(7), Left),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::Eq => "==",
            BinOp::NotEq => "!=",
            BinOp::Lt => "<",
            BinOp::LtEq => "<=",
            BinOp::Gt => ">",
            BinOp::GtEq => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
            BinOp::Coalesce => "??",
            BinOp::Pipe => "|>",
            BinOp::In => "in",
        }
    }
}

#[derive(Debug)]
pub struct Lambda<'a> {
    pub params: &'a [Param<'a>],
    pub ret: Option<&'a Located<Type<'a>>>,
    /// `{ … }` bodies are `Expr::Block`; an assignment body (`fn() count += 1`)
    /// is wrapped as a one-statement block with no tail.
    pub body: &'a Located<Expr<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct IfBranch<'a> {
    pub condition: &'a Located<Expr<'a>>,
    pub body: &'a Located<Block<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct MatchArm<'a> {
    /// `p1 | p2`
    pub patterns: &'a [&'a Located<Pattern<'a>>],
    pub guard: Option<&'a Located<Expr<'a>>>,
    /// `Expr::Block` when braced.
    pub body: &'a Located<Expr<'a>>,
}

// ============================================================================
// Style
// ============================================================================

#[derive(Debug)]
pub struct Style<'a> {
    pub entries: &'a [StyleEntry<'a>],
}

#[derive(Clone, Copy, Debug)]
pub struct StyleEntry<'a> {
    pub key: Located<StyleKey<'a>>,
    pub value: StyleValue<'a>,
}

#[derive(Clone, Copy, Debug)]
pub enum StyleKey<'a> {
    Ident(&'a str),
    /// `":hover"`, `"@media (max-width: 600px)"`
    Str(&'a str),
}

#[derive(Clone, Copy, Debug)]
pub enum StyleValue<'a> {
    /// `16px`, `1.5rem`, `100%`, `-8px` — a number immediately followed by a unit.
    /// A leading `-` negates `value` and is kept in `text` (`"-8"`).
    Dimension {
        number: NumberLit<'a>,
        unit: &'a str,
    },
    Expr(&'a Located<Expr<'a>>),
    Nested(&'a Style<'a>),
}

// ============================================================================
// Queries
// ============================================================================

#[derive(Debug)]
pub enum Query<'a> {
    Select(&'a Select<'a>),
    /// `values` must be an `Expr::Pin` (checked by the parser: `Insert::Pin`).
    Insert {
        table: Name<'a>,
        values: &'a Located<Expr<'a>>,
    },
    Update {
        table: Name<'a>,
        set: &'a [RecordField<'a>],
        where_: Option<&'a Located<Expr<'a>>>,
    },
    Delete {
        table: Name<'a>,
        where_: Option<&'a Located<Expr<'a>>>,
    },
}

#[derive(Debug)]
pub struct Select<'a> {
    pub projection: Projection<'a>,
    pub from: TableRef<'a>,
    pub joins: &'a [Join<'a>],
    pub where_: Option<&'a Located<Expr<'a>>>,
    pub group_by: &'a [&'a Located<Expr<'a>>],
    pub order_by: &'a [Order<'a>],
    pub limit: Option<&'a Located<Expr<'a>>>,
    pub offset: Option<&'a Located<Expr<'a>>>,
}

#[derive(Clone, Copy, Debug)]
pub enum Projection<'a> {
    Star(Region),
    Fields(&'a [&'a Located<Expr<'a>>]),
}

#[derive(Clone, Copy, Debug)]
pub struct TableRef<'a> {
    pub name: Name<'a>,
    pub alias: Option<Name<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct Join<'a> {
    pub kind: Located<JoinKind>,
    pub table: TableRef<'a>,
    pub on: &'a Located<Expr<'a>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinKind {
    /// bare `join`
    Plain,
    Inner,
    Left,
}

#[derive(Clone, Copy, Debug)]
pub struct Order<'a> {
    pub expr: &'a Located<Expr<'a>>,
    pub direction: Option<Located<OrderDir>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderDir {
    Asc,
    Desc,
}

// ============================================================================
// Markup
// ============================================================================

/// What `<…>` in expression position produces.
#[derive(Debug)]
pub enum Markup<'a> {
    Element(&'a Element<'a>),
    /// `<> … </>`
    Fragment(&'a [&'a Located<Child<'a>>]),
}

#[derive(Debug)]
pub struct Element<'a> {
    pub name: Located<ElementName<'a>>,
    pub attrs: &'a [Attr<'a>],
    pub children: &'a [&'a Located<Child<'a>>],
    pub self_closing: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum ElementName<'a> {
    /// `div`, `box`, `my-widget`, and keyword-shaped names like `table`, `style` (§2.4)
    Tag(&'a str),
    /// `Spinner`, `Ui::Button`
    Component(Path<'a>),
}

#[derive(Clone, Copy, Debug)]
pub struct Attr<'a> {
    /// May contain `-` (`aria-label`) and may be a reserved word (`type`, `for`; §2.4).
    pub name: Name<'a>,
    /// `None` for a boolean attribute (`<text bold>`).
    pub value: Option<AttrValue<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub enum AttrValue<'a> {
    Str(Located<&'a str>),
    Expr(&'a Located<Expr<'a>>),
}

#[derive(Debug)]
pub enum Child<'a> {
    Element(&'a Element<'a>),
    Fragment(&'a [&'a Located<Child<'a>>]),
    /// Raw text; whitespace-only runs containing a newline are dropped by the parser.
    Text(&'a str),
    /// `{expr}`
    Hole(&'a Located<Expr<'a>>),
    If {
        branches: &'a [ChildIfBranch<'a>],
        final_else: Option<&'a Located<ChildBlock<'a>>>,
    },
    For {
        pattern: &'a Located<Pattern<'a>>,
        iter: &'a Located<Expr<'a>>,
        key: Option<&'a Located<Expr<'a>>>,
        body: &'a Located<ChildBlock<'a>>,
        empty: Option<&'a Located<ChildBlock<'a>>>,
    },
    Match {
        scrutinee: &'a Located<Expr<'a>>,
        arms: &'a [ChildMatchArm<'a>],
    },
}

#[derive(Clone, Copy, Debug)]
pub struct ChildIfBranch<'a> {
    pub condition: &'a Located<Expr<'a>>,
    pub body: &'a Located<ChildBlock<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct ChildMatchArm<'a> {
    pub patterns: &'a [&'a Located<Pattern<'a>>],
    pub guard: Option<&'a Located<Expr<'a>>>,
    /// A bare `child` body is stored as a one-item block.
    pub body: &'a Located<ChildBlock<'a>>,
}

/// `{ … }` after a directive head: setup statements and children.
#[derive(Debug)]
pub struct ChildBlock<'a> {
    pub items: &'a [ChildItem<'a>],
}

#[derive(Clone, Copy, Debug)]
pub enum ChildItem<'a> {
    /// Only `let` / `let mut` and `use` are recognized here.
    Stmt(&'a Located<Stmt<'a>>),
    Child(&'a Located<Child<'a>>),
}

// ============================================================================
// Patterns
// ============================================================================

#[derive(Clone, Copy, Debug)]
pub enum Pattern<'a> {
    Anything,
    Var(&'a str),
    /// `^expr` — compare against an existing value.
    Pin(&'a Located<Expr<'a>>),
    /// `-1` allowed; `text` keeps the sign.
    Number(NumberLit<'a>),
    BigInt(&'a str),
    Str(&'a str),
    Bool(bool),
    Unit,
    /// `None`, `Some(x)`, `Option::Some(x)`
    Ctor {
        path: Path<'a>,
        args: &'a [&'a Located<Pattern<'a>>],
    },
    /// `Rect { width, height: h, .. }`
    CtorRecord {
        path: Path<'a>,
        fields: &'a [FieldPattern<'a>],
        rest: Option<Region>,
    },
    /// `:not_found(id)`
    Tag {
        name: Name<'a>,
        args: &'a [&'a Located<Pattern<'a>>],
    },
    Tuple {
        first: &'a Located<Pattern<'a>>,
        second: &'a Located<Pattern<'a>>,
        rest: &'a [&'a Located<Pattern<'a>>],
    },
    /// `[a, b, ..rest]`, `[a, ..]`
    Array {
        elements: &'a [&'a Located<Pattern<'a>>],
        rest: Option<ArrayRest<'a>>,
    },
    Record {
        fields: &'a [FieldPattern<'a>],
        rest: Option<Region>,
    },
    Alias {
        pattern: &'a Located<Pattern<'a>>,
        name: Name<'a>,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct ArrayRest<'a> {
    /// Region of `..` (plus the name when present).
    pub region: Region,
    pub name: Option<Name<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct FieldPattern<'a> {
    pub name: Name<'a>,
    /// `None` = shorthand binding of the field name.
    pub pattern: Option<&'a Located<Pattern<'a>>>,
}

// ============================================================================
// Types
// ============================================================================

#[derive(Clone, Copy, Debug)]
pub enum Type<'a> {
    /// `a`, and applied higher-kinded variables `f[a]`, `t[f[a]]`.
    Var {
        name: &'a str,
        args: &'a [&'a Located<Type<'a>>],
    },
    /// `User`, `Map[String, Array[User]]`, `Option::Foo`
    Named {
        path: Path<'a>,
        args: &'a [&'a Located<Type<'a>>],
    },
    Fn {
        params: &'a [&'a Located<Type<'a>>],
        ret: &'a Located<Type<'a>>,
    },
    Unit,
    Tuple {
        first: &'a Located<Type<'a>>,
        second: &'a Located<Type<'a>>,
        rest: &'a [&'a Located<Type<'a>>],
    },
    /// `{ r | name: String, nickname?: String }`
    Record {
        fields: &'a [FieldType<'a>],
        ext: Option<Name<'a>>,
    },
    /// `[:not_found(Id) | :timeout | r]`
    ErrorRow {
        tags: &'a [TagVariant<'a>],
        ext: Option<Name<'a>>,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct FieldType<'a> {
    pub field: Name<'a>,
    /// Region of the `?`.
    pub optional: Option<Region>,
    pub typ: &'a Located<Type<'a>>,
}
