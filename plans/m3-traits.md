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
  (see open decision 1).

## Settled decisions

- Explicit subject parameter; `impl Trait[Type]`; no `Self`, no receivers,
  no `x.show()`.
- Associated types: `type Item` in the trait, `type Item = T` in the impl,
  constrained in `where` with `i.Item == Number`.
- Default method bodies allowed in the trait.
- Orphan rule: an `impl` must live in the package defining the trait or
  the type.
- Higher-kinded variables get their kind inferred from use; `f[a]` in a
  type is `Type::Var { name, args }` in the source AST already.
- No explicit type arguments at call sites; ambiguity is resolved by
  annotation or is an error.

## Open decisions (recommendation in bold)

1. Arithmetic and equality. **`==` and `!=` stay structural and built in
   for all non-function types (Elm's rule), so no `Eq` bound is needed to
   compare; `Eq` exists as a trait only for user-defined equality on
   opaque types and as a bound name. Arithmetic operators are methods of a
   `Num` trait with instances for `Number` and `BigInt`; comparisons are
   `Ord`.** This keeps `if a == b` free of bounds in generic code, which
   is what JS developers expect, while letting `BigInt` arithmetic work.
2. Coherence beyond orphans. **Overlapping impls are an error; no
   specialization.**
3. Instance resolution strategy. **Elm's solver gains deferred
   constraints: a bound is recorded when a trait function is used at a
   type variable, discharged when the variable is unified with a concrete
   type or generalized into the enclosing function's `where`.** Resolution
   runs after generalization, per declaration.
4. Dictionary representation. **A plain JS object per impl, one field per
   trait function (and per associated function), created once per impl
   at module load; generic functions take dictionaries as leading
   parameters in trait-declaration order.** HKT dictionaries are the same
   shape.
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
- Built-in derive implementations as a compiler pass over enum/record
  declarations.
- File ownership.

### Wave 1: front end (parallel)

- `alder-can`: trait/impl environment, orphan and overlap checks, `where`
  clause collection, derive attribute recognition.
- `alder-constrain`: bound constraints, associated type constraints,
  superclass expansion.
- `alder-solve`: deferred constraints, instance search, kind inference,
  generalization with bounds, elaboration.
- Error rendering for the new error types.
- Tests: inference tests for every rule; snapshot tests for every error.

### Wave 2: back end (parallel)

- `alder-codegen`: dictionary construction per impl, dictionary passing,
  static resolution at monomorphic sites, default method inlining.
- `std/`: `Show`, `Eq`, `Ord`, `Hash`, `Num`, `Functor`, `Applicative`,
  `Monad`, `Traversable`, `Iterator` (with `Item`), `Json` traits and
  instances for the primitives and containers; arithmetic moved onto
  `Num`.
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
  produce expected output for three instances each.

## Risks

- Deferred constraints interact with Elm's rank-based generalization;
  the design must state exactly when a constraint is generalized versus
  reported. Get this reviewed adversarially before wave 1.
- HKT kind inference is a new solver component; keep kinds simple (no
  higher-rank, no kind polymorphism beyond inference from use).
- Moving arithmetic onto `Num` changes every numeric error message; the
  renderer must still say "expected Number" in the common case.
