//! Canonical AST for Alder.
//!
//! This is the name-resolved representation consumed by constraint generation,
//! solving, and code generation. See `docs/canonical-internals.md`.

use alder_region::{Located, Region};

pub use alder_source::{AssignOp, BinOp, JoinKind, OrderDir};

pub type Node<'a, T> = &'a Located<T>;
pub type Name<'a> = Located<&'a str>;
pub type FreeVars<'a> = &'a [&'a str];

// ============================================================================
// Names
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackageName<'a> {
    pub author: &'a str,
    pub project: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PackageId<'a> {
    Named(PackageName<'a>),
    Application,
    ApplicationMember(&'a str),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UseId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TraitId<'a>(pub QualifiedName<'a>);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MethodId<'a> {
    pub trait_: TraitId<'a>,
    pub index: u16,
    pub name: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssocTypeId<'a> {
    pub trait_: TraitId<'a>,
    pub index: u16,
    pub name: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ImplOrigin {
    Source {
        item_ordinal: u32,
    },
    Derived {
        type_ordinal: u32,
        derive_index: u16,
    },
    AutomaticEq {
        type_ordinal: u32,
    },
    Builtin {
        index: u16,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImplId<'a> {
    pub module: ModuleId<'a>,
    pub origin: ImplOrigin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind<'a> {
    Type,
    Arrow {
        param: &'a Kind<'a>,
        result: &'a Kind<'a>,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct TypeParam<'a> {
    pub name: Name<'a>,
    pub kind: Kind<'a>,
}

#[derive(Clone, Copy, Debug)]
pub struct TraitRef<'a> {
    pub trait_: TraitId<'a>,
    pub args: &'a [Node<'a, Type<'a>>],
}

#[derive(Clone, Copy, Debug)]
pub struct ProjectionType<'a> {
    pub trait_ref: TraitRef<'a>,
    pub assoc: AssocTypeId<'a>,
}

#[derive(Clone, Copy, Debug)]
pub struct ProjectionEquality<'a> {
    pub projection: ProjectionType<'a>,
    pub typ: Node<'a, Type<'a>>,
    pub region: Region,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeriveKind {
    Show,
    Eq,
    Ord,
    Hash,
    Json,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DictionaryKind {
    Singleton,
    Factory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalName<'a> {
    pub id: LocalId,
    pub text: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BindingName<'a> {
    Local(LocalName<'a>),
    TopLevel(QualifiedName<'a>),
}

// ============================================================================
// Modules, imports, and items
// ============================================================================

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
    Extern {
        module: Option<&'a str>,
        symbol: Option<&'a str>,
    },
    Derive {
        region: Region,
        names: &'a [QualifiedName<'a>],
    },
    Other {
        name: Name<'a>,
    },
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
    Type {
        name: QualifiedName<'a>,
    },
}

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
    AssocType {
        name: Name<'a>,
        typ: Node<'a, Type<'a>>,
    },
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
    Bound {
        var: Name<'a>,
        traits: &'a [QualifiedName<'a>],
    },
    AssocEq {
        var: Name<'a>,
        assoc: Name<'a>,
        typ: Node<'a, Type<'a>>,
    },
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
    Field {
        name: Name<'a>,
        typ: Option<Node<'a, Type<'a>>>,
        rules: &'a [Modifier<'a>],
    },
}

#[derive(Debug)]
pub struct TestDecl<'a> {
    pub name: Located<&'a str>,
    pub body: Node<'a, Block<'a>>,
}

#[derive(Debug)]
pub struct MacroDecl<'a> {
    pub name: QualifiedName<'a>,
    pub params: &'a [Name<'a>],
    pub body: Located<&'a str>,
}

// ============================================================================
// Blocks, statements, and expressions
// ============================================================================

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
    Use {
        provider: QualifiedName<'a>,
    },
    Assign {
        use_id: Option<UseId>,
        place: &'a Place<'a>,
        op: Located<AssignOp>,
        value: Node<'a, Expr<'a>>,
    },
    For {
        pattern: Node<'a, Pattern<'a>>,
        iter: Node<'a, Expr<'a>>,
        body: Node<'a, Block<'a>>,
    },
    While {
        condition: Node<'a, Expr<'a>>,
        body: Node<'a, Block<'a>>,
    },
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
    Foreign {
        reference: QualifiedName<'a>,
        annotation: &'a Annotation<'a>,
    },
    TraitMethod {
        method: MethodId<'a>,
        annotation: &'a Annotation<'a>,
    },
    /// A value exported by an embedded first-party stdlib module. Its
    /// signature is opaque until stdlib interfaces are loaded by the driver.
    Builtin(QualifiedName<'a>),
    Module(ModuleId<'a>),
    Provider(QualifiedName<'a>),
    QueryName(&'a str),
    /// An unresolved identifier inside a deferred M2-owned DSL body.
    Opaque(&'a str),
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
    Number {
        value: f64,
        text: &'a str,
    },
    BigInt(&'a str),
    Str(&'a str),
    Bool(bool),
    Template(&'a [TemplatePart<'a>]),
    TaggedTemplate {
        tag: Node<'a, Expr<'a>>,
        parts: &'a [TemplatePart<'a>],
    },
    Unit,
    Var {
        use_id: UseId,
        reference: ValueRef<'a>,
    },
    Constructor(ConstructorRef<'a>),
    Tag {
        group: Option<QualifiedName<'a>>,
        name: Name<'a>,
        args: &'a [Node<'a, Expr<'a>>],
    },
    Array(&'a [Node<'a, Expr<'a>>]),
    Tuple(&'a [Node<'a, Expr<'a>>]),
    Record(&'a [RecordField<'a>]),
    RecordConstructor {
        constructor: ConstructorRef<'a>,
        fields: &'a [RecordField<'a>],
    },
    Call {
        use_id: UseId,
        function: Node<'a, Expr<'a>>,
        arguments: &'a [Node<'a, Expr<'a>>],
    },
    Access {
        record: Node<'a, Expr<'a>>,
        field: Name<'a>,
    },
    TupleAccess {
        tuple: Node<'a, Expr<'a>>,
        index: Located<u32>,
    },
    Index {
        target: Node<'a, Expr<'a>>,
        index: Node<'a, Expr<'a>>,
    },
    Await(Node<'a, Expr<'a>>),
    Try(Node<'a, Expr<'a>>),
    Pin(Node<'a, Expr<'a>>),
    Negate {
        use_id: UseId,
        expr: Node<'a, Expr<'a>>,
    },
    Not(Node<'a, Expr<'a>>),
    Binop {
        use_id: UseId,
        op: Located<BinOp>,
        left: Node<'a, Expr<'a>>,
        right: Node<'a, Expr<'a>>,
    },
    Block(Node<'a, Block<'a>>),
    Lambda {
        params: &'a [Param<'a>],
        ret: Option<Node<'a, Type<'a>>>,
        body: Node<'a, Expr<'a>>,
    },
    If {
        branches: &'a [IfBranch<'a>],
        final_else: Option<Node<'a, Block<'a>>>,
    },
    Match {
        scrutinee: Node<'a, Expr<'a>>,
        arms: &'a [MatchArm<'a>],
    },
    Loop(Node<'a, Block<'a>>),
    Provide {
        provider: QualifiedName<'a>,
        value: Node<'a, Expr<'a>>,
        body: Node<'a, Block<'a>>,
    },
    State(Node<'a, Expr<'a>>),
    Style(&'a Style<'a>),
    Query(&'a Query<'a>),
    Markup(&'a Markup<'a>),
    MacroCall {
        name: Name<'a>,
        tokens: Located<&'a str>,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum TemplatePart<'a> {
    Text(&'a str),
    Expr(Node<'a, Expr<'a>>),
}

#[derive(Clone, Copy, Debug)]
pub enum RecordField<'a> {
    Field {
        name: Name<'a>,
        value: Node<'a, Expr<'a>>,
    },
    Spread(Node<'a, Expr<'a>>),
}

#[derive(Clone, Copy, Debug)]
pub struct IfBranch<'a> {
    pub condition: Node<'a, Expr<'a>>,
    pub body: Node<'a, Block<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct MatchArm<'a> {
    pub patterns: &'a [Node<'a, Pattern<'a>>],
    pub guard: Option<Node<'a, Expr<'a>>>,
    pub body: Node<'a, Expr<'a>>,
}

// ============================================================================
// Patterns
// ============================================================================

#[derive(Debug)]
pub enum Pattern<'a> {
    Anything,
    Bind(BindingName<'a>),
    Pin {
        use_id: UseId,
        value: Node<'a, Expr<'a>>,
    },
    Number {
        value: f64,
        text: &'a str,
    },
    BigInt(&'a str),
    Str(&'a str),
    Bool(bool),
    Unit,
    Constructor {
        constructor: ConstructorRef<'a>,
        args: &'a [Node<'a, Pattern<'a>>],
    },
    ConstructorRecord {
        constructor: ConstructorRef<'a>,
        fields: &'a [PatternField<'a>],
        rest: bool,
    },
    Tag {
        group: Option<QualifiedName<'a>>,
        name: Name<'a>,
        args: &'a [Node<'a, Pattern<'a>>],
    },
    Tuple(&'a [Node<'a, Pattern<'a>>]),
    Array {
        elements: &'a [Node<'a, Pattern<'a>>],
        rest: Option<ArrayRest<'a>>,
    },
    Record {
        fields: &'a [PatternField<'a>],
        rest: bool,
    },
    Alias {
        pattern: Node<'a, Pattern<'a>>,
        name: BindingName<'a>,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct PatternField<'a> {
    pub name: Name<'a>,
    pub pattern: Node<'a, Pattern<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct ArrayRest<'a> {
    pub region: Region,
    pub name: Option<BindingName<'a>>,
}

// ============================================================================
// Types
// ============================================================================

#[derive(Debug)]
pub struct Annotation<'a> {
    pub params: &'a [TypeParam<'a>],
    pub trait_predicates: &'a [TraitRef<'a>],
    pub projection_equalities: &'a [ProjectionEquality<'a>],
    pub typ: Node<'a, Type<'a>>,
}

#[derive(Debug)]
pub enum Type<'a> {
    Var {
        name: &'a str,
        args: &'a [Node<'a, Type<'a>>],
    },
    Named {
        reference: QualifiedName<'a>,
        args: &'a [Node<'a, Type<'a>>],
    },
    Partial {
        constructor: QualifiedName<'a>,
        slots: &'a [TypeSlot<'a>],
    },
    Projection(ProjectionType<'a>),
    Fn {
        params: &'a [Node<'a, Type<'a>>],
        ret: Node<'a, Type<'a>>,
    },
    Unit,
    Tuple(&'a [Node<'a, Type<'a>>]),
    Record {
        fields: &'a [RecordTypeField<'a>],
        ext: RowExtension<'a>,
    },
    ErrorRow {
        tags: &'a [ErrorTagType<'a>],
        ext: RowExtension<'a>,
    },
    Alias {
        reference: QualifiedName<'a>,
        arguments: &'a [AliasArgument<'a>],
        target: AliasType<'a>,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum TypeSlot<'a> {
    Hole(u16),
    Fixed(Node<'a, Type<'a>>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowExtension<'a> {
    Closed,
    Open(&'a str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldPresence {
    Required,
    Optional,
}

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
pub struct AliasArgument<'a> {
    pub name: &'a str,
    pub typ: Node<'a, Type<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub enum AliasType<'a> {
    Open(Node<'a, Type<'a>>),
    Filled(Node<'a, Type<'a>>),
}

// ============================================================================
// Style, queries, and markup
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
    Str(&'a str),
}

#[derive(Clone, Copy, Debug)]
pub enum StyleValue<'a> {
    Dimension {
        value: f64,
        text: &'a str,
        unit: &'a str,
    },
    Expr(Node<'a, Expr<'a>>),
    Nested(&'a Style<'a>),
}

#[derive(Debug)]
pub enum Query<'a> {
    Select(&'a Select<'a>),
    Insert {
        table: QualifiedName<'a>,
        values: Node<'a, Expr<'a>>,
    },
    Update {
        table: QualifiedName<'a>,
        set: &'a [RecordField<'a>],
        where_: Option<Node<'a, Expr<'a>>>,
    },
    Delete {
        table: QualifiedName<'a>,
        where_: Option<Node<'a, Expr<'a>>>,
    },
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
pub enum Projection<'a> {
    Star(Region),
    Fields(&'a [Node<'a, Expr<'a>>]),
}

#[derive(Clone, Copy, Debug)]
pub struct TableRef<'a> {
    pub table: QualifiedName<'a>,
    pub alias: Option<Name<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct Join<'a> {
    pub kind: Located<JoinKind>,
    pub table: TableRef<'a>,
    pub on: Node<'a, Expr<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct Order<'a> {
    pub expr: Node<'a, Expr<'a>>,
    pub direction: Option<Located<OrderDir>>,
}

#[derive(Debug)]
pub enum Markup<'a> {
    Element(&'a Element<'a>),
    Fragment(&'a [Node<'a, Child<'a>>]),
}

#[derive(Debug)]
pub struct Element<'a> {
    pub name: Located<ElementName<'a>>,
    pub attrs: &'a [Attr<'a>],
    pub children: &'a [Node<'a, Child<'a>>],
    pub self_closing: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum ElementName<'a> {
    Tag(&'a str),
    Component(QualifiedName<'a>),
}

#[derive(Clone, Copy, Debug)]
pub struct Attr<'a> {
    pub name: Name<'a>,
    pub value: Option<AttrValue<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub enum AttrValue<'a> {
    Str(Located<&'a str>),
    Expr(Node<'a, Expr<'a>>),
}

#[derive(Debug)]
pub enum Child<'a> {
    Element(&'a Element<'a>),
    Fragment(&'a [Node<'a, Child<'a>>]),
    Text(&'a str),
    Hole(Node<'a, Expr<'a>>),
    If {
        branches: &'a [ChildIfBranch<'a>],
        final_else: Option<Node<'a, ChildBlock<'a>>>,
    },
    For {
        pattern: Node<'a, Pattern<'a>>,
        iter: Node<'a, Expr<'a>>,
        key: Option<Node<'a, Expr<'a>>>,
        body: Node<'a, ChildBlock<'a>>,
        empty: Option<Node<'a, ChildBlock<'a>>>,
    },
    Match {
        scrutinee: Node<'a, Expr<'a>>,
        arms: &'a [ChildMatchArm<'a>],
    },
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
pub struct ChildBlock<'a> {
    pub items: &'a [ChildItem<'a>],
}

#[derive(Clone, Copy, Debug)]
pub enum ChildItem<'a> {
    Stmt(Node<'a, Stmt<'a>>),
    Child(Node<'a, Child<'a>>),
}

// ============================================================================
// Solved module interfaces
// ============================================================================

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    Function,
    Let,
    Component,
    Table,
    Schema,
    Extern,
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
pub enum PublicTypeBody<'a> {
    Alias(Node<'a, Type<'a>>),
    Opaque(OpaqueKind),
    ErrorGroup(&'a [ErrorTagType<'a>]),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpaqueKind {
    Extern,
    Table,
    Schema,
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
pub struct InterfaceModule<'a> {
    pub exported_as: &'a str,
    pub module: ModuleId<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Namespace {
    Value,
    Type,
    Enum,
    Constructor,
    Trait,
    Module,
    Provider,
    AssociatedItem,
}

#[derive(Clone, Copy, Debug)]
pub struct PrivateName<'a> {
    pub name: &'a str,
    pub namespace: Namespace,
}
