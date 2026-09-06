# alder-codegen

## 0.4.1 — 2026-09-06

### Patch changes

- Updated dependencies: alder-solve@0.5.0

## 0.4.0 — 2026-09-06

### Minor changes

- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Provide structural Json codecs for closed error rows with Json-capable payloads,
  independent of group names and declaration order. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Apply contextual recursive Option lifting to fresh record field initializers,
  including direct record return contexts, without converting existing mutable
  record aliases. Emit field wrapping through the centralized Option helpers. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Provide structural Show for closed error rows when every payload supports Show,
  independent of named error groups and their declaration order. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Treat optional record-field shorthand as an ordinary Option type and materialize omitted Option fields as None during contextual record construction.
  
  Remove separate record-field optionality from serialized interfaces and invalidate the previous interface format.
  
  Read Fiber traversal concurrency options using ordinary Option semantics so omitted and explicit None fields both select the sequential default.
  
  Preserve known generic Result error rows during propagation, including explicit Option fallback branches.
  
  Provide conditional Option ordering and Unit ordering so derived record payloads use ordinary field dictionaries, including nested Options. — Thanks @rvcas!
- [f87e540](https://github.com/orbistry/alder/commit/f87e540113feb6c1052b2d782a7ad6e1faa7d204) Add inferred lazy tasks, generator-based async lowering, Promise extern lifting,
  and a structured fiber runtime with interruption, scopes, finalizers, `all`, and
  `race`. — Thanks @rvcas!
- [0db756d](https://github.com/orbistry/alder/commit/0db756d7014df5c506fafe13df0b93306896d1fa) Represent explicit async function declarations and lazy async block syntax. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Resolve Hash structurally for closed error rows, honoring payload dictionaries
  and preserving hashes across equivalent groups and row widening. Share payload
  evidence with the equality superclass without duplicating nested generated code.
  Remove nominal error-group derives and their declaration-order-dependent behavior;
  error groups use conditional structural Eq, Show, Hash, and Json capabilities.
  Normalize derived enum payloads through the ordinary annotation converter so
  nominal wrappers of named error groups retain their checked structural evidence. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Infer contextual outer Option wrapping jointly across call arguments and emit
  the resolved Some layers directly through the centralized kernel representation.
  Report incompatible inference preferences with a source-aware diagnostic. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Allow omission of trailing Option arguments and emit explicit None values at
  direct, higher-order, and piped call sites using solver-provided call metadata. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Support Option propagation with postfix `?` in Option-returning contexts,
  including async bodies and pipe destinations, without implicit Result conversion.
  Wait for recursive peers to constrain unknown propagation carriers before
  generalization, preserving inferred Option contracts across module boundaries. — Thanks @rvcas!

### Patch changes

- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Capture reached pattern payloads before later pins can mutate their source,
  preserving checked binding types, array-rest values, and alias identity. — Thanks @rvcas!
- [1efea6c](https://github.com/orbistry/alder/commit/1efea6c0a882752faf882525f5f292e2b20a6360) Require Json evidence for module encode/decode calls and dispatch through the
  selected codec, including custom instances. Remove unchecked JSON entry points
  and fix public export names for imported direct dictionary calls. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Share lazy child dictionaries between container Hash and its Eq superclass,
  preventing exponential emitted-code growth while preserving equality dispatch. — Thanks @rvcas!
- [a0b9a41](https://github.com/orbistry/alder/commit/a0b9a41b148b86bf6f93567a56ed6d7c98227410) Check refutable patterns before binding payloads in lets, parameters, and loops.
  Failed bindings now report a source-located match failure instead of exposing
  values that violate their checked types, including inside lazy async tasks. — Thanks @rvcas!
- [f7fb26b](https://github.com/orbistry/alder/commit/f7fb26bab952eeb7c574638b5e237e82ebd817ad) Preserve left-to-right evaluation of array and tuple elements, call arguments,
  and tag payloads when later operands require setup statements. Evaluate earlier
  values before those statements without copying referenced mutable containers. — Thanks @rvcas!
- [54d1b63](https://github.com/orbistry/alder/commit/54d1b636097930bb115607e6b11b391954927339) Carry physical source origins alongside generated ASTs so local JavaScript extern modules resolve beside their Alder declarations. Preserve virtual module identities after AST transfer, order bundle inputs deterministically, and verify Promise fulfillment, foreign defects, and cancellation through local wrappers. — Thanks @rvcas!
- [ea6b284](https://github.com/orbistry/alder/commit/ea6b2849eee22f4332de8319392a1cbe60d7c5d7) Resolve embedded stdlib members through packaged Alder signatures, rejecting unknown members, wrong arguments, invalid callbacks, and arity mismatches. Remove untyped builtin references and correct expected-versus-actual call diagnostics. — Thanks @rvcas!
- [6010a5e](https://github.com/orbistry/alder/commit/6010a5e41f8b9b0ca691ec29d6813d1b57c1b3d9) Reject executable state expressions and component declarations until their M6
  runtime semantics exist, rather than silently emitting identity expressions and
  ordinary functions. Preserve provisional check-only support. — Thanks @rvcas!
- [b52e7ad](https://github.com/orbistry/alder/commit/b52e7ad05c1edca946b84275e35ed066a595e653) Consume hidden trait dictionaries in extern adapters without leaking them into
  foreign JavaScript arguments, including Result and abort-aware Task wrappers. — Thanks @rvcas!
- [b8924f2](https://github.com/orbistry/alder/commit/b8924f26405306239a1e33b87592a1e61eeb3fcc) Make equality inherited through primitive Hash dictionaries agree with ordinary
  equality, including signed zero and NaN through nested containers and derives. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Resolve recursive derived dictionary references through nested container and
  structural evidence, preventing undefined self bindings in emitted equality,
  showing, and hashing functions. — Thanks @rvcas!
- [d6cff55](https://github.com/orbistry/alder/commit/d6cff55e79663588e5433f6c85ec384ebad1b741) Preserve explicit module initialization dependencies when references resolve through re-exports to their original definitions. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Preserve contextual field types inside fresh Option-wrapped record and array
  payloads. Apply ordinary checked-call rules to bare pipe destinations, including
  Option lifting, omitted arguments, and dictionary passing. — Thanks @rvcas!
- [12b09ad](https://github.com/orbistry/alder/commit/12b09ad1f41251491b7a548e9d6f1e16ad320bd4) Validate primitive Json trait decoding against the requested type, preserving
  validation inside derived and container codecs. Encode unit as null and BigInt
  as a decimal JSON string; reject non-finite JSON numbers instead of silently
  encoding them as null. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Avoid exponential generated-code growth for nested JSON codecs by sharing lazy
  payload descriptors between encoding and decoding, preserving recursive codecs. — Thanks @rvcas!
- [41f55b6](https://github.com/orbistry/alder/commit/41f55b6201ca3abebac3fafd4e88ff0031d627a7) Retain Alder source and extern declaration regions through bundling, render unresolved externs as labeled shared diagnostics, and verify sibling JavaScript wrapper resolution across path-dependency packages. — Thanks @rvcas!
- [677e20f](https://github.com/orbistry/alder/commit/677e20fd158941294eb7688d34180b68cda0e17e) Preserve record-tail identities during inference and publication, check ordinary
  field types at value boundaries, and read stored Option fields directly. — Thanks @rvcas!
- [c9bb0b6](https://github.com/orbistry/alder/commit/c9bb0b6fa425b1b26c5d5c21f2705129ec0e9c34) Fix Option coalescing to return its payload type, preserve unit and nested
  Options, and evaluate the default only for None. — Thanks @rvcas!
- [13079e7](https://github.com/orbistry/alder/commit/13079e73b1ea0756df0323a7bbabcb9fe8841967) Read stored Option fields directly in record destructuring and record-shaped
  enum patterns, preserving nested Option values without presence-based wrapping. — Thanks @rvcas!
- [138d0d0](https://github.com/orbistry/alder/commit/138d0d0f10b38f2d6065ef790a5a46c61b584950) Distinguish normal fallthrough from control-flow exits, reject missing function results after zero-iteration loops, accept returning branches and lambdas without artificial unit values, and preserve loop-tail and break-payload effects in generated JavaScript. — Thanks @rvcas!
- [a0b9a41](https://github.com/orbistry/alder/commit/a0b9a41b148b86bf6f93567a56ed6d7c98227410) Register the documented Option constructors and lower Some/None construction
  and patterns through the existing nesting-preserving Option representation. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Retain and emit inherited default methods when implementing an imported trait,
  including async defaults and constrained dictionary factories. Publish accurate
  default-helper symbols and implementation method metadata, and invalidate stale
  interface caches. — Thanks @rvcas!
- [3662065](https://github.com/orbistry/alder/commit/366206512a719b79ba48f2ccdf846af928495838) Reject query expressions during executable code generation with source context
  instead of emitting a runtime stub that always throws. Query execution remains
  deferred to M7; check-only handling is unchanged. — Thanks @rvcas!
- [5932a9c](https://github.com/orbistry/alder/commit/5932a9c1a420fd95e6257080683523d4a0cec8ca) Include runtime tag prefixes in structural error-row equality metadata so
  errors with the same tag but different payloads no longer compare equal. — Thanks @rvcas!
- [edbcd63](https://github.com/orbistry/alder/commit/edbcd633652e4097acc18656bca8cd1b891a219a) Add Array.iter and independent ArrayIterator cursors, replacing the non-advancing
  Iterator instance on arrays. Preserve shared source values, live iteration, and
  correct Option payloads through exhaustion. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Initialize trait dictionaries in superclass dependency order before top-level
  values, fixing references to later-declared or derived superclass dictionaries. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Track active nominal comparison pairs in derived equality so recursive values
  can compare cyclic payloads without repeatedly following the same cycle.
  Preserve payload dictionary checks, NaN inequality, and cleanup after failures. — Thanks @rvcas!
- [f7fb26b](https://github.com/orbistry/alder/commit/f7fb26bab952eeb7c574638b5e237e82ebd817ad) Preserve template interpolation evaluation and string-conversion order across
  later setup statements. Check tagged templates against the actual tag function,
  including argument types and its return type, instead of assuming String.
  Preserve empty tagged-template segments around adjacent interpolations. — Thanks @rvcas!
- [f7fb26b](https://github.com/orbistry/alder/commit/f7fb26bab952eeb7c574638b5e237e82ebd817ad) Evaluate assignment targets before right-hand setup statements, preserving the
  original receiver across mutation. Support indexed targets with setup statements
  and capture compound-assignment values before right-hand effects, including
  dictionary-dispatched arithmetic. — Thanks @rvcas!
- [a0b9a41](https://github.com/orbistry/alder/commit/a0b9a41b148b86bf6f93567a56ed6d7c98227410) Short-circuit nested pin effects after failed enclosing pattern checks,
  including pins that suspend while awaiting a task. — Thanks @rvcas!
- [05015b9](https://github.com/orbistry/alder/commit/05015b90da5f97cf8c133550281c993d5c88d0e1) Reject executable markup and styles until the rendering/CSS backends exist.
  Remove placeholder object lowering that silently discarded markup directives;
  preserve provisional check-only support with source-aware build diagnostics. — Thanks @rvcas!
- [a9f1d44](https://github.com/orbistry/alder/commit/a9f1d4450bc4b377bdb1e062790f53192a4994a8) Preserve lexical break and continue targets when while-condition setup is
  lowered inside a generated JavaScript loop, including suspended conditions. — Thanks @rvcas!
- [f7fb26b](https://github.com/orbistry/alder/commit/f7fb26bab952eeb7c574638b5e237e82ebd817ad) Preserve record field evaluation and spread copy order when later operands require setup statements. — Thanks @rvcas!
- Updated dependencies: alder-ast@0.3.0, alder-solve@0.4.0

## 0.3.0 — 2026-09-04

### Minor changes

- [ac24445](https://github.com/orbistry/alder/commit/ac24445101bb7a8d5bef6076ff145b94e103c91a) Add row-typed `Result` errors, inferred and propagated error rows, exhaustive
  error matching, diagnostics, direct AST lowering, and typed JSON failures. — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-solve@0.3.0

## 0.2.1 — 2026-09-03

### Patch changes

- Updated dependencies: alder-can@0.2.1, alder-constrain@0.2.1, alder-solve@0.2.1

## 0.2.0 — 2026-09-03

### Minor changes

- [ff0e0a3](https://github.com/orbistry/alder/commit/ff0e0a303fec4805bef7bfcb2ac002feb09d44e4) Add built-in `Show`, `Eq`, `Ord`, `Hash`, and `Json` enum derives with callable trait methods and direct Oxc AST dictionary lowering. — Thanks @rvcas!
- [6e721dc](https://github.com/orbistry/alder/commit/6e721dc86e8cc39fac1f4464eac6f1beea422803) Add the built-in `Iterator` trait, its associated `Item` type, and the initial Array implementation. — Thanks @rvcas!
- [0258ff1](https://github.com/orbistry/alder/commit/0258ff1fef3e279249933be2a3c8e149ad28afcf) Adopt arrow lambdas and juxtaposed function return types, forward piped values
  to the first argument of existing calls, and add `Array.filter` for pipeline
  composition. — Thanks @rvcas!
- [9939d2b](https://github.com/orbistry/alder/commit/9939d2b638073809ba463d7a145f84e33f96a93e) Add kernel-backed `Show`, `Hash`, and `Json` instances for primitives and built-in containers. — Thanks @rvcas!
- [7dab530](https://github.com/orbistry/alder/commit/7dab5303f81d1b4d07a9b9c43b6ea3bb6297d11a) Add the built-in `Traversable` trait and Array, Option, and Result implementations with method-level Applicative evidence. — Thanks @rvcas!
- [2b5848b](https://github.com/orbistry/alder/commit/2b5848b8d5b48c8ed3c954010e999a34567970e3) Add built-in `Applicative` and `Monad` traits and Array, Option, and Result implementations. — Thanks @rvcas!
- [7d53578](https://github.com/orbistry/alder/commit/7d53578e5aef1c152bda29fb55181c78fd9af45d) Implement the M2 core-language pipeline through direct Oxc AST generation,
  Rolldown bundling, the embedded standalone runtime, stdlib/kernel foundations,
  formatting, and test execution. — Thanks @rvcas!
- [51c443a](https://github.com/orbistry/alder/commit/51c443ae6d3ac718b0f9b955f84ef33f91b8a3f2) Carry and lower transitive superclass dictionary paths for generic bounds and implementation prerequisites. — Thanks @rvcas!
- [289710f](https://github.com/orbistry/alder/commit/289710fa67480fe9b425c38573dfccb09b6690d0) Lower pin patterns and compound assignments through their solved `Eq` and `Num` dictionaries while preserving single evaluation of indexed assignment targets. — Thanks @rvcas!
- [5b56b86](https://github.com/orbistry/alder/commit/5b56b86556473042b72d8e2abc6b6471303c1172) Add the built-in `Functor` trait with kernel-backed `Array`, `Option`, and partially applied `Result` instances. — Thanks @rvcas!
- [730c9f3](https://github.com/orbistry/alder/commit/730c9f38d0c06e27dcf4f1084783171b62a25cf7) Validate trait implementation superclasses and emit their resolved dictionary fields. — Thanks @rvcas!
- [0865782](https://github.com/orbistry/alder/commit/08657827feb72149803f79e55485920a413e259a) Capture resolved trait dictionaries when constrained functions or trait methods are used as first-class values. — Thanks @rvcas!

### Patch changes

- [dceb9d4](https://github.com/orbistry/alder/commit/dceb9d4c5b00569226f38382733b48d31689367b) Generate compiler-backed Eq, Show, Ord, Hash, and Json implementations for error groups. — Thanks @rvcas!
- [453c72d](https://github.com/orbistry/alder/commit/453c72dc916563c7c50154af8226def856b5fa47) Order derived enum values by declaration position before comparing payloads. — Thanks @rvcas!
- [6a6d7a4](https://github.com/orbistry/alder/commit/6a6d7a466e1bad50fdea3504857298d632f3bd9e) Implement the documented `Ord.compare -> Ordering` dictionary ABI. Generic
  comparison operators now inspect the tagged result, derived ordering composes
  selected field dictionaries through `compare`, and primitive comparisons keep
  their direct JavaScript lowering. — Thanks @rvcas!
- [794af50](https://github.com/orbistry/alder/commit/794af5063f5133c39e68ecdb18f34e9b493258fc) Encode and decode derived enums and error groups through their documented tagged JSON shape. — Thanks @rvcas!
- [e1d71c8](https://github.com/orbistry/alder/commit/e1d71c81b96d5afc8b473cf5e7ab0aaf3ed152f0) Resolve and retain trait evidence for every derived payload field, including
  nested builtin containers. Generated Show, Eq, Ord, Hash, and Json dictionaries
  now dispatch through the selected field dictionaries, and dictionary emission
  orders Eq superclasses before their dependents. — Thanks @rvcas!
- [b699950](https://github.com/orbistry/alder/commit/b699950207423f198107d2872dabf690d84235b3) Add source-aware miette diagnostics for compiler errors and warnings, render
  trait and parser failures with labeled snippets, and preserve Alder source in
  code generation snapshots. — Thanks @rvcas!
- [7f62c2d](https://github.com/orbistry/alder/commit/7f62c2d71e83499bb88a66d0a93f03eb1bd70576) Expose `Eq` and `Ord` superclass dictionaries through built-in `Hash` and `Num`, and return 64-bit `BigInt` hashes. — Thanks @rvcas!
- [87b3bee](https://github.com/orbistry/alder/commit/87b3bee468b1167410594b9db194d4bf8cced8ac) Match generated error-group derive dictionaries against the runtime's
  colon-prefixed tag representation, and make recursive derived dictionaries
  refer to their emitted binding instead of a factory-only local. — Thanks @rvcas!
- [630f635](https://github.com/orbistry/alder/commit/630f635691158c8def6ca564a4950b6df98eca57) Preserve optional enum payload fields through canonicalization and inference,
  omit them from derived JSON, and accept them as absent when decoding. Render
  derived record-payload constructors in Alder syntax and source field order.
  Derived hashes now include canonical type identity, declaration variant index,
  and every payload field. Derived JSON decoding rejects unexpected envelope and
  payload fields.
  Associated-type validation now rejects indirect projection cycles and reports
  the complete cycle through the structured diagnostic renderer. — Thanks @rvcas!
- Updated dependencies: alder-ast@0.2.0, alder-can@0.2.0, alder-constrain@0.2.0, alder-parse@0.2.0, alder-region@0.2.0, alder-solve@0.2.0

