# alder-can

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

