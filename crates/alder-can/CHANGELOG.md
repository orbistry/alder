# alder-can

## 0.7.0 — 2026-09-15

### Minor changes

- [ee01c34](https://github.com/orbistry/alder/commit/ee01c34d1568654083951d3d44a232b624bd5cbb) Implement the M6 web application workflow: checked-in reproducible HTML schema,
  typed components/events/children, compiler-tracked signals and stores, reactive
  directives and keyed reconciliation, resources, escaped SSR, safe hydration
  transport, and lifecycle ownership. Generate filesystem routes, typed params
  and load data, browser navigation, error boundaries, HTTP endpoints, remote
  query/command stubs, typed page actions/text forms, and server/client hooks.
  Enforce server-only reachability and exclude private stores from hydration.
  
  Add standalone and Cloudflare web build/dev adapters, state-preserving HMR and
  recoverable diagnostics, public assets, staged stale-output cleanup, static and
  dynamic prerendering, Cloudflare binding/DO/queue/workflow adapters, and an
  explicit-target deployment path with local dry-run validation. Package pinned
  compiler-owned platform support in native releases and version-proxy installs.
  Support owned Durable Object state and Workflow step wrappers with explicit
  native-handle identity equality and compiled execution coverage.
  Add direct compiler/kernel/routing/bundler/runtime regressions, opt-in browser
  checks, a source-only full example, and documented implementation/acceptance
  boundaries. A real preview deployment remains separately authorized verification,
  not a claim made by this changeset. — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-ast@0.4.0, alder-parse@0.6.0

## 0.6.1 — 2026-09-11

### Patch changes

- [f001988](https://github.com/orbistry/alder/commit/f001988a97cea7cb6d4b246d59f0e941e96f745b) Remove unlinked Elm-era canonicalization and constraint implementations from
  source packages. The active Alder canonicalizer and requirement traversal remain
  unchanged. — Thanks @rvcas!

## 0.6.0 — 2026-09-07

### Minor changes

- [2cc1d05](https://github.com/orbistry/alder/commit/2cc1d057460100bce03b09d5cda3f8e7975f1a87) Unify bundled, local, and external imports with grouped syntax, lowercase
  standard-library namespaces, explicit utility imports, identity-preserving
  namespace re-exports, and canonical comment-preserving formatting. Share public
  interfaces between prelude and explicit imports, reject conflicting bindings,
  and make module initialization independent of import declaration order.
  
  Interface format 8 replaces earlier contracts without compatibility readers. — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-ast@0.3.1, alder-parse@0.5.0, alder-source@0.4.0

## 0.5.0 — 2026-09-06

### Minor changes

- [8e03ebe](https://github.com/orbistry/alder/commit/8e03ebe56450b5f86574e1a2f5a5651c2d7b5023) Recover independent implementation and trait-default body failures using fresh inference attempts, retaining declaration contracts for diagnostics and suppressing dependent bodies without hiding unrelated methods. — Thanks @rvcas!
- [0b70ba7](https://github.com/orbistry/alder/commit/0b70ba754c594fecfb7fe7344e77ffb32f2ffe3f) Generate unused-local and parameter warnings from resolved source bindings,
  respecting shadowing, alternatives, captures, pins, guards, and writes. Explain
  safe pattern discards without recommending removal of needed initializer effects. — Thanks @rvcas!
- [6590795](https://github.com/orbistry/alder/commit/65907955dca4efc84622b5e72219274763826e95) Generate unused import-binding warnings through resolved source lookups, respecting
  aliases, shadowing, type and trait uses, and public re-exports. Preserve initializer
  effects and source-order diagnostic delivery in check, build, and test commands. — Thanks @rvcas!
- [effb878](https://github.com/orbistry/alder/commit/effb878c6a279ad19eb2f77feae8f57724ea1416) Warn on unused module value bindings using resolved references and conservative
  reachability. Respect exports, recursion, shadowing, entry points, tests and
  method bodies, while retaining side-effectful initializers and their helpers. — Thanks @rvcas!

### Patch changes

- [563041a](https://github.com/orbistry/alder/commit/563041aaef0af67e3c1813ad41235a2bd42c2f1e) Accumulate independent declaration type errors using fresh inference attempts,
  suppress dependent failures, and retain the failed-module publication barrier. — Thanks @rvcas!
- [5adcd76](https://github.com/orbistry/alder/commit/5adcd76c4e07726d71ba42a6566fe3901c1a6df6) Report independent statement type errors within a function using isolated retries, suppressing invalid local dependencies and delivering the results to CLI and editor consumers. — Thanks @rvcas!
- Updated dependencies: alder-parse@0.4.0

## 0.4.0 — 2026-09-06

### Minor changes

- [470e3ef](https://github.com/orbistry/alder/commit/470e3ef31e57097de46903b6a2c9513af3410c1c) Parse named optional parameter annotations and canonicalize their shorthand to
  ordinary builtin Option types, including lambda and trait signatures. — Thanks @rvcas!
- [f7fb26b](https://github.com/orbistry/alder/commit/f7fb26bab952eeb7c574638b5e237e82ebd817ad) Permit type-checked reassignment and field/index writes through ordinary let bindings and function or lambda parameters. Keep assignment-aware generalization restrictions on shared replaceable values.
  
  Remove obsolete mutability fields from canonical lets, parameters, and assignment places. Local pattern bindings are writable; non-storage references retain assignment-target checks with diagnostics that no longer suggest adding `mut`.
  
  Remove `mut` from the grammar, keyword list, and source AST. Parsing uses the current grammar without compatibility handling or migration diagnostics. — Thanks @rvcas!
- [83c2be3](https://github.com/orbistry/alder/commit/83c2be3acb3e9216511e9e91168a8365df08e992) Add lazy Ref cells with synchronous atomic reads, writes, updates, and modifications. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Treat optional record-field shorthand as an ordinary Option type and materialize omitted Option fields as None during contextual record construction.
  
  Remove separate record-field optionality from serialized interfaces and invalidate the previous interface format.
  
  Read Fiber traversal concurrency options using ordinary Option semantics so omitted and explicit None fields both select the sequential default.
  
  Preserve known generic Result error rows during propagation, including explicit Option fallback branches.
  
  Provide conditional Option ordering and Unit ordering so derived record payloads use ordinary field dictionaries, including nested Options. — Thanks @rvcas!
- [f87e540](https://github.com/orbistry/alder/commit/f87e540113feb6c1052b2d782a7ad6e1faa7d204) Add inferred lazy tasks, generator-based async lowering, Promise extern lifting,
  and a structured fiber runtime with interruption, scopes, finalizers, `all`, and
  `race`. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Expose Fiber.map, forEach, tryMap, and tryForEach with optional MapOptions,
  execution-time concurrency validation, and sequential defaults. — Thanks @rvcas!
- [0db756d](https://github.com/orbistry/alder/commit/0db756d7014df5c506fafe13df0b93306896d1fa) Represent explicit async function declarations and lazy async block syntax. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Resolve Hash structurally for closed error rows, honoring payload dictionaries
  and preserving hashes across equivalent groups and row widening. Share payload
  evidence with the equality superclass without duplicating nested generated code.
  Remove nominal error-group derives and their declaration-order-dependent behavior;
  error groups use conditional structural Eq, Show, Hash, and Json capabilities.
  Normalize derived enum payloads through the ordinary annotation converter so
  nominal wrappers of named error groups retain their checked structural evidence. — Thanks @rvcas!
- [83c2be3](https://github.com/orbistry/alder/commit/83c2be3acb3e9216511e9e91168a8365df08e992) Add fixed-capacity semaphores with FIFO weighted requests and scoped,
  cancellation-safe permit ownership. — Thanks @rvcas!
- [83c2be3](https://github.com/orbistry/alder/commit/83c2be3acb3e9216511e9e91168a8365df08e992) Add synchronized shared cells with serialized task-based state transformations. — Thanks @rvcas!
- [749ba31](https://github.com/orbistry/alder/commit/749ba31c566f66ced41107e9591da82ddd329370) Publish public named and wildcard imports in module interfaces, preserving
  original identities and checked schemes including multiple aliases of one name.
  Interface construction APIs now require dependency interfaces; the driver threads
  them through header and final publication. — Thanks @rvcas!

### Patch changes

- [1efea6c](https://github.com/orbistry/alder/commit/1efea6c0a882752faf882525f5f292e2b20a6360) Require Json evidence for module encode/decode calls and dispatch through the
  selected codec, including custom instances. Remove unchecked JSON entry points
  and fix public export names for imported direct dictionary calls. — Thanks @rvcas!
- [743a09c](https://github.com/orbistry/alder/commit/743a09c81ed6f65f5bddd21f7ebd853cf8350e18) Resolve pinned expressions against the enclosing scope before pattern bindings
  are introduced, preventing field-order-dependent lookup and uninitialized
  local references when patterns shadow an outer name. — Thanks @rvcas!
- [ea6b284](https://github.com/orbistry/alder/commit/ea6b2849eee22f4332de8319392a1cbe60d7c5d7) Resolve embedded stdlib members through packaged Alder signatures, rejecting unknown members, wrong arguments, invalid callbacks, and arity mismatches. Remove untyped builtin references and correct expected-versus-actual call diagnostics. — Thanks @rvcas!
- [808e7bb](https://github.com/orbistry/alder/commit/808e7bb774a1cf9d10d4003466738f72eca55745) Include assignment targets in top-level inference dependencies, including writes inside nested functions, so write-only references preserve recursive groups and dependency ordering. — Thanks @rvcas!
- [08c77e5](https://github.com/orbistry/alder/commit/08c77e55441ccd3a3ca944c7a5425a6139ac41f8) Reject imported types and traits bound under the same name, consistently with
  local declarations, including wildcard and renamed public re-exports. Preserve
  distinct aliases and report both conflicting source imports. — Thanks @rvcas!
- [daef79b](https://github.com/orbistry/alder/commit/daef79b53ec5d3249cef75704a1ed3d2f99c06fb) Reject recursive type-alias dependencies during canonicalization and report a
  source-aware diagnostic explaining how to represent recursive data with enums.
  Expand local and imported alias references with instantiated canonical targets,
  including generic arguments and ordinary record field types. — Thanks @rvcas!
- [cc51b66](https://github.com/orbistry/alder/commit/cc51b668b6a2c3bedd8d02b9a868eae6afb9c631) Honor local aliases when importing trait methods while preserving their original dispatch identity, and diagnose collisions at the aliased binding. — Thanks @rvcas!
- [d8c4983](https://github.com/orbistry/alder/commit/d8c4983815b570e62b4c3085e309a460f508ce20) Allow type variables in local let annotations and reuse the enclosing callable's
  generic variables consistently with lambda annotations. Preserve generic
  contract checking and monomorphic local bindings. — Thanks @rvcas!
- [570ee29](https://github.com/orbistry/alder/commit/570ee29a9364271e17c6e6e324be27cca24015f8) Preserve individual repeated trait-bound clauses without panicking, and avoid
  false associated-type ambiguity when the same trait is mentioned more than once. — Thanks @rvcas!
- [f7fb26b](https://github.com/orbistry/alder/commit/f7fb26bab952eeb7c574638b5e237e82ebd817ad) Preserve ordered record-overlay relationships through inference, generalization, and owned interfaces. Keep independent input rows distinct, respect rightmost field overwrites, and check exposed fields against generic contracts. — Thanks @rvcas!
- [743a09c](https://github.com/orbistry/alder/commit/743a09c81ed6f65f5bddd21f7ebd853cf8350e18) Require alternative match patterns to bind identical names and share canonical
  binding identities, with source-aware diagnostics for missing or extra names. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Resolve source-local type aliases in packaged standard-library function signatures
  and qualified public type aliases in caller annotations. Expose Fiber::MapOptions
  for traversal configuration. — Thanks @rvcas!
- [a0b9a41](https://github.com/orbistry/alder/commit/a0b9a41b148b86bf6f93567a56ed6d7c98227410) Register the documented Option constructors and lower Some/None construction
  and patterns through the existing nesting-preserving Option representation. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Retain and emit inherited default methods when implementing an imported trait,
  including async defaults and constrained dictionary factories. Publish accurate
  default-helper symbols and implementation method metadata, and invalidate stale
  interface caches. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Expose `Fiber.unbounded` as a Number value for explicit unbounded traversal
  concurrency, including typed built-in value lookup and the bundled runtime export. — Thanks @rvcas!
- [743a09c](https://github.com/orbistry/alder/commit/743a09c81ed6f65f5bddd21f7ebd853cf8350e18) Keep match-only pin patterns and constructor lookup confined to actual match
  patterns instead of leaking permission into nested bindings. — Thanks @rvcas!
- [edbcd63](https://github.com/orbistry/alder/commit/edbcd633652e4097acc18656bca8cd1b891a219a) Add Array.iter and independent ArrayIterator cursors, replacing the non-advancing
  Iterator instance on arrays. Preserve shared source values, live iteration, and
  correct Option payloads through exhaustion. — Thanks @rvcas!
- [d87e721](https://github.com/orbistry/alder/commit/d87e721ad6aa621a26853257257f3f5b2989d3e9) Carry error-row inclusion relationships in canonical and serialized annotations,
  preserving them through arena copies and interface hydration. Bump the interface
  format for the new scheme layout.
  
  Retain inclusion dependencies during solver generalization and instantiation,
  and reject inferred inclusions between independent universal error-row tails.
  
  Resolve concrete error unions for inferred results, including exhaustive matches
  across module boundaries. Preserve explicitly open result contracts and apply
  directional Result return checking consistently to named functions and lambdas. — Thanks @rvcas!
- [da17cab](https://github.com/orbistry/alder/commit/da17cab2397dca24da7124364a3c9a06c992c4d6) Support lowercase type variables in lambda annotations and share enclosing generic variables and trait bounds without leaking names between sibling scopes. — Thanks @rvcas!
- [f7fb26b](https://github.com/orbistry/alder/commit/f7fb26bab952eeb7c574638b5e237e82ebd817ad) Base top-level generalization on resolved assignments rather than mutation permission flags. Preserve safe polymorphism for never-assigned function bindings while restricting shared replaceable functions, including writes inside nested closures. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Add sparse exact-length tuple constraint metadata to annotations and stored interfaces, preserving arena copies and fingerprint identity with a new interface format. Carry imported constraints through inference and check concrete tuple lengths, constrained elements, and generic contracts.
  
  Finalize source tuple projections without allocating by the largest index, preserve fixed tuple lengths, and reject recursive tuple-element constraints. — Thanks @rvcas!
- Updated dependencies: alder-ast@0.3.0, alder-parse@0.3.0, alder-source@0.3.0

## 0.3.0 — 2026-09-04

### Minor changes

- [ac24445](https://github.com/orbistry/alder/commit/ac24445101bb7a8d5bef6076ff145b94e103c91a) Add row-typed `Result` errors, inferred and propagated error rows, exhaustive
  error matching, diagnostics, direct AST lowering, and typed JSON failures. — Thanks @rvcas!

## 0.2.1 — 2026-09-03

### Patch changes

- [c8fe6ce](https://github.com/orbistry/alder/commit/c8fe6ce6fdf364087ef57d077c4839049e044dff) Package the embedded first-party trait source inside `alder-can` so published
  crates build independently of the workspace layout. — Thanks @rvcas!

## 0.2.0 — 2026-09-03

### Minor changes

- [dceb9d4](https://github.com/orbistry/alder/commit/dceb9d4c5b00569226f38382733b48d31689367b) Generate compiler-backed Eq, Show, Ord, Hash, and Json implementations for error groups. — Thanks @rvcas!
- [ff0e0a3](https://github.com/orbistry/alder/commit/ff0e0a303fec4805bef7bfcb2ac002feb09d44e4) Add built-in `Show`, `Eq`, `Ord`, `Hash`, and `Json` enum derives with callable trait methods and direct Oxc AST dictionary lowering. — Thanks @rvcas!
- [6e721dc](https://github.com/orbistry/alder/commit/6e721dc86e8cc39fac1f4464eac6f1beea422803) Add the built-in `Iterator` trait, its associated `Item` type, and the initial Array implementation. — Thanks @rvcas!
- [7dab530](https://github.com/orbistry/alder/commit/7dab5303f81d1b4d07a9b9c43b6ea3bb6297d11a) Add the built-in `Traversable` trait and Array, Option, and Result implementations with method-level Applicative evidence. — Thanks @rvcas!
- [2b5848b](https://github.com/orbistry/alder/commit/2b5848b8d5b48c8ed3c954010e999a34567970e3) Add built-in `Applicative` and `Monad` traits and Array, Option, and Result implementations. — Thanks @rvcas!
- [7d53578](https://github.com/orbistry/alder/commit/7d53578e5aef1c152bda29fb55181c78fd9af45d) Implement the M2 core-language pipeline through direct Oxc AST generation,
  Rolldown bundling, the embedded standalone runtime, stdlib/kernel foundations,
  formatting, and test execution. — Thanks @rvcas!
- [5b56b86](https://github.com/orbistry/alder/commit/5b56b86556473042b72d8e2abc6b6471303c1172) Add the built-in `Functor` trait with kernel-backed `Array`, `Option`, and partially applied `Result` instances. — Thanks @rvcas!
- [4dc0e01](https://github.com/orbistry/alder/commit/4dc0e0118bd1ce68549d110f2499eb5f510739bb) Resolve associated-type equalities to stable trait identities, preserve them in inferred schemes and interfaces, and normalize projections through declared equalities and impl bindings. — Thanks @rvcas!

### Patch changes

- [725ee34](https://github.com/orbistry/alder/commit/725ee34ee4e9951913f73d5cc42ca17b542009e2) Load first-party trait and primitive/container instance headers from the
  audited Alder source module `std/Traits.ald`. Builtin instances now participate
  in ordinary database matching and prerequisite resolution before intrinsic
  code-generation evidence is selected. — Thanks @rvcas!
- [7757405](https://github.com/orbistry/alder/commit/7757405a85dbdd52f0a4d3109df65dfd2b34414a) Preserve implementation source locations across semantic interfaces and add validated package-instance-index persistence and hydration. — Thanks @rvcas!
- [6a6d7a4](https://github.com/orbistry/alder/commit/6a6d7a466e1bad50fdea3504857298d632f3bd9e) Implement the documented `Ord.compare -> Ordering` dictionary ABI. Generic
  comparison operators now inspect the tagged result, derived ordering composes
  selected field dictionaries through `compare`, and primitive comparisons keep
  their direct JavaScript lowering. — Thanks @rvcas!
- [a9be91f](https://github.com/orbistry/alder/commit/a9be91f26e5dc7b825c8d15ade9f13c12ebca8da) Add the explicit `Ref.same` identity operation selected by the trait equality
  design, backed by JavaScript reference equality and covered at runtime. — Thanks @rvcas!
- [7f62c2d](https://github.com/orbistry/alder/commit/7f62c2d71e83499bb88a66d0a93f03eb1bd70576) Expose `Eq` and `Ord` superclass dictionaries through built-in `Hash` and `Num`, and return 64-bit `BigInt` hashes. — Thanks @rvcas!
- [3af5769](https://github.com/orbistry/alder/commit/3af576961e371c3a993f4e6176c36725e6f471b0) Canonicalize package trait headers independently of value and method bodies,
  then compile every module against one frozen package-wide header closure.
  Coherence and sibling-instance behavior no longer depend on source order or on
  whether another module's body type-checks. — Thanks @rvcas!
- [f76286e](https://github.com/orbistry/alder/commit/f76286e87b755df2a337a02fb435368d6ddc92ae) Keep private local implementations in package headers while omitting them from published solved interfaces. — Thanks @rvcas!
- [cd06f1d](https://github.com/orbistry/alder/commit/cd06f1d67ecd65bb81c6f9ca1b1e4112d1db0792) Only require derive dictionaries for type parameters that occur in enum payload fields. — Thanks @rvcas!
- [7193977](https://github.com/orbistry/alder/commit/7193977e280cc2feb7d2beee160419ff3e552e9a) Reject orphan implementations during canonicalization while retaining the
  package-wide coherence check for imported metadata. — Thanks @rvcas!
- [630f635](https://github.com/orbistry/alder/commit/630f635691158c8def6ca564a4950b6df98eca57) Preserve optional enum payload fields through canonicalization and inference,
  omit them from derived JSON, and accept them as absent when decoding. Render
  derived record-payload constructors in Alder syntax and source field order.
  Derived hashes now include canonical type identity, declaration variant index,
  and every payload field. Derived JSON decoding rejects unexpected envelope and
  payload fields.
  Associated-type validation now rejects indirect projection cycles and reports
  the complete cycle through the structured diagnostic renderer. — Thanks @rvcas!
- Updated dependencies: alder-ast@0.2.0, alder-parse@0.2.0, alder-region@0.2.0, alder-source@0.2.0

