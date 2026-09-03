# M3: Traits

Type classes in the Haskell style with Rust spelling: the subject type is
an explicit parameter (`trait Show[a]`, `impl Show[User]`), higher-kinded
parameters are inferred from use (`trait Functor[f]`, `impl Functor[Option]`),
bounds live in `where` clauses, there is no `self` and no method-call
sugar, and Rust's orphan rule applies. Codegen is dictionary passing with
static resolution wherever the type is known.

## Starting state

- M2a done: `trait` and `impl` items canonicalize into the environment but
  constraints are ignored; `where` clauses parse and are dropped.
- M2b done: codegen exists; a trait function call currently fails to
  resolve because nothing dispatches it.
- Reference: the parser contract §5.15 (trait/impl grammar), `docs/language.md`
  "Traits" and "Type application and variables".

## Exit criteria

- `trait`, `impl`, bounds in `where`, associated types, default method
  bodies, superclass constraints (`trait Ord[a] where a: Eq`), and
  higher-kinded parameters type-check with Elm-quality errors for missing
  instances, ambiguous instances, orphan impls, and unsatisfiable bounds.
- A generic function `fn describe(xs: Array[a]) -> String where a: Show`
  compiles to JS taking a dictionary and is called with a statically
  selected dictionary at monomorphic call sites.
- `Functor`, `Applicative`-style HKT traits work end to end (`map` over
  `Option`, `Array`, `Result[_, e]` partially applied).
- Built-in derives `Show`, `Eq`, `Ord`, `Hash`, `Json` exist as compiler
  implementations behind `#[derive(...)]`, to be replaced by macro derives
  in M5 without changing user code.
- `+`, `-`, `*`, `/`, `%`, comparisons, and `==` have their final typing
  (decision 1): `Eq`, `Ord`, and `Num` traits with static resolution.

## Settled decisions

- Explicit subject parameter; `impl Trait[Type]`; no `Self`, no receivers,
  no `x.show()`.
- Existing multi-parameter trait heads remain accepted; argument zero is the
  coherence subject. The current `where a: Trait` shorthand names unary traits
  only until bound syntax grows explicit trait arguments.
- Associated types: `type Item` in the trait, `type Item = T` in the impl,
  constrained in `where` with `i.Item == Number`.
- Default method bodies allowed in the trait.
- Orphan rule: an `impl` must live in the package defining the trait or
  the type.
- Higher-kinded variables get their kind inferred from use; `f[a]` in a
  type is `Type::Var { name, args }` in the source AST already.
- No explicit type arguments at call sites; ambiguity is resolved by
  annotation or is an error.
- Compiler phases keep structured, typed errors and use `thiserror` where an
  error owns its display text. Presentation is centralized in an
  `alder-report` crate: its owned diagnostic type implements
  `miette::Diagnostic`, retains the named source, translates Alder `Region`s
  to byte spans, and carries codes, severity, primary and secondary labels,
  help, and related diagnostics. The driver performs phase-to-report
  conversion while the module source and arena-backed errors are both alive;
  the CLI delegates snippet rendering for errors and warnings to miette.
- Snapshot helpers for compiler pipeline tests follow the parser convention:
  source is passed through `indoc!`, installed as insta's `description`, and
  `omit_expression` is enabled. Multiline Alder programs must not be hidden in
  escaped Rust expressions in snapshots.
- Solved module interfaces cross arena and persistent-cache boundaries through
  a complete owned DTO. Interface and package-instance-index files are versioned,
  reject compiler-version mismatches, and fingerprint canonical bincode bytes
  with SHA-256 rather than Rust's process-oriented `DefaultHasher`.
- Derived dictionaries consume solver-selected evidence for every payload
  field. Evidence is retained through nested builtin containers, and generated
  Eq dictionaries are initialized before dictionaries that use them as a
  superclass.
- Package builds run a header-only canonicalization pass that skips value,
  default-method, and implementation-method bodies. Canonical headers survive
  body failures, and a final body pass uses the same complete package header
  closure for coherence and instance resolution.
- First-party trait and primitive/container instance headers are authored in
  `std/Traits.ald`, canonicalized through the header pipeline, and inserted
  into the ordinary `TraitDatabase`. Intrinsic evidence is selected only after
  an ordinary matching header wins instance search.
- `Ord` has one `compare(left, right) -> Ordering` method. Generic comparisons
  inspect the `Less`/`Equal`/`Greater` tag, while primitive intrinsics retain
  allocation-free native relational operators.
- Trait and impl parser diagnostics descend through their function, parameter,
  pattern, block, expression-leaf, and complete type error hierarchies before
  building an owned miette report, preserving the innermost source location
  and syntax-specific correction.
- Trait solver and coherence reports preserve source type-variable spellings,
  never expose unification-variable IDs, label both local impl sites for an
  overlap, and attach actionable help for bounds, ownership, kinds, and cycles.
  Deterministic no-color snapshots cover the rendered source.
- Instance search retains root-to-leaf obligation frames in structured errors.
  Ambiguity reports label every available local candidate and identify foreign
  candidates whose source is unavailable; nested missing-instance reports show
  the prerequisite chain that led to the leaf failure.
- The complete Traits guide example is byte-for-byte tied to an executed CLI
  fixture. Runtime coverage also exercises generic Applicative dispatch,
  Monad, Traversable, Iterator, all three builtin Functors, and dictionary
  capture through a first-class constrained callback.

## Open decisions (recommendation in bold)

1. Arithmetic and equality (settled in discussion, recorded here).
   **`==` and `!=` are the `Eq` trait, as in Rust.** `Eq` is derived
   automatically (no `#[derive]` needed) for records, enums, tuples,
   arrays, options, results, and any type whose parts are `Eq`; it is
   overridable for opaque types; functions have no `Eq`, so comparing
   them is a compile error rather than Elm's runtime crash. Generic code
   writes `where a: Eq`. Identity comparison is explicit (`Ref.same(a, b)`).
   Cost model: when both sides have a known primitive type (`Number`,
   `String`, `Bool`, `BigInt`, unit variants) the codegen emits `===`
   directly; known records and enums call their generated `eq_T`; only
   polymorphic code pays a dictionary indirection. Cycles built with
   `mut` are the user's responsibility, as in Rust. **Arithmetic
   operators are methods of a `Num` trait with instances for `Number` and
   `BigInt`; comparisons are `Ord`.**
2. Coherence beyond orphans. **Overlapping impls are an error; no
   specialization.**
3. Instance resolution strategy. **The active solver gains deferred
   constraints: a bound is recorded when a trait function is used at a type
   variable, discharged when the variable becomes concrete, or checked against
   the enclosing function's explicit `where` clause during generalization.** A
   missing generic bound is reported with a suggested clause; body edits never
   silently change public dictionary ABI. Resolution runs per value SCC.
4. Dictionary representation. **A plain JS object per closed impl, one field
   per trait function (and per associated function), created once at module
   load; an impl with `where` prerequisites is a dictionary factory receiving
   those prerequisite dictionaries. Generic functions take dictionaries as
   leading parameters in predicate order.** HKT dictionaries are the same
   shape. A singleton for every impl is impossible for an impl such as
   `Show[Array[a]] where a: Show`, because its methods need `Show[a]`.
5. Superclass access. **A dictionary carries its superclass dictionaries
   as fields** (`Ord` dict has `eq`), Haskell style.

## Work breakdown

### Wave 0: contract

Design panel producing `docs/traits-internals.md`:

- Environment additions: trait declarations (params, kinds, superclasses,
  associated types, method signatures, defaults), impls (head types,
  where clauses, associated type bindings), orphan/coherence rules.
- Constraint representation in `alder-constrain` and the deferred
  constraint store in `alder-solve`; kind inference for HKT variables;
  associated type projection and normalization; generalization with
  bounds; error types (`MissingInstance`, `AmbiguousInstance`,
  `OrphanImpl`, `OverlappingImpl`, `UnsatisfiedBound`,
  `KindMismatch`, `AssocTypeMismatch`).
- Elaboration output: the canonical AST gains explicit dictionary
  parameters and arguments after solving (a separate elaborated form or
  annotations on nodes; the panel decides).
- Codegen rules for dictionaries and static resolution.
- Built-in derive implementations as a compiler pass over enums and error
  groups; closed records use structural Eq because Alder record aliases are
  transparent, not nominal declarations.
- File ownership.

The resulting contract also fixes package-wide, build-order-independent impl
collection and adds the narrowly scoped `_` type hole required to represent
`Result[_, e]` in an impl head. The hole is not a general inferred annotation.

### Wave 1: front end (parallel)

- `alder-can`: trait/impl environment, orphan and overlap checks, `where`
  clause collection, derive attribute recognition.
- `alder-constrain`: bound constraints, associated type constraints,
  superclass expansion.
- `alder-solve`: deferred constraints, instance search, kind inference,
  generalization with bounds, elaboration.
- Error rendering for the new error types through `alder-report` and miette;
  no phase-local terminal or source-snippet formatter.
- Tests: inference tests for every rule; snapshot tests for every error.

### Wave 2: back end (parallel)

- `alder-codegen`: dictionary construction per impl, dictionary passing,
  static resolution at monomorphic sites, default method inlining.
- `std/`: `Show`, `Eq`, `Ord`, `Hash`, `Num`, `Functor`, `Applicative`,
  `Monad`, `Traversable`, `Iterator` (with `Item`), `Json` traits and
  instances for the primitives and containers; `==` moved onto `Eq` with
  automatic structural instances and `===` emission for known primitives;
  `Ref.same` for identity; arithmetic moved onto `Num`.
- Built-in derives.
- e2e projects exercising generic functions, HKT `map`, derives.

### Wave 3: sweep

- Docs: `docs/language.md` traits section updated with the arithmetic
  decision; SPEC M3 ticked; changeset.
- Critic pass: every trait example in the docs compiles and runs.

## Tests to add (minimum)

- Instance resolution: direct, via bound, via superclass, via associated
  type equality, HKT partial application (`Result[_, e]`), nested
  (`Show[Array[Option[a]]]`).
- Errors: missing instance with suggestion, ambiguous, orphan, overlap,
  unsatisfied bound in a nested call, kind mismatch (`impl Functor[Number]`).
- Codegen: dictionary snapshots; runtime tests that `describe` and `map`
  produce expected output for three instances each; `==` on `Number`
  emits `===` (snapshot), on a record calls `eq_T`, on a function is a
  compile error, and `Some(1) == Some(1)` is true at runtime.

## Risks

- Deferred constraints interact with Elm's rank-based generalization;
  the design must state exactly when a constraint is generalized versus
  reported. Get this reviewed adversarially before wave 1.
- HKT kind inference is a new solver component; keep kinds simple (no
  higher-rank, no kind polymorphism beyond inference from use).
- Moving arithmetic onto `Num` changes every numeric error message; the
  renderer must still say "expected Number" in the common case.
