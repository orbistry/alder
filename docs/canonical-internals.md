# Canonicalization internals — M2 core language

**Status: the contract for milestone M2.** This document fixes the shared
canonical AST, name and module identity, import and visibility rules,
canonicalization errors, row representation, persistent interface boundary,
and file ownership. `SPEC.md` and the user-facing documents remain the
language authority; this document defines how those semantics cross compiler
crate boundaries.

The contract deliberately does not preserve the Elm AST where Alder syntax or
semantics differ. Elm remains a guide for SCC construction, name suggestions,
constraint generation, unification, and error presentation.

## 1. Decisions closed by this contract

- A module is identified by a package plus a slice of path segments. The
  package root is an empty slice. A basename is never a module key.
- User module aliases are lowercase namespaces. Modules are not first-class
  record values in M2. Prelude modules such as `Array`, `Http`, and `Fiber`
  occupy a separate capitalized module namespace.
- A logical non-root module path resolves to either `path.ald` or
  `path/mod.ald`. If both exist, resolution fails as ambiguous. The root is
  `src/mod.ald`.
- Canonical locals have compiler-assigned IDs. Their source spelling remains
  for diagnostics and code generation. An assignment root is already resolved
  and carries its mutability.
- Canonical functions are n-ary, including zero-argument functions. Tuples
  have arbitrary arity. Neither is encoded using Elm's binary/three-element
  representation.
- Record and error rows are distinct variants at every compiler layer.
  Optional record fields retain presence metadata and are not rewritten as
  `Option[T]`.
- `provide Path = value { body }` is an expression. The source parser must move
  it from `Stmt` to `Expr` before expression canonicalization lands.
- `for` and `while` have type `()`. A `loop` has a fresh result unified with
  each `break value`; a bare `break` contributes `()`.
- Pattern pins are legal in `match` patterns. Expression pins are legal only
  in queries. Their outside-context errors are separate.
- Table and schema internals are name-resolved in M2, but do not generate type
  constraints. This reconciles structural acceptance with the promise that
  canonicalization resolves names inside deferred constructs.
- Interfaces used during a build are arena-borrowed. Cached interfaces use a
  separate owned, versioned DTO; serde is never derived over bump references.
- Enum runtime representation is `{ $: "Some", _0: x }` for tuple variants,
  `{ $: "Rect", width, height }` for record variants, and a shared frozen
  singleton for a unit variant. M2b code generation treats this as ABI.
- Expression-position blocks are statement-lifted into temporaries. Match
  compilation uses an Elm-style decision tree. Neither choice changes the
  canonical AST.

## 2. Allocation and node conventions

There is one `Bump` arena per source module. Source text is copied into it
first, and all canonical strings are slices of that text or of strings copied
from dependency interfaces into an interface arena. Recursive and large nodes
are referenced; small `Copy` values and `Region` are inline.

```rust
use alder_region::{Located, Region};

pub type Node<'a, T> = &'a Located<T>;
pub type Name<'a> = Located<&'a str>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackageName<'a> {
    pub author: &'a str,
    pub project: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PackageId<'a> {
    Named(PackageName<'a>),
    Application,
    Builtin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleId<'a> {
    pub package: PackageId<'a>,
    pub path: &'a [&'a str],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QualifiedName<'a> {
    pub module: ModuleId<'a>,
    pub name: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConstructorName<'a> {
    pub enum_: QualifiedName<'a>,
    pub variant: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LocalName<'a> {
    pub id: LocalId,
    pub text: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BindingName<'a> {
    Local(LocalName<'a>),
    TopLevel(QualifiedName<'a>),
}
```

`PackageId::Application` is safe because an application cannot be a package
dependency. Every workspace library has a `Named` identity. `Builtin` is
reserved for compiler-known primitive and prelude definitions.

## 3. Modules, imports, and items

```rust
#[derive(Debug)]
pub struct Module<'a> {
    pub id: ModuleId<'a>,
    pub imports: &'a [ResolvedImport<'a>],
    pub items: &'a [Node<'a, Item<'a>>],
    pub value_sccs: &'a [ValueScc<'a>],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    Private,
    Public(Region),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportMode {
    Runtime,
    Test,
}

#[derive(Clone, Copy, Debug)]
pub struct ResolvedImport<'a> {
    pub module: ModuleId<'a>,
    pub region: Region,
    pub visibility: Visibility,
    pub kind: ResolvedImportKind<'a>,
}

#[derive(Clone, Copy, Debug)]
pub enum ResolvedImportKind<'a> {
    Module { binding: Name<'a> },
    Names(&'a [ResolvedImportName<'a>]),
    All,
}

#[derive(Clone, Copy, Debug)]
pub struct ResolvedImportName<'a> {
    pub source: Name<'a>,
    pub binding: Name<'a>,
}

#[derive(Clone, Copy, Debug)]
pub struct ValueScc<'a> {
    pub recursive: bool,
    pub members: &'a [QualifiedName<'a>],
}

#[derive(Debug)]
pub struct Item<'a> {
    pub visibility: Visibility,
    pub attributes: &'a [Attribute<'a>],
    pub kind: ItemKind<'a>,
}

#[derive(Clone, Copy, Debug)]
pub enum Attribute<'a> {
    Extern { module: Option<&'a str>, symbol: Option<&'a str> },
    Derive { region: Region, names: &'a [QualifiedName<'a>] },
    Other { name: Name<'a> },
}

#[derive(Debug)]
pub enum ItemKind<'a> {
    Fn(&'a FnDecl<'a>),
    Let(&'a TopLevelLet<'a>),
    TypeAlias(&'a TypeAlias<'a>),
    Enum(&'a EnumDecl<'a>),
    Trait(&'a TraitDecl<'a>),
    Impl(&'a ImplDecl<'a>),
    ErrorGroup(&'a ErrorGroup<'a>),
    Component(&'a ComponentDecl<'a>),
    Table(&'a TableDecl<'a>),
    Schema(&'a SchemaDecl<'a>),
    Test(&'a TestDecl<'a>),
    Tests(&'a [Node<'a, Item<'a>>]),
    Macro(&'a MacroDecl<'a>),
    Comptime(Node<'a, Block<'a>>),
    Extern(&'a ExternDecl<'a>),
}

#[derive(Debug)]
pub struct FnDecl<'a> {
    pub name: QualifiedName<'a>,
    pub params: &'a [Param<'a>],
    pub ret: Option<Node<'a, Type<'a>>>,
    pub constraints: &'a [TypeConstraint<'a>],
    pub body: Node<'a, Block<'a>>,
}

#[derive(Debug)]
pub struct TopLevelLet<'a> {
    pub bindings: &'a [QualifiedName<'a>],
    pub mutable: bool,
    pub pattern: Node<'a, Pattern<'a>>,
    pub annotation: Option<Node<'a, Type<'a>>>,
    pub value: Node<'a, Expr<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct Param<'a> {
    pub mutable: bool,
    pub pattern: Node<'a, Pattern<'a>>,
    pub annotation: Option<Node<'a, Type<'a>>>,
}

#[derive(Debug)]
pub enum ExternDecl<'a> {
    Fn {
        module: &'a str,
        symbol: &'a str,
        name: QualifiedName<'a>,
        params: &'a [Param<'a>],
        ret: Node<'a, Type<'a>>,
        constraints: &'a [TypeConstraint<'a>],
    },
    Type { name: QualifiedName<'a> },
}
```

Top-level pattern lets export every bound name and list those names in the SCC.
`pub let` therefore makes every binding public. Functions and top-level lets
are mutually recursive and generalized by SCC. Block lets are sequential,
cannot see themselves, and are never generalized.

`#[extern("module", "symbol")]` is valid only on a bodiless function with a
complete return annotation. `#[extern] type Name` is the only valid bodiless
type form. Other attribute/body combinations are canonicalization errors.

The remaining item payloads are:

```rust
#[derive(Debug)]
pub struct TypeAlias<'a> {
    pub name: QualifiedName<'a>,
    pub params: &'a [Name<'a>],
    pub typ: Node<'a, Type<'a>>,
}

#[derive(Debug)]
pub struct EnumDecl<'a> {
    pub name: QualifiedName<'a>,
    pub params: &'a [Name<'a>],
    pub variants: &'a [Variant<'a>],
}

#[derive(Clone, Copy, Debug)]
pub struct Variant<'a> {
    pub name: ConstructorName<'a>,
    pub index: u16,
    pub alternatives: u16,
    pub payload: VariantPayload<'a>,
}

#[derive(Clone, Copy, Debug)]
pub enum VariantPayload<'a> {
    Unit,
    Tuple(&'a [Node<'a, Type<'a>>]),
    Record(&'a [RecordTypeField<'a>]),
}

#[derive(Debug)]
pub struct TraitDecl<'a> {
    pub name: QualifiedName<'a>,
    pub params: &'a [Name<'a>],
    pub constraints: &'a [TypeConstraint<'a>],
    pub items: &'a [TraitItem<'a>],
}

#[derive(Clone, Copy, Debug)]
pub enum TraitItem<'a> {
    AssocType(Name<'a>),
    Fn(&'a TraitFn<'a>),
}

#[derive(Debug)]
pub struct TraitFn<'a> {
    pub name: Name<'a>,
    pub params: &'a [Param<'a>],
    pub ret: Option<Node<'a, Type<'a>>>,
    pub constraints: &'a [TypeConstraint<'a>],
    pub body: Option<Node<'a, Block<'a>>>,
}

#[derive(Debug)]
pub struct ImplDecl<'a> {
    pub trait_: QualifiedName<'a>,
    pub args: &'a [Node<'a, Type<'a>>],
    pub constraints: &'a [TypeConstraint<'a>],
    pub items: &'a [ImplItem<'a>],
}

#[derive(Clone, Copy, Debug)]
pub enum ImplItem<'a> {
    AssocType { name: Name<'a>, typ: Node<'a, Type<'a>> },
    Fn(&'a ImplFn<'a>),
}

#[derive(Debug)]
pub struct ImplFn<'a> {
    pub name: Name<'a>,
    pub params: &'a [Param<'a>],
    pub ret: Option<Node<'a, Type<'a>>>,
    pub constraints: &'a [TypeConstraint<'a>],
    pub body: Node<'a, Block<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub enum TypeConstraint<'a> {
    Bound { var: Name<'a>, traits: &'a [QualifiedName<'a>] },
    AssocEq { var: Name<'a>, assoc: Name<'a>, typ: Node<'a, Type<'a>> },
}

#[derive(Debug)]
pub struct ErrorGroup<'a> {
    pub name: QualifiedName<'a>,
    pub tags: &'a [ErrorTagType<'a>],
}

#[derive(Debug)]
pub struct ComponentDecl<'a> {
    pub name: QualifiedName<'a>,
    pub params: &'a [Param<'a>],
    pub body: Node<'a, Block<'a>>,
}

#[derive(Debug)]
pub struct TableDecl<'a> {
    pub name: QualifiedName<'a>,
    pub columns: &'a [TableColumn<'a>],
}

#[derive(Clone, Copy, Debug)]
pub struct TableColumn<'a> {
    pub name: Name<'a>,
    pub builder: Node<'a, Expr<'a>>,
    pub modifiers: &'a [Modifier<'a>],
}

#[derive(Clone, Copy, Debug)]
pub struct Modifier<'a> {
    pub name: Name<'a>,
    pub args: &'a [Node<'a, Expr<'a>>],
}

#[derive(Debug)]
pub struct SchemaDecl<'a> {
    pub name: QualifiedName<'a>,
    pub from: Option<QualifiedName<'a>>,
    pub items: &'a [SchemaItem<'a>],
}

#[derive(Clone, Copy, Debug)]
pub enum SchemaItem<'a> {
    Pick(&'a [Name<'a>]),
    Field { name: Name<'a>, typ: Option<Node<'a, Type<'a>>>, rules: &'a [Modifier<'a>] },
}

#[derive(Debug)]
pub struct TestDecl<'a> { pub name: Located<&'a str>, pub body: Node<'a, Block<'a>> }

#[derive(Debug)]
pub struct MacroDecl<'a> {
    pub name: QualifiedName<'a>,
    pub params: &'a [Name<'a>],
    pub body: Located<&'a str>,
}
```

Components are value-like functions whose M2 result is `Html`. Tables and
schemas introduce opaque type identities and value handles. Impls, tests,
`tests`, and comptime blocks are never exported. Macro declarations are kept,
but calls and comptime execution report that macros are unavailable in M2.

## 4. Blocks, statements, expressions, and places

```rust
#[derive(Debug)]
pub struct Block<'a> {
    pub statements: &'a [Node<'a, Stmt<'a>>],
    pub tail: Option<Node<'a, Expr<'a>>>,
}

#[derive(Debug)]
pub struct LocalLet<'a> {
    pub mutable: bool,
    pub pattern: Node<'a, Pattern<'a>>,
    pub annotation: Option<Node<'a, Type<'a>>>,
    pub value: Node<'a, Expr<'a>>,
}

#[derive(Debug)]
pub enum Stmt<'a> {
    Let(&'a LocalLet<'a>),
    Use { provider: QualifiedName<'a> },
    Assign { place: &'a Place<'a>, op: Located<alder_source::AssignOp>, value: Node<'a, Expr<'a>> },
    For { pattern: Node<'a, Pattern<'a>>, iter: Node<'a, Expr<'a>>, body: Node<'a, Block<'a>> },
    While { condition: Node<'a, Expr<'a>>, body: Node<'a, Block<'a>> },
    Return(Option<Node<'a, Expr<'a>>>),
    Break(Option<Node<'a, Expr<'a>>>),
    Continue,
    Assert(Node<'a, Expr<'a>>),
    Expr(Node<'a, Expr<'a>>),
}

#[derive(Debug)]
pub struct Place<'a> {
    pub root: BindingName<'a>,
    pub root_region: Region,
    pub mutable: bool,
    pub steps: &'a [PlaceStep<'a>],
}

#[derive(Clone, Copy, Debug)]
pub enum PlaceStep<'a> {
    Field(Name<'a>),
    TupleIndex(Located<u32>),
    Index(Node<'a, Expr<'a>>),
}

#[derive(Clone, Copy, Debug)]
pub enum ValueRef<'a> {
    Local(LocalName<'a>),
    TopLevel(QualifiedName<'a>),
    Foreign { reference: QualifiedName<'a>, annotation: &'a Annotation<'a> },
    Builtin(QualifiedName<'a>), // embedded stdlib member; opaque until its interface is loaded
    Module(ModuleId<'a>),
    Provider(QualifiedName<'a>),
    QueryName(&'a str),
}

#[derive(Clone, Copy, Debug)]
pub struct ConstructorRef<'a> {
    pub name: ConstructorName<'a>,
    pub index: u16,
    pub alternatives: u16,
    pub payload: VariantPayload<'a>,
    pub annotation: &'a Annotation<'a>,
}

#[derive(Debug)]
pub enum Expr<'a> {
    Number { value: f64, text: &'a str },
    BigInt(&'a str),
    Str(&'a str),
    Bool(bool),
    Template(&'a [TemplatePart<'a>]),
    TaggedTemplate { tag: Node<'a, Expr<'a>>, parts: &'a [TemplatePart<'a>] },
    Unit,
    Var(ValueRef<'a>),
    Constructor(ConstructorRef<'a>),
    Tag { group: Option<QualifiedName<'a>>, name: Name<'a>, args: &'a [Node<'a, Expr<'a>>] },
    Array(&'a [Node<'a, Expr<'a>>]),
    Tuple(&'a [Node<'a, Expr<'a>>]),
    Record(&'a [RecordField<'a>]),
    RecordConstructor { constructor: ConstructorRef<'a>, fields: &'a [RecordField<'a>] },
    Call { function: Node<'a, Expr<'a>>, arguments: &'a [Node<'a, Expr<'a>>] },
    Access { record: Node<'a, Expr<'a>>, field: Name<'a> },
    TupleAccess { tuple: Node<'a, Expr<'a>>, index: Located<u32> },
    Index { target: Node<'a, Expr<'a>>, index: Node<'a, Expr<'a>> },
    Await(Node<'a, Expr<'a>>),
    Try(Node<'a, Expr<'a>>),
    Pin(Node<'a, Expr<'a>>),
    Negate(Node<'a, Expr<'a>>),
    Not(Node<'a, Expr<'a>>),
    Binop { op: Located<alder_source::BinOp>, left: Node<'a, Expr<'a>>, right: Node<'a, Expr<'a>> },
    Block(Node<'a, Block<'a>>),
    Lambda { params: &'a [Param<'a>], ret: Option<Node<'a, Type<'a>>>, body: Node<'a, Expr<'a>> },
    If { branches: &'a [IfBranch<'a>], final_else: Option<Node<'a, Block<'a>>> },
    Match { scrutinee: Node<'a, Expr<'a>>, arms: &'a [MatchArm<'a>] },
    Loop(Node<'a, Block<'a>>),
    Provide { provider: QualifiedName<'a>, value: Node<'a, Expr<'a>>, body: Node<'a, Block<'a>> },
    State(Node<'a, Expr<'a>>),
    Style(&'a Style<'a>),
    Query(&'a Query<'a>),
    Markup(&'a Markup<'a>),
    MacroCall { name: Name<'a>, tokens: Located<&'a str> },
}

#[derive(Clone, Copy, Debug)]
pub enum TemplatePart<'a> { Text(&'a str), Expr(Node<'a, Expr<'a>>) }

#[derive(Clone, Copy, Debug)]
pub enum RecordField<'a> {
    Field { name: Name<'a>, value: Node<'a, Expr<'a>> },
    Spread(Node<'a, Expr<'a>>),
}

#[derive(Clone, Copy, Debug)]
pub struct IfBranch<'a> { pub condition: Node<'a, Expr<'a>>, pub body: Node<'a, Block<'a>> }

#[derive(Clone, Copy, Debug)]
pub struct MatchArm<'a> {
    pub patterns: &'a [Node<'a, Pattern<'a>>],
    pub guard: Option<Node<'a, Expr<'a>>>,
    pub body: Node<'a, Expr<'a>>,
}
```

Record shorthand is expanded during canonicalization, so every canonical
field has a value. Fixed binary precedence is resolved into nested `Binop`
nodes using `alder_source::BinOp::precedence()`; no operator environment is
carried forward. Pipe remains a binop until constraint/codegen lowering.

`Placeholder` is intentionally absent. A call containing `_` is wrapped in a
lambda, placeholders are replaced left-to-right with fresh `LocalId`s, and
nested calls are canonicalized before their enclosing call. This means pipe
lowering can distinguish a direct RHS call (default first-argument forwarding)
from a placeholder-selected call, whose RHS is already a unary lambda.

Every function/lambda boundary resets the loop stack. Consequently a closure
cannot `break` or `continue` an enclosing loop. A `provide` value resolves in
the outer environment; its body uses a child provider frame. `use Db` makes
the provider path `Db` available under that same uppercase spelling; it does
not synthesize a lowercase binding. `Db.member` is resolved against provider
and module namespaces before ordinary record access and ambiguity is reported.

## 5. Patterns

```rust
#[derive(Debug)]
pub enum Pattern<'a> {
    Anything,
    Bind(BindingName<'a>),
    Pin(Node<'a, Expr<'a>>),
    Number { value: f64, text: &'a str },
    BigInt(&'a str),
    Str(&'a str),
    Bool(bool),
    Unit,
    Constructor { constructor: ConstructorRef<'a>, args: &'a [Node<'a, Pattern<'a>>] },
    ConstructorRecord { constructor: ConstructorRef<'a>, fields: &'a [PatternField<'a>], rest: bool },
    Tag { group: Option<QualifiedName<'a>>, name: Name<'a>, args: &'a [Node<'a, Pattern<'a>>] },
    Tuple(&'a [Node<'a, Pattern<'a>>]),
    Array { elements: &'a [Node<'a, Pattern<'a>>], rest: Option<ArrayRest<'a>> },
    Record { fields: &'a [PatternField<'a>], rest: bool },
    Alias { pattern: Node<'a, Pattern<'a>>, name: BindingName<'a> },
}

#[derive(Clone, Copy, Debug)]
pub struct PatternField<'a> { pub name: Name<'a>, pub pattern: Node<'a, Pattern<'a>> }

#[derive(Clone, Copy, Debug)]
pub struct ArrayRest<'a> { pub region: Region, pub name: Option<BindingName<'a>> }
```

Record shorthand patterns are expanded to `Bind`. Pins resolve their operand
against the outer pre-pattern environment and never introduce bindings.

Constructors are qualified outside match arms, except prelude
`Some`/`None`/`Ok`/`Err`. Inside a match arm, an unqualified constructor is
looked up across visible enums; zero candidates is unknown and multiple
candidates is ambiguous. Expected scrutinee type may later improve the error,
but canonicalization never silently picks one.

## 6. Types and rows

```rust
#[derive(Debug)]
pub struct Annotation<'a> {
    pub free_vars: &'a [&'a str],
    pub typ: Node<'a, Type<'a>>,
}

#[derive(Debug)]
pub enum Type<'a> {
    Var { name: &'a str, args: &'a [Node<'a, Type<'a>>] },
    Named { reference: QualifiedName<'a>, args: &'a [Node<'a, Type<'a>>] },
    Fn { params: &'a [Node<'a, Type<'a>>], ret: Node<'a, Type<'a>> },
    Unit,
    Tuple(&'a [Node<'a, Type<'a>>]),
    Record { fields: &'a [RecordTypeField<'a>], ext: RowExtension<'a> },
    ErrorRow { tags: &'a [ErrorTagType<'a>], ext: RowExtension<'a> },
    Alias { reference: QualifiedName<'a>, arguments: &'a [AliasArgument<'a>], target: AliasType<'a> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowExtension<'a> { Closed, Open(&'a str) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldPresence { Required, Optional }

#[derive(Clone, Copy, Debug)]
pub struct RecordTypeField<'a> {
    pub index: u16,
    pub name: &'a str,
    pub presence: FieldPresence,
    pub typ: Node<'a, Type<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct ErrorTagType<'a> {
    pub index: u16,
    pub name: &'a str,
    pub args: &'a [Node<'a, Type<'a>>],
}

#[derive(Clone, Copy, Debug)]
pub struct AliasArgument<'a> { pub name: &'a str, pub typ: Node<'a, Type<'a>> }

#[derive(Clone, Copy, Debug)]
pub enum AliasType<'a> { Open(Node<'a, Type<'a>>), Filled(Node<'a, Type<'a>>) }
```

Entries are name-sorted for deterministic solver and interface behavior; the
`index` preserves source/runtime order. Error tag arity is preserved rather
than collapsed to a tuple.

Inference mirrors the distinction:

```rust
pub enum FlatType<'a> {
    App(ModuleId<'a>, &'a str, Vec<Variable>),
    Fn(Vec<Variable>, Variable),
    Tuple(Vec<Variable>),
    EmptyRecord,
    Record(BTreeMap<&'a str, InferField>, Variable),
    EmptyErrorRow,
    ErrorRow(BTreeMap<&'a str, Vec<Variable>>, Variable),
}

pub struct InferField { pub presence: FieldPresence, pub typ: Variable }
```

Ordinary type equality requires equal field presence. Compatibility with an
expected record is asymmetric and uses a dedicated `RecordSubsumes`
constraint:

- an expected required field requires an actual required field;
- an expected optional field accepts a required field, an optional field, or
  absence;
- payload types of fields present on both sides unify;
- reading a required field yields `T`; reading an optional field yields
  `Option[T]`;
- a field explicitly written or updated becomes required, even if it was
  optional in a spread base.

This prevents an optional actual from satisfying a required consumer while
allowing a concrete record to satisfy an optional-props signature.

The M2 primitive universe is `Number`, `BigInt`, `String`, `Bool`, `Array`,
`Map`, `Set`, `Task`, `Option`, `Result`, unit, tuples, records, and error rows.
Elm `Int`/`Float`/`Char`/`List` and `number`/`comparable`/`appendable`
supertypes are removed. Arithmetic is `Number`-only until traits land.

## 7. Deferred structural nodes

Style, query, and markup types mirror `alder_source` one-for-one, replacing
source `Expr`, `Pattern`, `Block`, and resolved paths with the canonical types
above. This is a contract requirement: opaque outer typing must not discard
children that still need name resolution, constraints, formatting, or later
lowering.

```rust
#[derive(Debug)]
pub struct Style<'a> { pub entries: &'a [StyleEntry<'a>] }

#[derive(Clone, Copy, Debug)]
pub struct StyleEntry<'a> { pub key: Located<StyleKey<'a>>, pub value: StyleValue<'a> }

#[derive(Clone, Copy, Debug)]
pub enum StyleKey<'a> { Ident(&'a str), Str(&'a str) }

#[derive(Clone, Copy, Debug)]
pub enum StyleValue<'a> {
    Dimension { value: f64, text: &'a str, unit: &'a str },
    Expr(Node<'a, Expr<'a>>),
    Nested(&'a Style<'a>),
}

#[derive(Debug)]
pub enum Query<'a> {
    Select(&'a Select<'a>),
    Insert { table: QualifiedName<'a>, values: Node<'a, Expr<'a>> },
    Update { table: QualifiedName<'a>, set: &'a [RecordField<'a>], where_: Option<Node<'a, Expr<'a>>> },
    Delete { table: QualifiedName<'a>, where_: Option<Node<'a, Expr<'a>>> },
}

#[derive(Debug)]
pub struct Select<'a> {
    pub projection: Projection<'a>,
    pub from: TableRef<'a>,
    pub joins: &'a [Join<'a>],
    pub where_: Option<Node<'a, Expr<'a>>>,
    pub group_by: &'a [Node<'a, Expr<'a>>],
    pub order_by: &'a [Order<'a>],
    pub limit: Option<Node<'a, Expr<'a>>>,
    pub offset: Option<Node<'a, Expr<'a>>>,
}

#[derive(Clone, Copy, Debug)]
pub enum Projection<'a> { Star(Region), Fields(&'a [Node<'a, Expr<'a>>]) }

#[derive(Clone, Copy, Debug)]
pub struct TableRef<'a> { pub table: QualifiedName<'a>, pub alias: Option<Name<'a>> }

#[derive(Clone, Copy, Debug)]
pub struct Join<'a> {
    pub kind: Located<alder_source::JoinKind>,
    pub table: TableRef<'a>,
    pub on: Node<'a, Expr<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct Order<'a> {
    pub expr: Node<'a, Expr<'a>>,
    pub direction: Option<Located<alder_source::OrderDir>>,
}

#[derive(Debug)]
pub enum Markup<'a> { Element(&'a Element<'a>), Fragment(&'a [Node<'a, Child<'a>>]) }

#[derive(Debug)]
pub struct Element<'a> {
    pub name: Located<ElementName<'a>>,
    pub attrs: &'a [Attr<'a>],
    pub children: &'a [Node<'a, Child<'a>>],
    pub self_closing: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum ElementName<'a> { Tag(&'a str), Component(QualifiedName<'a>) }

#[derive(Clone, Copy, Debug)]
pub struct Attr<'a> { pub name: Name<'a>, pub value: Option<AttrValue<'a>> }

#[derive(Clone, Copy, Debug)]
pub enum AttrValue<'a> { Str(Located<&'a str>), Expr(Node<'a, Expr<'a>>) }

#[derive(Debug)]
pub enum Child<'a> {
    Element(&'a Element<'a>),
    Fragment(&'a [Node<'a, Child<'a>>]),
    Text(&'a str),
    Hole(Node<'a, Expr<'a>>),
    If { branches: &'a [ChildIfBranch<'a>], final_else: Option<Node<'a, ChildBlock<'a>>> },
    For {
        pattern: Node<'a, Pattern<'a>>,
        iter: Node<'a, Expr<'a>>,
        key: Option<Node<'a, Expr<'a>>>,
        body: Node<'a, ChildBlock<'a>>,
        empty: Option<Node<'a, ChildBlock<'a>>>,
    },
    Match { scrutinee: Node<'a, Expr<'a>>, arms: &'a [ChildMatchArm<'a>] },
}

#[derive(Clone, Copy, Debug)]
pub struct ChildIfBranch<'a> {
    pub condition: Node<'a, Expr<'a>>,
    pub body: Node<'a, ChildBlock<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct ChildMatchArm<'a> {
    pub patterns: &'a [Node<'a, Pattern<'a>>],
    pub guard: Option<Node<'a, Expr<'a>>>,
    pub body: Node<'a, ChildBlock<'a>>,
}

#[derive(Debug)]
pub struct ChildBlock<'a> { pub items: &'a [ChildItem<'a>] }

#[derive(Clone, Copy, Debug)]
pub enum ChildItem<'a> { Stmt(Node<'a, Stmt<'a>>), Child(Node<'a, Child<'a>>) }
```

- `Style` keeps keys, dimensions, nested styles, and canonical value
  expressions. Its type is opaque `Style`.
- `Query` keeps operation, projection, joins, predicates, grouping, ordering,
  limits, offsets, and canonical pin operands. Host expressions are checked;
  the result is `Query[r]` with fresh `r`.
- `Markup` keeps elements/fragments, attributes, holes, directives, child
  blocks, child lets, guards, keys, and canonical children. Holes and directive
  host expressions are checked; the outer expression is `Html`.
- `state(x)` retains a `State` node and has the type of `x` in M2.
- Trait/impl/bound names and method bodies are canonicalized, while trait
  constraints are ignored by inference.
- Error tags produce fresh error variables in M2. Full row merging is M4.
- `.await` is retained and types as `Task[a] -> a`; its containing function
  must explicitly return `Task[...]` during M2.
- `use`/`provide` retain opaque provider nodes and do not check provider
  consistency until M4.
- Macro calls, comptime blocks, and derives produce explicit unavailable
  errors. Macro declarations remain in the module.

## 8. Environment and scope invariants

`alder-can` uses separate namespaces rather than a single Elm-shaped table.

```rust
pub struct Env<'a> {
    pub home: ModuleId<'a>,
    pub scopes: Vec<Scope<'a>>,
    pub types: NameTable<'a, TypeBinding<'a>>,
    pub enums: NameTable<'a, EnumBinding<'a>>,
    pub traits: NameTable<'a, TraitBinding<'a>>,
    pub modules: NameTable<'a, ModuleBinding<'a>>,
    pub providers: Vec<ProviderScope<'a>>,
    pub context: Context<'a>,
    pub prelude: Prelude<'a>,
}

pub struct Scope<'a> { pub values: BTreeMap<&'a str, ValueBinding<'a>> }

pub struct ValueBinding<'a> {
    pub reference: ValueRef<'a>,
    pub region: Region,
    pub mutable: bool,
    pub origin: Origin,
    pub annotation: Option<&'a Annotation<'a>>,
}

pub struct Context<'a> {
    pub function: Option<FunctionContext<'a>>,
    pub loops: Vec<LoopContext>,
    pub match_depth: u16,
    pub query_depth: u16,
}
```

`NameTable` retains unique, ambiguous, and private candidates so diagnostics
can distinguish unknown, ambiguous, and non-public access. Canonicalization:

1. resolves imports and builds namespace tables;
2. predeclares every top-level value, type, enum, trait, component, table,
   schema, error group, and macro, reporting duplicates;
3. canonicalizes signatures and bodies;
4. computes top-level value SCCs;
5. emits an unsolved module and the inventory needed by interface creation.

Block lets add their bindings only after the initializer. Branches, arms, and
loop bodies use child scopes. Match guards and bodies share their arm's pattern
bindings; sibling arms do not. Lexical locals may shadow imports, while a
duplicate in the same lexical scope is an error. Top-level declarations and
explicit named imports may not collide. Top-level declarations take
precedence over star imports; colliding star imports become ambiguity on use.
Uppercase type, enum, trait, component-type, table, schema, error-group, and
opaque declarations may not reuse a spelling in one module even where a later
phase could disambiguate by context. Enum variants occupy only their owning
enum's subnamespace. Imports nested in `tests {}` are scoped to that group and
do not leak or participate in the ordinary module SCC; test bodies are
canonicalized only in test mode.

`let mut pattern` and mutable parameters mark every binding introduced by the
pattern mutable. Field/index assignment permission comes from the resolved
root. Imports, prelude entries, module bindings, and immutable locals cannot
be assigned.

## 9. Import and visibility contract

The driver resolves source module paths before canonicalization:

```rust
pub fn resolve_imports<'a>(
    project: &Project,
    importer: ModuleId<'a>,
    imports: impl Iterator<Item = &'a alder_source::Import<'a>>,
    mode: ImportMode,
) -> Result<Vec<ResolvedModuleImport>, Vec<DriverError>>;
```

- `~` selects the importing workspace member's `src/` root.
- `@author/package` selects self or a declared dependency. Test dependencies
  are visible only in test mode.
- Resolution rejects missing modules, undeclared packages, target-incompatible
  dependencies, duplicate package identities, paths escaping the source root,
  and ambiguous `path.ald`/`path/mod.ald` pairs.
- Graph nodes and interface maps are keyed by structured module identity.
  Import errors retain the source import region. Unresolved edges are never
  silently dropped.
- Fetching sources in parallel must restore graph order before compilation.

A bare import or `as` binds a module namespace. `.{ names }` copies each public
binding of that spelling in the applicable namespace; aliases rename the local
binding but preserve canonical origin. `.*` copies every public binding except
ordinary enum variants. Constructors remain namespaced. `pub import` adds
selected public entries to the current module interface with their origin
unchanged.

Private interface inventories are available within the current build so
`PrivateAccess` can be reported. Published/cache public views need not leak
private type bodies. The cached interface retains a compact inventory of
private spelling plus namespace so cross-package access can be diagnosed as
private rather than unknown.

## 10. Module interfaces and persistence

The in-memory solved interface is arena-borrowed:

```rust
#[derive(Clone, Copy, Debug)]
pub struct Interface<'a> {
    pub home: ModuleId<'a>,
    pub values: &'a [InterfaceValue<'a>],
    pub types: &'a [InterfaceType<'a>],
    pub enums: &'a [InterfaceEnum<'a>],
    pub traits: &'a [InterfaceTrait<'a>],
    pub modules: &'a [InterfaceModule<'a>],
    pub private_names: &'a [PrivateName<'a>],
}

#[derive(Clone, Copy, Debug)]
pub struct InterfaceValue<'a> {
    pub exported_as: &'a str,
    pub reference: QualifiedName<'a>,
    pub annotation: &'a Annotation<'a>,
    pub kind: ValueKind,
}

#[derive(Clone, Copy, Debug)]
pub struct InterfaceType<'a> {
    pub exported_as: &'a str,
    pub reference: QualifiedName<'a>,
    pub params: &'a [&'a str],
    pub body: PublicTypeBody<'a>,
}

#[derive(Clone, Copy, Debug)]
pub struct InterfaceEnum<'a> {
    pub exported_as: &'a str,
    pub reference: QualifiedName<'a>,
    pub params: &'a [&'a str],
    pub variants: &'a [Variant<'a>],
}

#[derive(Clone, Copy, Debug)]
pub struct InterfaceTrait<'a> {
    pub exported_as: &'a str,
    pub reference: QualifiedName<'a>,
    pub params: &'a [&'a str],
    pub assoc_types: &'a [&'a str],
    pub methods: &'a [InterfaceValue<'a>],
}

#[derive(Clone, Copy, Debug)]
pub struct InterfaceModule<'a> { pub exported_as: &'a str, pub module: ModuleId<'a> }
```

`PublicTypeBody` distinguishes transparent aliases, opaque extern/table/schema
types, error groups, and any later nominal kind. Components appear as values;
tables and schemas appear as opaque types plus value handles. Impls, tests,
test groups, and comptime blocks never appear.

`alder-driver` owns `InterfaceFile`, an owned deterministic DTO using `String`
and `Vec`. It includes a schema version, compiler version, structured module
ID, normalized solved type schemes (including row kind and field presence),
aliases, complete enum payloads, traits, error groups, opaque types, module
re-exports, and origin identity. Encoding and hydration are explicit deep
conversions:

```rust
impl InterfaceFile {
    pub const SCHEMA_VERSION: u32 = 1;
    pub fn from_solved(interface: &Interface<'_>) -> Self;
    pub fn encode(&self) -> Result<Vec<u8>, InterfaceError>;
    pub fn decode(bytes: &[u8]) -> Result<Self, InterfaceError>;
    pub fn hydrate<'a>(&self, bump: &'a Bump) -> Interface<'a>;
}
```

The fingerprint is a documented stable hash of canonical encoded public bytes,
not `DefaultHasher`. Cache keys include compiler and schema versions, source
content hash, and exact dependency interface fingerprints. Modification times
may optimize checks but are never the sole correctness signal.

## 11. Canonicalization errors

Errors have a common located wrapper and nested ownership hierarchy:

```rust
#[derive(Clone, Debug)]
pub struct Error<'a> { pub region: Region, pub kind: ErrorKind<'a> }

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Namespace { Value, Type, Enum, Constructor, Trait, Module, Provider, AssociatedItem }

#[derive(Clone, Debug)]
pub enum NameError<'a> {
    Unknown { namespace: Namespace, qualifier: Option<&'a str>, name: &'a str, suggestions: &'a [&'a str] },
    Ambiguous { namespace: Namespace, name: &'a str, candidates: &'a [QualifiedName<'a>] },
    Private { owner: ModuleId<'a>, namespace: Namespace, name: &'a str },
}
```

The nested enums must cover at least:

- unknown names with deterministic namespace-specific suggestions, ambiguous
  imports, missing import names, private access, explicit alias collisions,
  private re-exports, and unqualified constructors outside match;
- duplicate definitions in every namespace, duplicate params/pattern
  bindings/fields/variants/tags, constructor arity and field errors, type
  arity, and unbound/unused type variables;
- assignment to immutable/imported bindings, invalid assignment targets,
  `break` or `continue` outside loops, `return` outside functions,
  expression pin outside query, pattern pin outside match, and placeholder
  outside an immediate call argument;
- invalid extern placement/arguments/body/signature, unavailable macro calls
  and comptime, unavailable derives, and M2's explicit-task-return rule for
  `.await`;
- fixed-operator non-associative conflicts and all deferred structural name
  errors.

Suggestions use edit distance plus prefix matching, are stable-sorted and
capped, and never expose private names as ordinary suggestions. Diagnostics
render the new syntax (`Map[String, Number]`, `fn(a) b`,
`{ name?: String }`) and retain all declaration/reference regions needed for
Elm-quality primary and secondary labels.

## 12. Downstream pipeline contract

```text
driver resolve paths/imports
  -> can canonicalize(module arena, ModuleId, resolved imports, source Module)
  -> constrain(module arena, canonical Module, inference context)
  -> solve(module arena, union-find, constraints)
  -> build solved Interface + InterfaceFile
  -> codegen while module arena is alive
```

Constraint generation carries function-return, loop-result, and query
contexts. `return`, `break`, and `continue` are diverging control flow rather
than ordinary unit expressions. Sequential local lets use a dedicated
non-generalizing binding constraint; top-level SCCs retain rank-based
generalization.

The driver owns one source/canonical arena per active module and an interface
arena for hydrated dependencies. It does not retain canonical modules merely
to satisfy interface lifetimes. M2b codegen runs before the module arena is
dropped or consumes a separately lowered owned IR.

## 13. File ownership and implementation order

Wave 0 is serial because it establishes shared types:

- `docs/canonical-internals.md`: this contract.
- `crates/alder-source`, `crates/alder-parse`: promote `provide` to `Expr` and
  add the comment side table only when formatter work begins.
- `crates/alder-ast/src/lib.rs`: canonical types in §§2–7, initially with no
  compatibility aliases for the Elm AST.

Wave 1 owners are disjoint after the AST lands:

- `alder-can/environment.rs` and `environment/*`: namespace tables, scopes,
  imports, visibility, prelude, and local IDs.
- `alder-can/item.rs` and `module.rs`: predeclarations, items, SCCs, interfaces.
- `alder-can/expression.rs` and `statement.rs`: expressions, statements,
  precedence, placeholders, control contexts, and mutation.
- `alder-can/pattern.rs`: patterns, constructors, pins, and bindings.
- `alder-can/types.rs`: canonical types, aliases, rows, and constraints.
- `alder-can/error.rs`: hierarchy and rendering data.

Wave 2 remains disjoint by crate:

- `alder-constrain`: canonical traversal and new constraint forms.
- `alder-solve`: Alder primitives, n-ary functions/tuples, both row kinds,
  optional compatibility, annotations, and type rendering.
- `alder-driver`: path resolver, graph, interface DTO/cache, and ordered build.
- `alder-cli`: `alder check` integration.

M2b creates `alder-codegen`, `alder-kernel`, `alder-fmt`, and `std/`, then adds
`run`, `build`, `fmt`, and `test` to the CLI. Codegen and runtime choices in §1
are ABI and must be tested with emitted-JS snapshots plus runtime assertions.

## 14. Required verification

- Every canonicalization rule and error leaf has granular success/error
  snapshots in the new Alder syntax.
- Solver tests cover every new typing rule, including three-field and nested
  optional-row regressions, asymmetric optional compatibility, zero-argument
  functions, arbitrary tuples, loop breaks, return flow, placeholders, and
  `?`.
- Driver tests cover root and nested `mod.ald`, sibling files, `~/`, workspace
  packages, both-file ambiguity, private access, re-exports, cycles, stable
  cache round trips, and parallel fetch order.
- Every full-module documentation example canonicalizes. Deferred constructs
  are structurally traversed without M2 type errors, except explicitly
  unavailable macro/comptime/derive use.
- M2a ends only when workspace build, clippy with warnings denied, tests, and
  `alder check` end-to-end fixtures are green. M2b additionally gates emitted
  JS/run/build/fmt/test, stdlib compilation, formatter idempotency/comment
  preservation, and runtime e2e projects.
