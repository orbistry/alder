# Trait internals

This document is the implementation contract for M3. It fixes the shared
representations and phase boundaries for Alder traits, higher-kinded type
parameters, associated types, derives, operator dispatch, and JavaScript
dictionary passing. `plans/m3-traits.md`, `SPEC.md`, and `docs/language.md`
define the user-facing requirements; this document defines how the compiler
meets them.

The design was reviewed from the canonicalization/tooling, solver/type-system,
and code-generation/runtime angles. The compact active solver in
`alder-solve/src/inference.rs` is the implementation base. The uncompiled Elm
solver files are reference material only.

## Semantic boundaries

- A trait is a globally named predicate over one or more types. Argument zero
  is the subject for coherence. The existing grammar permits multiple
  parameters and M3 preserves that; the current `where a: Trait` shorthand can
  name only unary traits until bound syntax grows trait arguments. A subject
  may have kind `Type -> Type`, supporting `Functor[f]` and
  `Functor[Result[_, e]]`.
- Trait functions are ordinary namespace-visible functions. There is no
  `self`, receiver syntax, method-call sugar, or explicit type arguments.
- Trait `where` predicates are direct superclasses. An impl `where` clause is
  the set of dictionaries needed to construct that impl.
- Associated types are non-injective projections. They may be normalized from
  an impl or constrained by a declared projection equality; they never infer
  their input backwards from their result.
- Instance coherence is package-wide and independent of source or build order.
- Solving produces an Alder-owned evidence plan. Code generation consumes that
  plan and constructs Oxc AST directly; it does not generate JavaScript source
  fragments for Rolldown to parse.
- Static resolution is an optimization of dictionary semantics, not a second
  dispatch model.

## Stable identities

Source regions are diagnostic locations and are not identities. Synthetic
nodes can share a region, and arena pointers cannot survive interface copying.
M3 adds deterministic module-local IDs to dispatch sites and package-stable IDs
to declarations:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UseId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EvidenceOwner<'a> {
    Binding(QualifiedName<'a>),
    ImplMethod { impl_: ImplId<'a>, method: MethodId<'a> },
    Default(MethodId<'a>),
}

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
    Source { item_ordinal: u32 },
    Derived { type_ordinal: u32, derive_index: u16 },
    AutomaticEq { type_ordinal: u32 },
    Builtin { index: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImplId<'a> {
    pub module: ModuleId<'a>,
    pub origin: ImplOrigin,
}
```

`UseId`s are assigned in canonical source traversal order, including default
bodies. Synthetic bodies allocate after source bodies in `ImplOrigin` order.
They are stored at every site that can require evidence:

```rust
Expr::Var { use_id: UseId, reference: ValueRef<'a> }
Expr::Call { use_id: UseId, callee: Node<'a, Expr<'a>>, args: &'a [Node<'a, Expr<'a>>] }
Expr::Binop { use_id: UseId, op: Located<BinOp>, left: Node<'a, Expr<'a>>, right: Node<'a, Expr<'a>> }
Expr::Negate { use_id: UseId, expr: Node<'a, Expr<'a>> }
Pattern::Pin { use_id: UseId, value: Node<'a, Expr<'a>> }
Stmt::Assign { use_id: Option<UseId>, place: &'a Place<'a>, op: Located<AssignOp>, value: Node<'a, Expr<'a>> }
```

`Stmt::Assign.use_id` is `None` for `=` and `Some` for compound numeric
assignment. A pinned pattern emits Eq evidence. A compound assignment must
evaluate its place once. Call IDs permit direct-call specialization without
depending on the syntactic shape of the callee.

Use IDs need only be unique within a module because the elaboration map is
owned by that module. This avoids replacing the existing
`Node = &Located<T>` representation. Interface serialization carries complete
declaration IDs rather than recomputing them.

## Package identity

The orphan rule cannot use the current `PackageId::Application` for every
workspace member. Before coherence checking, the driver must assign every
member its real package identity:

- configured packages use `PackageId::Named(author/project)`;
- a standalone application uses `PackageId::Application`;
- workspace applications receive a stable workspace-member application ID;
- embedded stdlib modules use `PackageId::Builtin`.

`PackageId` therefore gains `ApplicationMember(&str)` (the arena-owned,
workspace-relative member path). The driver passes a `ModuleSource { uri,
package, source_root }` into compilation rather than deriving every module ID
from the URI as `Application`. A package is local only when the `PackageId`s
are equal.

## Public canonical representation

Kinds and predicates live in `alder-ast` because canonicalization, solving,
interfaces, and code generation share them:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
```

Curried `Kind::Arrow` avoids a separate representation for each constructor
arity. M3 has no kind polymorphism and no higher-rank kinds.

The source and canonical `Type` enums add:

```rust
// alder_source::Type
Type::Hole

// alder_ast::Type
Type::Partial {
    constructor: QualifiedName<'a>,
    slots: &'a [CanonicalTypeSlot<'a>],
}
Type::Projection(ProjectionType<'a>)

pub enum CanonicalTypeSlot<'a> {
    Hole(u16),
    Fixed(Node<'a, Type<'a>>),
}
```

The parser accepts `_` as a type atom and preserves its one-byte region; the
canonicalizer accepts it only as a direct argument of a named constructor in
an impl head, such as `Result[_, e]`. It is rejected as the whole impl subject,
in value annotations, aliases, enum fields, associated bindings, and nested
arbitrary expressions. A partial type with `n` holes has the kind of an
`n`-argument constructor, with holes filled left-to-right. Hole ordinal is its
left-to-right position within that one partial expression. Holes bind the
enclosing partial; they are not inference variables. Canonicalization consumes
source holes into one `Type::Partial`, so a raw hole cannot escape into an
ordinary solved annotation. A normalized partial may remain in an impl-head
interface template. `Type::Projection` is canonical only; source syntax reaches
it through a bare associated name or an associated equality.

Annotations become constrained schemes:

```rust
pub struct Annotation<'a> {
    pub params: &'a [TypeParam<'a>],
    pub trait_predicates: &'a [TraitRef<'a>],
    pub projection_equalities: &'a [ProjectionEquality<'a>],
    pub typ: Node<'a, Type<'a>>,
}

```

All existing consumers migrate in wave 0; there is no permanent parallel
`free_vars` field. Quantifiers are ordered by first occurrence in the value
type, then trait predicates, then projection equalities. Only trait predicates
have runtime dictionaries.

Trait and impl declarations become semantic records instead of preserving the
source item union:

```rust
pub struct AssocTypeDecl<'a> {
    pub id: AssocTypeId<'a>,
    pub kind: Kind<'a>,
    pub region: Region,
}

pub struct MethodParam<'a> {
    pub mutable: bool,
    pub pattern: Node<'a, Pattern<'a>>,
    pub typ: Node<'a, Type<'a>>,
}

pub struct TraitMethod<'a> {
    pub id: MethodId<'a>,
    pub params: &'a [MethodParam<'a>],
    pub scheme: &'a Annotation<'a>,
    pub default_body: Option<Node<'a, Block<'a>>>,
    pub region: Region,
}

pub struct TraitDecl<'a> {
    pub id: TraitId<'a>,
    pub params: &'a [TypeParam<'a>],
    pub superclasses: &'a [TraitRef<'a>],
    pub associated_types: &'a [AssocTypeDecl<'a>],
    pub methods: &'a [TraitMethod<'a>],
}

pub struct AssocBinding<'a> {
    pub assoc: AssocTypeId<'a>,
    pub typ: Node<'a, Type<'a>>,
    pub region: Region,
}

pub struct ImplMethod<'a> {
    pub method: MethodId<'a>,
    pub params: &'a [MethodParam<'a>],
    pub scheme: &'a Annotation<'a>,
    pub body: Node<'a, Block<'a>>,
    pub region: Region,
}

pub struct ImplDecl<'a> {
    pub id: ImplId<'a>,
    pub trait_ref: TraitRef<'a>,
    pub params: &'a [TypeParam<'a>],
    pub trait_predicates: &'a [TraitRef<'a>],
    pub projection_equalities: &'a [ProjectionEquality<'a>],
    pub assoc_bindings: &'a [AssocBinding<'a>],
    pub methods: &'a [ImplMethod<'a>],
    pub synthetic: Option<DeriveKind>,
    pub region: Region,
}

pub enum DeriveKind { Show, Eq, Ord, Hash, Json }
```

Every trait method parameter and return type must be annotated. Its scheme does
not recursively contain the owning trait predicate; using the method emits that
predicate separately. An impl method may omit annotations only where the
corresponding trait signature uniquely supplies them; its canonical
`MethodParam`s and scheme contain the filled, checked types. Method-level
predicates may not be stronger than the trait method contract after subject
substitution: the trait contract must entail every implementation predicate.

`ValueRef` adds:

```rust
ValueRef::TraitMethod {
    method: MethodId<'a>,
    annotation: &'a Annotation<'a>,
}
```

Trait methods are inserted into the ordinary value namespace. The already
parsed `PathVar` form `Show::show` resolves an imported/local trait alias then
selects its method directly. A bare or normal module-qualified method follows
ordinary import/name ambiguity rules; M3 does not retain an overload set under
one bare name. This keeps name resolution deterministic before inference.

## Interfaces and build-wide trait database

Public trait definitions and every impl header must cross module boundaries.
Impl visibility is metadata visibility, independent of whether a source item
has `pub` spelling.

```rust
pub struct InterfaceTrait<'a> {
    pub exported_as: &'a str,
    pub id: TraitId<'a>,
    pub params: &'a [TypeParam<'a>],
    pub superclasses: &'a [TraitRef<'a>],
    pub associated_types: &'a [AssocTypeDecl<'a>],
    pub methods: &'a [InterfaceMethod<'a>],
}

pub struct InterfaceMethod<'a> {
    pub id: MethodId<'a>,
    pub exported_as: &'a str,
    pub scheme: &'a Annotation<'a>,
    pub has_default: bool,
    pub default_symbol: Option<&'a str>,
}

pub enum InterfaceValueIdentity<'a> {
    Binding(QualifiedName<'a>),
    TraitMethod(MethodId<'a>),
}

pub enum MethodImplementation<'a> {
    Provided { symbol: &'a str },
    Default { symbol: &'a str },
}

pub struct InterfaceImpl<'a> {
    pub id: ImplId<'a>,
    pub params: &'a [TypeParam<'a>],
    pub trait_ref: TraitRef<'a>,
    pub trait_predicates: &'a [TraitRef<'a>],
    pub projection_equalities: &'a [ProjectionEquality<'a>],
    pub assoc_bindings: &'a [AssocBinding<'a>],
    pub dictionary_symbol: &'a str,
    pub dictionary_kind: DictionaryKind,
    pub methods: &'a [(MethodId<'a>, MethodImplementation<'a>)],
}

pub enum DictionaryKind { Singleton, Factory }
```

`InterfaceValue` gains `identity: InterfaceValueIdentity`; public trait methods
also appear in the top-level searchable `Interface.values` table. A local trait
puts its methods in the local value namespace. Importing a trait name alone
does not import its bare methods but makes `TraitAlias::method` available;
importing `method` exposes the bare spelling, and an open import imports both.
Normal module qualification (`Module.method`) remains available. Two bare
method spellings collide under ordinary value-name rules rather than becoming
an overload set.

`InterfaceType` and `InterfaceEnum` replace bare parameter names with
`TypeParam`s and carry their result kind, so downstream kind inference never
guesses constructor kinds. `Interface` gains `instances: &[InterfaceImpl]` for
externally usable impls. An impl is externally usable only when its trait, head,
predicates, projection equations, and associated bindings contain public,
externally nameable identities. All local impls remain in the in-memory package
header database even when omitted from a published interface.

Package dependencies expose a package-level instance-index artifact. Importing
any module from a package makes that package's exported instance index
available; reaching the same package through multiple paths deduplicates by
`ImplId`. This gives package-wide instance semantics rather than accidental
import-closure semantics. Evidence-selected foreign dictionaries create ESM
dependency edges even when no source-level value import refers to their module.

The persistent format is an owned, versioned DTO; the former export-name-only
cache has been removed:

```rust
pub struct InterfaceFile {
    pub format_version: u32,
    pub compiler_version: String,
    pub module: OwnedModuleId,
    pub values: Vec<OwnedValue>,
    pub types: Vec<OwnedTypeDecl>,
    pub traits: Vec<OwnedTrait>,
    pub instances: Vec<OwnedImplHeader>,
    pub modules: Vec<OwnedModuleExport>,
    pub private_names: Vec<OwnedPrivateName>,
    pub fingerprint: [u8; 32],
}

pub struct PackageInstanceIndexFile {
    pub format_version: u32,
    pub compiler_version: String,
    pub package: OwnedPackageId,
    pub modules: Vec<OwnedModuleId>,
    pub instances: Vec<OwnedImplHeader>,
    pub fingerprint: [u8; 32],
}

pub enum OwnedPackageId {
    Named { author: String, project: String },
    Application,
    ApplicationMember(String),
    Builtin,
}
pub struct OwnedModuleId { pub package: OwnedPackageId, pub path: Vec<String> }
pub struct OwnedQualifiedName { pub module: OwnedModuleId, pub name: String }
pub struct OwnedTraitId(pub OwnedQualifiedName);
pub struct OwnedMethodId { pub trait_: OwnedTraitId, pub index: u16, pub name: String }
pub struct OwnedAssocTypeId { pub trait_: OwnedTraitId, pub index: u16, pub name: String }
pub struct OwnedImplId { pub module: OwnedModuleId, pub origin: ImplOrigin }

pub enum OwnedKind { Type, Arrow(Box<OwnedKind>, Box<OwnedKind>) }
pub struct OwnedTypeParam { pub name: String, pub kind: OwnedKind }
pub struct OwnedTraitRef { pub trait_: OwnedTraitId, pub args: Vec<OwnedType> }
pub struct OwnedProjection {
    pub trait_ref: OwnedTraitRef,
    pub assoc: OwnedAssocTypeId,
}
pub struct OwnedProjectionEquality {
    pub projection: OwnedProjection,
    pub typ: OwnedType,
}
pub struct OwnedScheme {
    pub params: Vec<OwnedTypeParam>,
    pub trait_predicates: Vec<OwnedTraitRef>,
    pub projection_equalities: Vec<OwnedProjectionEquality>,
    pub typ: OwnedType,
}
pub enum OwnedType {
    Var { name: String, args: Vec<OwnedType> },
    Named { reference: OwnedQualifiedName, args: Vec<OwnedType> },
    Fn { params: Vec<OwnedType>, ret: Box<OwnedType> },
    Unit,
    Tuple(Vec<OwnedType>),
    Record { fields: Vec<OwnedRecordField>, ext: Option<String> },
    ErrorRow { tags: Vec<OwnedErrorTag>, ext: Option<String> },
    Alias { reference: OwnedQualifiedName, arguments: Vec<OwnedAliasArgument>, target: Box<OwnedType> },
    Partial { constructor: OwnedQualifiedName, slots: Vec<OwnedTypeSlot> },
    Projection(OwnedProjection),
}
pub enum OwnedTypeSlot { Hole(u16), Fixed(OwnedType) }
pub struct OwnedAliasArgument { pub name: String, pub typ: OwnedType }
pub struct OwnedRecordField { pub index: u16, pub name: String, pub optional: bool, pub typ: OwnedType }
pub struct OwnedErrorTag { pub index: u16, pub name: String, pub args: Vec<OwnedType> }
pub struct OwnedValue { pub exported_as: String, pub identity: OwnedValueIdentity, pub scheme: OwnedScheme, pub kind: ValueKind }
pub enum OwnedValueIdentity { Binding(OwnedQualifiedName), TraitMethod(OwnedMethodId) }
pub struct OwnedModuleExport { pub exported_as: String, pub module: OwnedModuleId }
pub struct OwnedPrivateName { pub name: String, pub namespace: Namespace }
pub struct OwnedTypeDecl { pub exported_as: String, pub reference: OwnedQualifiedName, pub params: Vec<OwnedTypeParam>, pub result_kind: OwnedKind, pub body: OwnedPublicTypeBody }
pub enum OwnedPublicTypeBody { Alias(OwnedType), Opaque(OpaqueKind), Enum(Vec<OwnedVariant>), ErrorGroup(Vec<OwnedErrorTag>) }
pub struct OwnedVariant { pub name: String, pub index: u16, pub alternatives: u16, pub payload: OwnedVariantPayload }
pub enum OwnedVariantPayload { Unit, Tuple(Vec<OwnedType>), Record(Vec<OwnedRecordField>) }
pub struct OwnedAssocType { pub id: OwnedAssocTypeId, pub kind: OwnedKind }
pub struct OwnedMethod { pub id: OwnedMethodId, pub scheme: OwnedScheme, pub has_default: bool, pub default_symbol: Option<String> }
pub enum OwnedMethodImplementation { Provided { symbol: String }, Default { symbol: String } }
pub struct OwnedTrait {
    pub id: OwnedTraitId,
    pub params: Vec<OwnedTypeParam>,
    pub superclasses: Vec<OwnedTraitRef>,
    pub associated_types: Vec<OwnedAssocType>,
    pub methods: Vec<OwnedMethod>,
}
pub struct OwnedImplHeader {
    pub id: OwnedImplId,
    pub source_uri: Option<String>,
    pub region: Option<Region>,
    pub params: Vec<OwnedTypeParam>,
    pub trait_ref: OwnedTraitRef,
    pub trait_predicates: Vec<OwnedTraitRef>,
    pub projection_equalities: Vec<OwnedProjectionEquality>,
    pub assoc_bindings: Vec<(OwnedAssocTypeId, OwnedType)>,
    pub dictionary_symbol: String,
    pub dictionary_kind: DictionaryKind,
    pub methods: Vec<(OwnedMethodId, OwnedMethodImplementation)>,
}
```

These DTOs carry no borrowed data; they may retain copied `Region`s and source
URIs for diagnostics. `dehydrate(&Interface) ->
InterfaceFile` and `InterfaceFile::hydrate(&Bump) -> Interface` perform the only
conversion. This list is closed: a new canonical public type variant must add
its owned equivalent in the same change.
Canonical interface impls retain their source region, and the driver attaches
the source URI while dehydrating a module. Hydration restores both fields, so a
package index does not lose diagnostic provenance at an arena boundary.
Serialization sorts all maps/sets into semantic source order and hashes the
canonical encoded bytes with SHA-256; it never uses `DefaultHasher` as a file
contract. M3 increments `format_version`. Loading rejects incompatible format
or compiler versions and rebuilds. `hydrate(&Bump) -> Interface` reconstructs
arena-backed values. A default-body change changes its artifact fingerprint;
signature/default-presence changes also change the interface fingerprint.
The dependency resolver loads `PackageInstanceIndexFile` once when any module
from that package is imported. Saving validates that every listed impl belongs
to a listed module and exposes only externally nameable identities.
For path dependencies, the project resolver reads the referenced package's
validated `.alder` artifacts, hydrates every interface named by its index, and
adds the complete index to the frozen database even when the defining instance
module was not imported directly. Source modules carry their declared package
identity, and `~/` imports retain that identity rather than becoming application
imports. Duplicate interface/index paths converge by stable `ImplId`.
Successful driver builds return deterministic owned interface files and one
deduplicated instance index per package. CLI build, check, run, and test flows
persist those artifacts below `.alder/interfaces/` and `.alder/instances/` only
after the complete build succeeds.

M3 preserves one `Bump` per module, with that module's source copied into it.
Parsed modules and canonical bodies borrow only their corresponding arena.
Dependency interfaces are deep-copied into the active module arena before
canonicalization, and a successful solved interface is deep-copied into the
separate build/interface arena before the module arena is dropped. No phase
therefore borrows another module's allocation, and the build/interface arena
never owns module AST nodes. Persistent header data crossing module boundaries
uses the owned DTOs below; the frozen trait database owns all of its names and
types. The driver performs:

1. Fetch and parse every module.
2. Resolve module/package identities and imports.
3. Predeclare `HeaderShell`s for every current-package module.
4. Canonicalize each borrowed `ModuleHeader` in dependency graph order against
   owned dependency headers, then dehydrate it to `OwnedModuleHeader`:
   named types, traits, impl heads, and derives.
5. Build one `TraitDatabase` from builtins, dependency package indexes, and every
   current-package header.
6. Validate superclass cycles, orphan rules, duplicate impls, and overlap over
   that complete database.
7. Canonicalize bodies and solve modules in dependency order against the frozen
   database.
8. Build solved public interfaces and defensively repeat coherence validation
   over the final linked closure.

The current driver realizes this as a discovery fixed point followed by a
final body pass. `canonicalize_headers` runs ordinary declaration/import/type
validation but omits value items and substitutes empty, region-preserving
blocks for trait defaults and impl methods. Each header is copied immediately
to the build arena, even when full body canonicalization or solving fails.
Provisional successful solves contribute inferred public value schemes needed
to discover downstream headers. Once no header or solved interface changes,
the driver recompiles every module against that identical frozen closure and
runs coherence before body canonicalization can mask a package error.

Header defaults carry only `has_default` and a deterministic symbol. The
default body is canonicalized later with ordinary local IDs and combined with
the matching method header to produce the final `TraitMethod`.

The shared frontend records and APIs are:

```rust
pub struct HeaderShell<'m> {
    pub home: ModuleId<'m>,
    pub types: &'m [TypeShell<'m>],
    pub traits: &'m [TraitShell<'m>],
}

pub struct TypeShell<'m> {
    pub name: QualifiedName<'m>,
    pub arity: u16,
    pub visibility: Visibility,
    pub region: Region,
}

pub struct TraitShell<'m> {
    pub id: TraitId<'m>,
    pub arity: u16,
    pub visibility: Visibility,
    pub region: Region,
}

pub struct TypeHeader<'m> {
    pub shell: TypeShell<'m>,
    pub params: &'m [TypeParam<'m>],
    pub result_kind: Kind<'m>,
    pub body: HeaderTypeBody<'m>,
}

pub enum HeaderTypeBody<'m> {
    Alias(Node<'m, Type<'m>>),
    Opaque(OpaqueKind),
    Enum(&'m [Variant<'m>]),
    ErrorGroup(&'m [ErrorTagType<'m>]),
}

pub struct ModuleHeader<'m> {
    pub home: ModuleId<'m>,
    pub types: &'m [TypeHeader<'m>],
    pub traits: &'m [TraitHeader<'m>],
    pub instances: &'m [InstanceHeader<'m>],
}

pub struct HeaderImports<'m> {
    pub resolved: &'m [ResolvedImport<'m>],
    pub local_headers: &'m [OwnedModuleHeader],
    pub dependency_interfaces: &'m [InterfaceFile],
}

pub struct OwnedModuleHeader {
    pub home: OwnedModuleId,
    pub types: Vec<OwnedHeaderType>,
    pub traits: Vec<OwnedHeaderTrait>,
    pub instances: Vec<OwnedImplHeader>,
}

pub enum OwnedVisibility { Private, Public }
pub struct OwnedHeaderType { pub visibility: OwnedVisibility, pub declaration: OwnedTypeDecl }
pub struct OwnedHeaderTrait { pub visibility: OwnedVisibility, pub declaration: OwnedTrait }

pub fn predeclare_header<'m>(
    bump: &'m Bump,
    home: ModuleId<'m>,
    source: &'m alder_source::Module<'m>,
) -> Result<HeaderShell<'m>, Vec<CanonicalError<'m>>>;

pub fn canonicalize_header<'m>(
    bump: &'m Bump,
    shell: &HeaderShell<'m>,
    imports: &HeaderImports<'m>,
    source: &'m alder_source::Module<'m>,
) -> Result<ModuleHeader<'m>, Vec<CanonicalError<'m>>>;

pub fn canonicalize_body<'m>(
    bump: &'m Bump,
    header: &ModuleHeader<'m>,
    traits: &TraitDatabase,
    source: &'m alder_source::Module<'m>,
) -> Result<&'m Module<'m>, Vec<CanonicalError<'m>>>;
```

`OwnedModuleHeader` is the owned-interface mirror of `ModuleHeader`. Type and
trait shells contain name, arity, visibility, definition region, and stable ID;
full headers add kinds, method schemes, superclasses, associated declarations,
impl prerequisites/bindings, symbols, and origins. `CanonicalError` is the
sum `Core(alder_can::Error)` or `Trait(CanonicalTraitError)`. Header errors are
copied into the calling module arena before return.

This header pass is required: collecting “modules compiled so far” makes an
overlap error depend on filename or dependency order. Candidate lookup is
indexed first by `TraitId`, then by a rigid outer subject constructor where one
exists.

## Canonical validation

Canonicalization reports all independent declaration errors it can find.

The canonical environment adds:

```rust
pub struct TraitBinding<'a> {
    pub header: &'a TraitHeader<'a>,
    pub region: Region,
    pub visibility: Visibility,
}

pub struct MethodBinding<'a> {
    pub id: MethodId<'a>,
    pub scheme: &'a Annotation<'a>,
    pub region: Region,
    pub visibility: Visibility,
}

pub enum Candidate<'a, T> {
    Unique(T),
    Ambiguous(&'a [T]),
    Private { owner: ModuleId<'a>, value: T },
}

// Fields on alder_can::Env
pub traits: BTreeMap<&'a str, Candidate<'a, TraitBinding<'a>>>,
pub methods: BTreeMap<&'a str, Candidate<'a, MethodBinding<'a>>>,
```

Local insertion checks same-namespace duplicates immediately. Qualified trait
lookup addresses `TraitBinding`; `Trait::method` resolves the trait alias and
then its method ID, while module access remains `Module.method`. Named/open
imports hydrate public methods into `methods`, while importing only a trait name
does not. Private methods follow their owning trait's visibility. Candidate
resolution produces the existing unknown/private/ambiguous shape with method
suggestions from edit distance over visible names.

For traits it validates:

- at least one parameter, with argument zero designated as the subject;
- duplicate parameters, associated types, and methods;
- complete method annotations;
- constraint variables occur in the trait head or method signature;
- colon bounds name unary traits and are well-kinded; multi-parameter traits
  remain usable in impl heads and method-owned predicates represented after
  canonicalization;
- duplicate direct superclasses and direct or transitive superclass cycles;
- an associated name in a signature belongs to this trait.

Inside a trait, a declared associated type shadows a module type of the same
spelling, and bare `Item` resolves to the projection of the current trait over
all its parameters. Inside an impl it resolves to the implemented trait's
projection, which subsequently normalizes using the impl binding. Otherwise
ordinary type lookup applies. The spelling `Iterator[i]::Item` in diagnostics
and this document is internal notation, not additional surface syntax; outside
trait/impl bodies and `i.Item == T`, arbitrary projections have no M3 syntax.
If an associated type has constructor kind, bare `Item[a]` canonicalizes as an
application of that projection and its inferred kind must accept the argument.

For `where i.Item == Number`, canonicalization gathers the declared bounds on
`i`, finds the traits that define `Item`, and requires exactly one. The selected
`AssocTypeId` is stored in the projection. Unknown or ambiguous associated names
are canonical errors, not solver guesses.

For impls it validates:

- trait head arity and kinds;
- impl-level `where` variables already occur in the impl head (a where clause
  never introduces a variable); method-level variables and predicates belong
  only to that method;
- method and associated names exist, are unique, and have the right kind;
- every required associated binding and non-default method is present;
- method signatures equal the trait signature after substituting the subject;
- the orphan and overlap rules below.

### Orphans and overlap

The subject is trait argument zero. Expand aliases, then inspect its outermost
nominal constructor. An impl is legal exactly when either:

- the trait's package equals the impl module's package; or
- that outer nominal type constructor's package equals the impl module's
  package.

Variables, functions, tuples, anonymous records, and foreign container
constructors are not local. Thus a foreign trait for `Array[Local]` is an
orphan, because `Array`, not `Local`, is the outer constructor. If the trait is
local, any well-kinded subject is allowed.

Two impls overlap when, after alpha-renaming their parameters and expanding
aliases, first-order unification can make their complete trait heads equal.

`alder-can` performs this check as soon as an impl head has been canonicalized,
so ordinary source modules receive the orphan error before body inference. The
frozen `TraitDatabase` repeats the same check across collected and hydrated
headers as a defense against stale or corrupt dependency metadata.
Where predicates are deliberately ignored. Any such pair is an error; M3 has
no specialization, priorities, negative impls, or local instances.

Impl predicates must mention only variables in the impl head. This coverage
rule plus instance-search cycle detection prevents unconstrained dictionary
construction. Termination additionally requires every impl prerequisite to be
strictly structurally smaller than the impl head and no head variable to occur
more often in the prerequisite. The check follows mutually recursive trait
prerequisites as a group. The current colon-bound grammar only permits a head
variable on the left, which naturally keeps common container impls decreasing;
the structural check remains authoritative for canonical/synthetic headers.

Size is measured after alias expansion and partial beta-normalization:
`size(Var) = size(Con) = 1`; `size(App(head, args)) = size(head) +
sum(size(args))`; `size(Partial(constructor, slots)) = 1 + sum(slot_size)`,
where a hole and a fixed variable each have slot size one and any other fixed
type has its ordinary size. A predicate/head's size is the sum of all trait
argument sizes. Function, tuple, closed-record, and closed-error-row size is
one plus the sizes of all children. Stuck projections, open rows, and unresolved
application heads are forbidden in impl heads/prerequisites. For each direct
prerequisite, total size must be less than the complete impl head's total size,
and every variable's occurrence count must be no greater than in the head.
Trait names do not affect size, so strict decrease also proves termination
through mutually recursive traits.

## Kind inference

Kind checking is a first-order unification prepass before value inference. Its
internal kinds are `KVar` and `Arrow(K, K)`; every public kind is fully zonked.

- A named constructor with `n` declared parameters starts with `n` fresh
  parameter kinds ending in `Type`.
- `f[a]` generates `kind(f) ~ kind(a) -> result`; use as a value type forces
  `result ~ Type`. Consequently `Functor[f]` infers `f: Type -> Type`.
- Trait arguments unify with the declared kinds of trait parameters.
- Functions, tuples, records, error rows, and concrete value types have kind
  `Type`.
- `Result[_, e]` has kind `Type -> Type`; applying it fills its hole.
- Associated-type kinds are inferred during the defining trait's header check
  solely from trait-local method signatures and uses. An unconstrained
  associated kind defaults to `Type`, is serialized in the trait interface,
  and is frozen before any impl is checked. Impl bindings must match it and
  never refine it.
- There is no kind polymorphism. Truly unconstrained declaration parameters
  default to `Type`; contradictory uses produce `KindMismatch`.

`impl Functor[Number]` therefore reports that `Number` has kind `Type` where
`Type -> Type` is required.

## Inference representation

The active inferencer replaces `Ty::Named` and the HKT-erasing `Ty::Any` path
with an application-preserving representation:

```rust
enum ITy<'a> {
    Var(TyVar),
    Con(QualifiedName<'a>),
    App { head: Box<ITy<'a>>, args: Vec<ITy<'a>> },
    Partial { constructor: QualifiedName<'a>, slots: Vec<TypeSlot<'a>> },
    Fn(Vec<ITy<'a>>, Box<ITy<'a>>),
    Unit,
    Tuple(Vec<ITy<'a>>),
    Record(/* existing fields and row extension */),
    ErrorRow(/* existing row representation */),
    Projection { trait_: TraitId<'a>, args: Vec<ITy<'a>>, assoc: AssocTypeId<'a> },
    Error,
}

enum TypeSlot<'a> { Hole(u16), Fixed(ITy<'a>) }

struct Scheme<'a> {
    quantified: Vec<TyVar>,
    kinds: Vec<(TyVar, IKind)>,
    predicates: Vec<IPredicate<'a>>,
    projection_eqs: Vec<IProjectionEq<'a>>,
    typ: ITy<'a>,
}
```

Applying a `Partial` fills holes left-to-right and reduces immediately.
Unifying partials compares constructor and fixed slots after alpha-renaming
hole positions. General type lambdas and higher-order unification are outside
M3.

Before every unification, application spines are flattened and partial
applications beta-reduced. M3 also implements one restricted higher-kinded
pattern rule needed by ordinary `map` inference:

```text
App(Var(f), pattern_args) ~ rigid
```

where `m = pattern_args.len()`, may solve `f` only when every argument is a
distinct flexible type variable, `f`
does not occur in `rigid`, the rigid side is a named constructor application
with at least `m` arguments, and kind unification permits the abstraction. By
language convention it unifies the pattern arguments with the leftmost `m` constructor
arguments, replaces those positions with fresh left-to-right partial holes,
and binds `f` to the resulting `Partial`. This matches Alder's standard
`Result[_, e]` orientation. The rule is symmetric. For example:

```text
f[a] ~ Result[Number, String]
=> a ~ Number
=> f ~ Result[_, String]
```

Arguments that repeat, are not variables, or create an occurs/kind cycle make
the equation `UnsupportedHigherKindedUnification` or the more specific
infinite-type/kind diagnostic. A program that needs a non-leftmost section
must expose that section explicitly in an impl head; general higher-order
inference is outside M3. The equation is never resolved by choosing an
instance. Hole numbers are local to the new `Partial` and alpha-renamed before
equality.

Equal-length `App`/`App` equations first structurally unify heads and
corresponding arguments. If neither side is constructor-headed enough for
either structural or restricted abstraction, the equation is deferred and
retried after every relevant substitution is zonked; it is diagnosed only at
the enclosing inference boundary if still unsolved.

Occurs checks, substitutions, free-variable collection, pretty-printing, and
interface conversion all traverse application heads, partial slots, and
projections. `ITy::Error` replaces the permissive `Any` escape hatch and only
suppresses cascades after an actual diagnostic.

## Obligations and inference order

Using a trait method instantiates its scheme and emits its owning predicate.
Using any constrained value instantiates all scheme predicates. Operators emit
the following obligations after unifying their operands:

| Syntax | Predicate | Result |
| --- | --- | --- |
| `==`, `!=` | `Eq[a]` | `Bool` |
| `<`, `<=`, `>`, `>=` | `Ord[a]` | `Bool` |
| `+`, `-`, `*`, `/`, `%`, unary `-` | `Num[a]` | `a` |

Number and BigInt literals stay concrete; M3 has no numeric defaulting.

```rust
struct Obligation<'a> {
    id: ObligationId,
    use_id: UseId,
    predicate: IPredicate<'a>,
    origin: Region,
    reason: ObligationReason<'a>,
}
```

Canonicalization must populate `module.value_sccs`; the current empty slice is
a wave-0 defect. It records dependency edges for every top-level binding and
runs deterministic Tarjan SCC ordering. A multi-binding pattern let contributes
one node per bound name with the same outgoing edges; constrained generalized
pattern lets are rejected unless the pattern binds exactly one name, avoiding
multiple incompatible evidence factories in M3.

```rust
struct SccCallEdge<'a> {
    caller: QualifiedName<'a>,
    callee: QualifiedName<'a>,
    use_id: UseId,
    callee_substitution: Vec<(TyVar, ITy<'a>)>,
}
```

SCC placeholders are monomorphic; M3 does not support polymorphic recursion.
Each edge records the callee instantiation in the SCC's shared inference-variable
space.

Top-level values are inferred and generalized by those SCCs, not item order:

1. Predeclare monomorphic placeholders for every SCC member.
2. Instantiate explicit signatures with rigid variables and register their
   declared predicates as givens.
3. Infer all bodies and solve equality and kind constraints.
4. Record `SccCallEdge`s and each member's direct obligations. The lattice is a
   stable ordered set of normalized `IPredicate`s over the SCC's shared
   monomorphic variables. An edge transfer substitutes the callee predicate
   through `callee_substitution`, minimizes superclasses, removes predicates
   entailed by caller givens, then stable-unions the result into the caller.
   Iterate to a least fixpoint. The set is finite because monomorphic
   unification rejects type-growing recursive calls and transfer can only
   instantiate the finite direct/callee predicate set. If recursive `f` calls
   `g` and only `g` calls `show`, propagation discovers the requirement for
   `f`; it becomes a parameter only when `f` declares an entailing bound,
   otherwise `f` receives `UnsatisfiedBound`.
5. Zonk types and obligations; quantify variables not free in the outer
   environment. `free_vars(env)` is the union of free variables in every
   reachable local/global scheme after removing that scheme's quantified
   variables. Mutable bindings do not generalize. Local block lets remain
   sequential and non-generalized in M3.
6. Discharge ground obligations.
7. Every residual obligation over quantified variables must be entailed by an
   explicit `where` predicate. Otherwise report `UnsatisfiedBound` and suggest
   the clause. Alder does not silently infer public trait constraints.
8. Preserve predicate declaration order, deduplicate, and remove predicates
   entailed by another predicate's superclass closure. `Ord[a]` makes a
   separate `Eq[a]` parameter redundant.
9. Reject a quantified variable that occurs only in predicates or projection
   equalities and not in the value type as ambiguous.
10. Construct evidence for every recursive call from the caller's declared
    dictionary parameters and ground impls, and store its `DirectCall` action.

Parameter and return annotations on one function form one signature. A type
variable spelling is shared across all of them and becomes a rigid skolem while
checking the body; missing annotations remain flexible. A declared where
variable must occur in that combined signature. At the end of a monomorphic or
non-generalizable declaration, any still-deferred variable-headed obligation is
`AmbiguousTypeVariable`.

This policy implements the documented rule that generic code writes its
bounds. It also makes interfaces and dictionary ABI stable under body edits.

## Instance resolution

Resolution takes a zonked goal, the declaration's givens, the frozen database,
and an active-goal stack:

```rust
fn resolve<'a>(
    infer: &mut Infer<'a>,
    goal: &IPredicate<'a>,
    givens: &[Given<'a>],
    instances: &InstanceIndex<'a>,
    stack: &mut Vec<GoalKey<'a>>,
) -> Result<Evidence<'a>, SolveTraitError<'a>>;
```

It proceeds deterministically:

1. Zonk the goal and normalize projections where evidence permits.
2. Search givens and their transitive superclasses first. Retain the evidence
   path rather than adding redundant parameters.
3. If the goal's zonked outer subject head is a variable or stuck projection,
   defer. A known rigid outer constructor selects candidates even when inner
   arguments remain unknown (`Show[Array[a]]`). Instance search never chooses
   an impl to fix a wholly variable subject.
4. Fresh-instantiate each indexed impl and one-way-match impl variables against
   the goal. Only the fresh impl binders are writable. Goal variables are
   rigid. Matching flattens `App`, beta-reduces `Partial`, expands transparent
   aliases, structurally matches closed rows, and rejects open-row,
   unnormalized-projection, or unresolved-application heads. Candidate
   prerequisites never disambiguate overlapping heads.
5. Recursively resolve the substituted impl predicates and normalize its
   associated equations.
6. No successful candidate is `MissingInstance`; more than one is
   `AmbiguousInstance`; exactly one yields impl evidence.

Ground successful goals are memoized. Repeating a normalized goal on the
active stack is `InstanceCycle`, never recursion until stack overflow. Failure
retains the deepest nested requirement so diagnostics explain, for example,
that `Show[Array[T]]` failed because `Show[T]` is missing.

The declaration-time structural-decrease rule is the primary termination
proof. Resolution also carries a deterministic fuel count proportional to the
number of indexed impl headers plus the input goal size as defense against
corrupt dependency metadata; exhausting it reports `InstanceCycle` with the
active chain.

Entailment is recursive, not textual. A declared `Show[a]` together with the
generic builtin `Show[Array[x]] where x: Show` entails the residual composite
goal `Show[Array[a]]` and produces applied impl evidence.

An `impl Ord[T]` must satisfy its direct superclass `Eq[T]`, either through an
impl predicate or a global instance. Superclass dictionaries are stored in
source order and expanded transitively only during entailment.

## Associated type normalization

The solver keeps compile-time equalities separate from runtime evidence:

```rust
struct AssumptionSet<'a> {
    trait_givens: Vec<Given<'a>>,
    projection_equations: Vec<IProjectionEq<'a>>,
}

struct Given<'a> {
    predicate: IPredicate<'a>,
    evidence: Evidence<'a>,
    origin: Region,
}
```

Projection normalization first consults `projection_equations`, then resolves
an impl and substitutes its associated binding. It recurses through the result
with an active projection stack. Conflicting equations report
`AssocTypeMismatch`; a normalization loop reports `ProjectionCycle`.

Ordinary unification of `Projection ~ T` eagerly normalizes when possible. If
it remains stuck, it enqueues an `IProjectionEq` after an occurs check rather
than binding through the opaque projection. A stuck projection generalized in
a scheme retains both its equality and its well-formedness trait predicate;
instantiation re-emits both. Projection result variables never enter the
determination closure for their subject, because associated types are
non-injective.

A projection whose trait input is still polymorphic remains in the generalized
scheme together with its trait predicate. It is not an error merely because it
is not yet reducible. Associated types are non-injective, so
`Iterator[i]::Item == Number` cannot choose or infer `i`.

Impl checking substitutes the trait head into every method scheme and
projection, checks each associated binding's kind, and checks each body under
the impl assumptions. A default body is checked once under a synthetic current
trait dictionary and its superclass givens. An omitted impl method specializes
that default with the current dictionary, so calls among defaults dynamically
dispatch through the dictionary and recursion works.

## Elaboration and evidence

Solving returns annotations plus a side table. It does not mutate the canonical
AST or manufacture JavaScript strings:

```rust
pub struct TraitDatabase {
    pub traits: BTreeMap<OwnedTraitId, OwnedTrait>,
    pub instances: BTreeMap<OwnedTraitId, Vec<OwnedImplHeader>>,
    pub structural_rules: Vec<StructuralRule>,
}

impl TraitDatabase {
    pub fn build(
        builtins: &[OwnedModuleHeader],
        dependency_interfaces: &[InterfaceFile],
        dependencies: &[PackageInstanceIndexFile],
        local_headers: &[OwnedModuleHeader],
    ) -> Result<Self, Vec<DatabaseError>>;
}

pub enum DatabaseError {
    SuperclassCycle { traits: Vec<OwnedTraitId> },
    OrphanImpl { impl_: OwnedImplId, subject: String },
    OverlappingImpl { first: OwnedImplId, second: OwnedImplId, witness: String },
    InvalidTermination { impl_: OwnedImplId, predicate: String },
    DuplicateIdentity { description: String },
}

pub struct TraitHeader<'a> {
    pub id: TraitId<'a>,
    pub params: &'a [TypeParam<'a>],
    pub superclasses: &'a [TraitRef<'a>],
    pub associated_types: &'a [AssocTypeDecl<'a>],
    pub methods: &'a [InterfaceMethod<'a>],
}

pub struct InstanceHeader<'a> {
    pub site: ImplSite<'a>,
    pub params: &'a [TypeParam<'a>],
    pub trait_ref: TraitRef<'a>,
    pub trait_predicates: &'a [TraitRef<'a>],
    pub projection_equalities: &'a [ProjectionEquality<'a>],
    pub assoc_bindings: &'a [AssocBinding<'a>],
    pub dictionary_symbol: &'a str,
    pub dictionary_kind: DictionaryKind,
    pub methods: &'a [(MethodId<'a>, MethodImplementation<'a>)],
}

pub enum StructuralRule { ClosedTupleEq, ClosedRecordEq }

pub struct Constraints<'m> {
    pub module: &'m Module<'m>,
    pub requirement_seeds: &'m [RequirementSeed<'m>],
}

pub struct RequirementSeed<'a> {
    pub use_id: UseId,
    pub kind: RequirementKind<'a>,
    pub region: Region,
}

pub enum RequirementKind<'a> {
    TraitMethod(MethodId<'a>),
    Eq,
    Ord,
    Num,
}

pub struct SolveOutput<'a> {
    pub annotations: Annotations<'a>,
    pub schemes: BTreeMap<QualifiedName<'a>, &'a Annotation<'a>>,
    pub bindings: BTreeMap<QualifiedName<'a>, BindingEvidence<'a>>,
    pub uses: BTreeMap<UseId, UseAction<'a>>,
    pub impls: &'a [ElaboratedImpl<'a>],
}

pub struct BindingEvidence<'a> {
    pub dictionary_params: &'a [TraitRef<'a>],
    pub abi: BindingAbi,
}

pub enum BindingAbi {
    PlainValue,
    DirectFunction,
    EvidenceFactory,
}

pub struct EvidenceParamId<'a> {
    pub owner: EvidenceOwner<'a>,
    pub index: u16,
}

pub enum DirectTarget<'a> {
    Binding(QualifiedName<'a>),
    TraitMethod {
        method: MethodId<'a>,
        implementation: Option<MethodImplementation<'a>>,
    },
}

pub enum UseAction<'a> {
    Reference {
        dictionaries: &'a [Evidence<'a>],
        method: Option<MethodId<'a>>,
    },
    DirectCall {
        callee_use: UseId,
        dictionaries: &'a [Evidence<'a>],
        target: Option<DirectTarget<'a>>,
    },
    IndirectCall,
    Operator { dictionary: Evidence<'a> },
    Pin { dictionary: Evidence<'a> },
    CompoundAssign { dictionary: Evidence<'a> },
}

pub enum Evidence<'a> {
    Param(EvidenceParamId<'a>),
    SelfDictionary { owner: EvidenceOwner<'a> },
    Super { base: &'a Evidence<'a>, slot: u16 },
    Impl {
        impl_id: ImplId<'a>,
        module: ModuleId<'a>,
        symbol: &'a str,
        kind: DictionaryKind,
        arguments: &'a [Evidence<'a>],
        intrinsic: Option<Intrinsic>,
    },
    StructuralEq {
        shape: StructuralEqShape<'a>,
        fields: &'a [Evidence<'a>],
    },
}

pub enum StructuralEqShape<'a> {
    Tuple(u16),
    Record(&'a [&'a str]),
}

pub enum Intrinsic {
    EqNumber, EqString, EqBool, EqBigInt,
    OrdNumber, OrdString, OrdBigInt,
    NumNumber, NumBigInt,
}

pub struct ElaboratedMethod<'a> {
    pub method: MethodId<'a>,
    pub implementation: MethodImplementation<'a>,
    pub method_dictionary_params: &'a [TraitRef<'a>],
}

pub struct ElaboratedImpl<'a> {
    pub id: ImplId<'a>,
    pub symbol: &'a str,
    pub kind: DictionaryKind,
    pub prerequisite_params: &'a [TraitRef<'a>],
    pub superclasses: &'a [Evidence<'a>],
    pub methods: &'a [ElaboratedMethod<'a>],
}
```

`StructuralRule` is the closed set of compiler-owned structural evidence rules
(`Eq` for closed tuples and closed records). It participates in ambiguity and
evidence semantics but is not a user impl header and cannot overlap a named
user impl. Open record rows have no structural Eq in M3.

For a trait-method reference/call action, `dictionaries[0]` is the owning-trait
dictionary and the rest are method-level trait predicates in scheme order. For
an ordinary constrained value, every entry aligns with
`Annotation.trait_predicates`; projection equalities never occupy a slot.
Nested lambdas retain `EvidenceParamId.owner` for captured dictionaries, and
the emitter resolves IDs through an evidence-environment stack.

Evidence is applied exactly once. A `DirectCall` tells the emitter to lower its
callee use without the callee's `Reference` wrapper and prepend the call's
dictionaries. Every other constrained reference emits an evidence-capturing
closure (or invokes an evidence factory for a non-function value). An
`IndirectCall` adds nothing because the callee expression already produced a
plain callable closure. Structural Eq evidence constructs a real `{ eq }`
dictionary, with recursively selected field evidence, so it can flow into
generic calls; record names are stored in canonical sorted order.

The phase signatures are:

```rust
pub fn constrain<'m>(bump: &'m Bump, module: &'m Module<'m>) -> Constraints<'m>;

pub fn solve<'m>(
    bump: &'m Bump,
    constraints: &Constraints<'m>,
    traits: &TraitDatabase,
) -> Result<SolveOutput<'m>, Vec<SolveError<'m>>>;

pub fn emit_module<'m>(
    module: &'m Module<'m>,
    solved: &SolveOutput<'m>,
    options: EmitOptions,
) -> Result<EmittedModule, EmitError>;
```

Selected database headers/evidence and foreign error payloads are deep-copied
into the module/output arena; no database borrow appears in `SolveOutput<'m>` or
`SolveError<'m>`.
The obsolete zero-sized `UnionFind` argument is removed. `alder-constrain`
walks syntax once to seed dispatch requirements; type-dependent obligations are
completed by inference, so solver and constrain never both allocate Use IDs.

## JavaScript dictionary ABI

Trait-predicate order is ABI order: explicit source order, with stable
deduplication; projection equalities take no slot, and superclasses do not add
parameters when entailed by another predicate. Generic constrained functions
receive dictionaries as leading hidden parameters. A constrained non-function
top-level binding becomes an evidence factory and is invoked with evidence at
each use. Extern declarations may declare bounds; their emitted adapter takes
the dictionaries but calls the foreign ABI with source arguments only unless
the extern attribute explicitly names an Alder-aware ABI in a later milestone.

A constrained direct call prepends evidence arguments. Passing a constrained
function as a value emits a closure that captures those dictionaries, including
through `if`, records, arrays, and nested lambdas. A trait method call uses this
ABI:

```text
dictionary method: method-local dictionaries, then source arguments
provided impl symbol: impl prerequisites, method-local dictionaries, source arguments
default helper: current self dictionary, method-local dictionaries, source arguments
```

The owning trait dictionary is not passed twice. When evidence and target are
ground, codegen may call a local/intrinsic concrete implementation directly.
Foreign provided/default targets are described by `InterfaceImpl.methods`; if
not imported for direct specialization, codegen safely falls back to
`dict.method(...)`.

A closed impl invokes a uniform builder once at module initialization:

```javascript
function $make$d7() {
  const $self = {};
  $self.show = (value) => $show$User(value);
  return Object.freeze($self);
}
const $d7 = $make$d7();
```

An impl with dictionary prerequisites is necessarily a factory:

```javascript
function $make$d8($Show$a) {
  const $self = {};
  $self.show = (xs) => $show$Array($Show$a, xs);
  return Object.freeze($self);
}
const $d8 = ($Show$a) => $make$d8($Show$a);
```

Memoizing factories by dependency dictionary identity is optional. The plan's
“one object per impl” applies only to closed impls. HKT impls without
prerequisites, such as `Functor[Option]`, are ordinary singletons.

Dictionary fields use source method names. Direct superclass dictionaries use
reserved fields `$super0`, `$super1`, ... in source order. Associated types have
no runtime field. Every dictionary uses shell/assign/freeze construction, not
only dictionaries that appear recursive. An omitted/default method closes over
`$self`; a direct static call to it passes `$self` to the default helper. Thus a
default calling an override, sibling default, or itself always dispatches
through the current dictionary.

Dictionary and reusable default symbols are compiler-private ESM exports even
though no source value is public. Symbols are deterministic module-local
manglings of `ImplId.origin`; the module specifier supplies global uniqueness.
Imported evidence uses collision-proof local aliases and contributes its module
to `EmittedModule.dependencies`, including modules whose only retained effect
is an impl. All identifier/property strings are copied into the Oxc allocator;
none borrow the source arena. All nodes are built with Oxc's AST builder and
passed to Rolldown as the emitted module representation already used by M2.

Pinned-pattern equality uses selected Eq evidence, not kernel `$equal`.
Compound assignment expands through its selected Num method while caching every
place component, so an indexed place is evaluated once.

## Builtins, derives, and operators

Builtin traits and instances enter the same `TraitDatabase` as user code.
Instance search does not contain ad-hoc branches for each trait. `Intrinsic`
evidence is the code-generation optimization marker recognized only after one
of those normal headers wins the same matching and prerequisite search as a
user instance.

The embedded stdlib defines `Show`, `Eq`, `Ord`, `Hash`, `Num`, `Functor`,
`Applicative`, `Monad`, `Traversable`, `Iterator`, and `Json`, plus primitive and
container impls. These stdlib definitions remain audited Alder/JS source files;
compiler synthesis is reserved for derives and automatic structural `Eq`.
Their headers are loaded into both the canonical name environment and the
`TraitDatabase` before operators or derive paths are canonicalized.

`std/Traits.ald` is embedded and header-canonicalized for each solver arena. It
is the audited source of every first-party trait header and the primitive,
container, HKT, and Array iterator instance headers. The canonicalizer retains
a minimal bootstrap name table because that same source defines the names it
must parse; a parity test compares every bootstrap trait ID, arity, and method
ID with the canonical source interface so the bootstrap cannot become an
independent language contract.

The fixed first-party declarations used by operators and derives are equivalent
to:

```alder
enum Ordering { Less, Equal, Greater }

trait Show[a] { fn show(value: a) String }
trait Eq[a] { fn eq(left: a, right: a) Bool }
trait Ord[a] where a: Eq { fn compare(left: a, right: a) Ordering }
trait Hash[a] where a: Eq { fn hash(value: a) BigInt }
trait Num[a] where a: Eq + Ord {
    fn add(left: a, right: a) a
    fn sub(left: a, right: a) a
    fn mul(left: a, right: a) a
    fn div(left: a, right: a) a
    fn rem(left: a, right: a) a
    fn negate(value: a) a
}
trait Json[a] {
    fn encode(value: a) String
    fn decode(text: String) Result[a, String]
}
trait Functor[f] { fn map(value: f[a], transform: fn(a) b) f[b] }
trait Applicative[f] where f: Functor {
    fn pure(value: a) f[a]
    fn apply(function: f[fn(a) b], value: f[a]) f[b]
}
trait Monad[f] where f: Applicative {
    fn flat_map(value: f[a], transform: fn(a) f[b]) f[b]
}
trait Traversable[t] {
    fn traverse(value: t[a], transform: fn(a) f[b]) f[t[b]]
        where f: Applicative
}
trait Iterator[i] {
    type Item
    fn next(iterator: i) Option[Item]
}
```

Operator and derive lookup uses these canonical first-party TraitIds, never a
lexical trait with the same spelling. Trait parameters are instantiated first
in a method scheme; method-only variables follow in first-occurrence order.

`#[derive(Show, Eq, Ord, Hash, Json)]` accepts enums (including record-payload
variants) and error groups. Alder currently has transparent record aliases, not
nominal record declarations, so aliases cannot coherently own Show/Ord/Hash/Json
instances. M3 rejects Show, Ord, Hash, and Json derives on aliases, and rejects
all derives on functions and opaque/table/schema types. Closed record aliases
inherit the same structural Eq as their expanded
anonymous row. `#[derive(Eq)]` on one is an idempotent assertion and creates no
impl. `docs/language.md` is corrected to use an enum derive example;
a future nominal-record declaration may become an additional derive target.

Attribute arguments must be trait paths. Canonical qualified or unqualified
paths are resolved, but M3 accepts only the five first-party derive traits.
Every argument retains its region; unknown/inapplicable arguments and duplicate
derives report structured errors. Explicit `#[derive(Eq)]` is an idempotent
request that suppresses a second implicit Eq synthesis. Every synthetic header
enters ordinary orphan/overlap checking with its `ImplOrigin`.

Derivation emits one obligation per payload field under an assumed recursive
instance for the type currently being derived. It resolves ground obligations,
retains only residual predicates over head parameters, deduplicates them, and
rejects impossible ground requirements such as a function field. Merely
mentioning a type parameter does not automatically create a bound.

The solver also records the selected evidence for each
`(ImplId, variant index, field index)`. Generated Show, Eq, Ord, Hash, and Json
dictionaries embed that evidence in their payload shape; they never inspect a
field with an unqualified generic kernel operation. Builtin Array, Option, and
Result evidence retains its child dictionaries recursively, so a field such as
`Array[Option[a]]` ultimately dispatches through the selected dictionary for
`a`. Structural Eq evidence likewise records whether it describes an Array,
Option, Result, tuple, or record, plus its child dictionaries.

Synthetic Eq dictionaries are emitted before source and derived dictionaries
that initialize Eq superclass slots. References used only inside dictionary
method bodies remain lazy, which permits recursive and later-declared field
instances without a JavaScript temporal-dead-zone access.

Eq is synthesized without an attribute for enums and error groups whose field
obligations succeed. Closed tuples and anonymous closed records (including
transparent aliases of them) use structural Eq rules at the use site; open rows
have no Eq because unknown fields cannot be ignored. Array, Option, Result, and
unit use builtin impls. Recursive Eq uses the assumed current instance.
Functions have no Eq. A user Eq impl overlaps automatic Eq and is rejected
except for an opaque type, for which an explicit user impl is the only possible
Eq.

Derived behavior is fixed:

- Show prints Alder constructor syntax; records use `{ field: value }` in
  source field order. Runtime strings use double quotes; double quote,
  backslash, newline, carriage return, tab, and NUL use their standard short
  backslash escapes. Other control scalars use a lowercase-hex Unicode escape,
  and other Unicode scalars render literally. It does not depend on lost source
  spelling.
- Ord orders enum variants by declaration index, then payloads
  lexicographically; records use source field order.
- Hash returns unsigned 64-bit FNV-1a encoded as `BigInt`, with offset
  `14695981039346656037`, prime `1099511628211`, and masking modulo `2^64`
  after each byte. Primitive streams begin with fixed tags: unit `00`, Bool
  `01`, Number `02`, BigInt `03`, String `04`; structural/enum values use
  `10`/`11`/`12`. Lengths and field/variant indices are unsigned 64-bit
  little-endian. Strings hash UTF-8 length then bytes. Numbers hash normalized
  IEEE-754 little-endian bits (`-0` becomes `0`; all NaNs become
  `0x7ff8000000000000`). BigInts hash sign byte, big-endian magnitude length,
  then magnitude bytes. A parent feeds each child hash as eight little-endian
  bytes preceded by its field/index marker; enum streams also include the
  length-prefixed UTF-8 canonical type name and declaration variant index.
- Json encodes record-payload variants as JSON objects in source field order.
  An optional field whose value is `None` is omitted. Enums encode as
  `{ "tag": "Variant", "fields": [...] }`; record-payload variants use an
  additional `"value"` object instead of `"fields"`. Decoding requires that
  exact shape and returns a path-qualified string error.

Static primitive operations are:

- `Eq[Number|String|Bool|BigInt]`: JavaScript `===`/`!==`;
- `Ord[Number|String|BigInt]`: native relational operators;
- `Num[Number|BigInt]`: native arithmetic operators with the normal
  Number/BigInt separation.
- `Eq[()]`: always true.

Non-intrinsic operator lowering is exact:

```text
left == right  -> eq.eq(left, right)
left != right  -> !eq.eq(left, right)
left + right   -> num.add(left, right)   // likewise sub/mul/div/rem
-value         -> num.negate(value)
left < right   -> ord.compare(left, right).$ === "Less"
left <= right  -> ord.compare(left, right).$ !== "Greater"
left > right   -> ord.compare(left, right).$ === "Greater"
left >= right  -> ord.compare(left, right).$ !== "Less"
```

Generic comparison uses the normal tagged `Ordering` enum ABI. Primitive Ord
intrinsics emit native comparison directly and do not allocate Ordering.

Consequently Number NaN values are unequal and `-0 == 0` is true. Hashing must
respect Eq: it canonicalizes both signed zeros to the same hash, while no NaN
values are required to compare equal. Unit variants use tag identity; known
records/enums call generated helpers such as `$eq$Type`.

The kernel representation of Option must preserve arbitrary nesting. Constructing
`Some` around `null` or an existing compiler Option box creates another tagged
box; it must not collapse `Some(Some(None))`. Derived/builtin Eq tests cover at
least three nested levels.

`Ref.same(a, b)` remains explicit reference identity and never satisfies an Eq
obligation.

## Diagnostics

Canonical errors and type errors are structured data with regions and related
locations. Shared payloads retain enough information for cross-file rendering:

```rust
pub struct ImplSite<'a> {
    pub id: ImplId<'a>,
    pub module: ModuleId<'a>,
    pub region: Option<Region>,
    pub origin: ImplOrigin,
}

pub struct ObligationFrame<'a> {
    pub trait_: TraitId<'a>,
    pub subject: &'a str,
    pub required_by: Option<ImplId<'a>>,
}

pub struct DisplayType<'a>(pub &'a str);
pub struct DisplayKind<'a>(pub &'a str);
pub struct Suggestion<'a>(pub &'a str);

pub enum SolveError<'a> {
    Core(alder_constrain::Error),
    Trait(SolveTraitError<'a>),
    Coherence(CoherenceError<'a>),
}

pub enum SolveTraitError<'a> {
    MissingInstance { trait_: TraitId<'a>, subject: &'a str, origin: Region, chain: &'a [ObligationFrame<'a>] },
    AmbiguousInstance { trait_: TraitId<'a>, subject: &'a str, origin: Region, details: &'a AmbiguousInstanceDetails<'a> },
    UnsatisfiedBound { trait_: TraitId<'a>, subject: &'a str, origin: Region, chain: &'a [ObligationFrame<'a>] },
    InstanceCycle { trait_: TraitId<'a>, subject: &'a str, origin: Region, chain: &'a [ObligationFrame<'a>] },
}

pub enum CanonicalTraitError<'a> {
    OrphanImpl { site: ImplSite<'a>, trait_: TraitId<'a>, subject: DisplayType<'a>, trait_package: PackageId<'a>, type_package: Option<PackageId<'a>> },
    OverlappingImpl { first: ImplSite<'a>, second: ImplSite<'a>, witness: TraitRef<'a> },
    UnknownAssocType { name: Name<'a>, suggestion: Option<Suggestion<'a>> },
    AmbiguousAssocType { name: Name<'a>, traits: &'a [TraitId<'a>] },
    MissingAssocBinding { assoc: AssocTypeId<'a>, impl_: ImplSite<'a> },
    UnknownMethod { name: Name<'a>, trait_: TraitId<'a>, suggestion: Option<Suggestion<'a>> },
    MissingMethod { method: MethodId<'a>, impl_: ImplSite<'a> },
    MethodTypeMismatch { method: MethodId<'a>, expected: DisplayType<'a>, actual: DisplayType<'a>, region: Region },
    SuperclassCycle { traits: &'a [TraitId<'a>], regions: &'a [Region] },
    InvalidDerive { region: Region, reason: DeriveError<'a> },
    Duplicate { namespace: Namespace, name: &'a str, first: Region, second: Region },
}

pub enum DeriveError<'a> {
    Unknown { name: &'a str },
    InvalidArgument,
    InvalidTarget { derive: DeriveKind },
    Duplicate { derive: DeriveKind, first: Region },
    ImpossibleField { field: &'a str, required: TraitId<'a> },
}
```

Dependency candidates can have `region: None` when source is unavailable.
Compiler phases retain these typed errors; the driver converts them while both
the arena-backed error and owned module source are available. The shared
`alder-report` crate owns the presentation-neutral diagnostic:

```rust
pub struct Source(Arc<NamedSource<String>>);

#[derive(thiserror::Error)]
pub struct Diagnostic {
    source_code: Source,
    message: String,
    code: Option<String>,
    severity: miette::Severity,
    labels: Vec<miette::LabeledSpan>,
    help: Option<String>,
    related: Vec<Diagnostic>,
}

impl miette::Diagnostic for Diagnostic { /* metadata accessors */ }
```

`Source` converts Alder's one-indexed byte-based `Region` coordinates to
miette byte spans and is shared across a module's diagnostics. `BuildResult`
carries owned diagnostics for failures and warnings. The CLI hands them to
miette, retaining normal terminal-aware colors; golden renderer tests choose a
fixed width and `unicode_nocolor()` explicitly so snapshots are deterministic.
Parser renderer tests keep the nested syntax error snapshot and separately
snapshot the final source excerpt, labels, help, and code. Wording and context
follow Elm's `Reporting/Error/Syntax.hs` where the construct has an Alder
equivalent, adapted to Alder syntax. Phase-local terminal formatting and
`format!("{:?}")` are not acceptable final paths.

Required trait diagnostics include:

- `MissingInstance { goal, origin, nested, suggestion }`
- `AmbiguousInstance { goal, candidates }`
- `OrphanImpl { trait_, subject, trait_package, type_package }`
- `OverlappingImpl { first, second, witness }`
- `UnsatisfiedBound { goal, declaration, suggested_where }`
- `KindMismatch { expected, actual, context }`
- `UnknownAssocType` and `AmbiguousAssocType`
- `MissingAssocBinding` and `AssocTypeMismatch`
- `UnknownMethod`, `MissingMethod`, and `MethodTypeMismatch`
- `SuperclassCycle` and `InstanceCycle`
- `AmbiguousTypeVariable`
- duplicate trait parameter, method, associated type, impl method, binding, or
  derive errors.

Messages name the user spelling of types and traits, underline the triggering
use, show candidate impl locations for ambiguity/overlap, and show the nested
obligation chain. Common arithmetic failures lead with the concrete expected
numeric type where available, preserving useful M2 diagnostics.

Every recursive instance lookup pushes an `ObligationFrame`. A leaf failure
retains the complete root-to-leaf slice; cycle errors retain the repeating
portion of the active stack. Ambiguity reports label every candidate in the
current module and list foreign candidates by module, explicitly marking sites
whose source is unavailable.

Inference retains the source spelling for every generalized type variable used
by an obligation. Reports render those names (for example, `a`) rather than
solver implementation details such as numeric unification-variable IDs.
Coherence reports label the exact impl sites involved and include an actionable
help message; overlap reports label both source sites when both are available.

## File ownership and landing order

Wave 0 is serial because it changes shared contracts:

- `alder-source`, `alder-parse`: narrowly scoped type-hole syntax and snapshots.
- `alder-ast`: identities, kinds, predicates, semantic trait/impl records,
  constrained annotations, interface records, use IDs, projections.
- `alder-driver`: package identity, header phase, frozen trait database plumbing.
- `alder-can`, `alder-constrain`, `alder-solve`, `alder-codegen`: compile against
  the new signatures with intentionally incomplete behavior made explicit in
  tests, never hidden by `#[ignore]`.

After that, disjoint owners are:

- frontend: `alder-can` trait environments, canonical validation, derives,
  interface construction, and structured diagnostics;
- inference: `alder-constrain` plus active `alder-solve/inference.rs`, kind
  inference, SCC generalization, resolution, projection normalization, evidence;
- backend/runtime: `alder-codegen`, embedded stdlib/kernel trait definitions,
  dictionaries, static lowering, and runtime snapshots;
- integration: `alder-driver`, package-wide headers, interface persistence, and
  cross-module/package end-to-end tests.

Owners do not revive the obsolete Elm solver modules. Shared type changes after
wave 0 require updating this contract first.

## Verification matrix

Granular success and error snapshots are required alongside runtime tests.

| Requirement | Required evidence |
| --- | --- |
| Direct instance | solver evidence snapshot and runtime call |
| Declared bound | scheme/evidence snapshot for generic `describe` |
| Superclass | `Ord[a]` supplies `Eq[a]` with one dictionary parameter |
| Associated equality | normalization success plus ambiguous-name error |
| HKT | Option, Array, and `Result[_, e]`; `Functor[Number]` kind error |
| Applicative | `pure` and `apply` compile and run through generic and ground calls |
| HKT unification | recover `Result[_, String]`; two-hole order; partial mismatch; occurs/kind and alias cases |
| Nested resolution | `Show[Array[Option[a]]]` and nested missing chain |
| Coherence | cross-module/package orphan and overlap tests, reordered modules |
| Defaults | omitted method, mutual/default recursion, static specialization |
| Derives | runtime behavior for all five on enums/error groups; recursive enum; implicit/explicit Eq dedupe; invalid alias/duplicate/overlap cases |
| Operators | every Eq/Ord/Num operator, primitive snapshots, generic dispatch |
| Equality | record helper, open-row/function rejection, pin evidence, nested Option, `Some(1) == Some(1)` runtime |
| First-class constrained fn | callback closure captures dictionary |
| Recursion | nonempty SCCs; 2/3-member predicate fixpoints; callbacks; bound mismatch; order independence |
| Generalization | subtract environment FVs; mutable/local restrictions; partial annotations; ambiguity |
| Search termination | decreasing nested impl; growing/cross-trait cycles; variable-head deferral |
| Diagnostics | one internal and rendered driver/CLI insta snapshot per variant, including multi-file and source-unavailable sites |
| Methods/imports | trait-qualified, module-qualified, named/open import, collision, method-local bound order |
| Interfaces | owned round trip; kinds/projections/impls/symbols; private filtering; version/hash changes |
| Evidence linkage | foreign factory, evidence-only module edge, colliding symbols, projection equality has no JS arg |
| Oxc ownership | artifact prints after source/solve arenas drop |
| Assignment/pins | indexed compound place evaluates once; pins use Eq |
| Docs | every complete trait example parses, compiles, and runs where applicable |

The milestone gate is:

```text
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build --workspace --all-targets --all-features
```

Before M3 is checked in `SPEC.md`, run the docs-example critic, inspect all new
snapshots, remove no longer referenced snapshots, add the multi-crate Sampo
changeset, and verify the CLI builds and runs the trait end-to-end fixtures.
