# alder-solve

## 0.4.0 — 2026-09-04

### Minor changes

- [f87e540](https://github.com/orbistry/alder/commit/f87e540113feb6c1052b2d782a7ad6e1faa7d204) Add inferred lazy tasks, generator-based async lowering, Promise extern lifting,
  and a structured fiber runtime with interruption, scopes, finalizers, `all`, and
  `race`. — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-ast@0.3.0, alder-can@0.4.0, alder-constrain@0.3.1

## 0.3.0 — 2026-09-04

### Minor changes

- [ac24445](https://github.com/orbistry/alder/commit/ac24445101bb7a8d5bef6076ff145b94e103c91a) Add row-typed `Result` errors, inferred and propagated error rows, exhaustive
  error matching, diagnostics, direct AST lowering, and typed JSON failures. — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-can@0.3.0, alder-constrain@0.3.0

## 0.2.1 — 2026-09-03

### Patch changes

- Updated dependencies: alder-can@0.2.1, alder-constrain@0.2.1

## 0.2.0 — 2026-09-03

### Minor changes

- [ff0e0a3](https://github.com/orbistry/alder/commit/ff0e0a303fec4805bef7bfcb2ac002feb09d44e4) Add built-in `Show`, `Eq`, `Ord`, `Hash`, and `Json` enum derives with callable trait methods and direct Oxc AST dictionary lowering. — Thanks @rvcas!
- [36492e6](https://github.com/orbistry/alder/commit/36492e624fd3075fef935ac54d3ab5209fae18a3) Reject trait implementation heads whose constructor kind does not match the trait parameter. — Thanks @rvcas!
- [6e721dc](https://github.com/orbistry/alder/commit/6e721dc86e8cc39fac1f4464eac6f1beea422803) Add the built-in `Iterator` trait, its associated `Item` type, and the initial Array implementation. — Thanks @rvcas!
- [0258ff1](https://github.com/orbistry/alder/commit/0258ff1fef3e279249933be2a3c8e149ad28afcf) Adopt arrow lambdas and juxtaposed function return types, forward piped values
  to the first argument of existing calls, and add `Array.filter` for pipeline
  composition. — Thanks @rvcas!
- [9939d2b](https://github.com/orbistry/alder/commit/9939d2b638073809ba463d7a145f84e33f96a93e) Add kernel-backed `Show`, `Hash`, and `Json` instances for primitives and built-in containers. — Thanks @rvcas!
- [b664174](https://github.com/orbistry/alder/commit/b6641741292b8240922ad7c1ae8822e0edfc16b1) Preseed declared type signatures and trait predicates for every value SCC so mutually recursive calls pass stable dictionary arguments independent of source order. — Thanks @rvcas!
- [7dab530](https://github.com/orbistry/alder/commit/7dab5303f81d1b4d07a9b9c43b6ea3bb6297d11a) Add the built-in `Traversable` trait and Array, Option, and Result implementations with method-level Applicative evidence. — Thanks @rvcas!
- [2b5848b](https://github.com/orbistry/alder/commit/2b5848b8d5b48c8ed3c954010e999a34567970e3) Add built-in `Applicative` and `Monad` traits and Array, Option, and Result implementations. — Thanks @rvcas!
- [7d53578](https://github.com/orbistry/alder/commit/7d53578e5aef1c152bda29fb55181c78fd9af45d) Implement the M2 core-language pipeline through direct Oxc AST generation,
  Rolldown bundling, the embedded standalone runtime, stdlib/kernel foundations,
  formatting, and test execution. — Thanks @rvcas!
- [e45ef4a](https://github.com/orbistry/alder/commit/e45ef4aee1ee5c8dceceb90e9b29ac586394c3a5) Provide automatic structural equality for arrays, options, and results whose type arguments implement `Eq`. — Thanks @rvcas!
- [51c443a](https://github.com/orbistry/alder/commit/51c443ae6d3ac718b0f9b955f84ef33f91b8a3f2) Carry and lower transitive superclass dictionary paths for generic bounds and implementation prerequisites. — Thanks @rvcas!
- [70ab929](https://github.com/orbistry/alder/commit/70ab9292acb31029b43046e525bc9abaaa705e75) Load imported dependency package instance indexes into package-aware builds and deduplicate their implementations by stable identity. — Thanks @rvcas!
- [7f62c2d](https://github.com/orbistry/alder/commit/7f62c2d71e83499bb88a66d0a93f03eb1bd70576) Expose `Eq` and `Ord` superclass dictionaries through built-in `Hash` and `Num`, and return 64-bit `BigInt` hashes. — Thanks @rvcas!
- [5b56b86](https://github.com/orbistry/alder/commit/5b56b86556473042b72d8e2abc6b6471303c1172) Add the built-in `Functor` trait with kernel-backed `Array`, `Option`, and partially applied `Result` instances. — Thanks @rvcas!
- [730c9f3](https://github.com/orbistry/alder/commit/730c9f38d0c06e27dcf4f1084783171b62a25cf7) Validate trait implementation superclasses and emit their resolved dictionary fields. — Thanks @rvcas!
- [4dc0e01](https://github.com/orbistry/alder/commit/4dc0e0118bd1ce68549d110f2499eb5f510739bb) Resolve associated-type equalities to stable trait identities, preserve them in inferred schemes and interfaces, and normalize projections through declared equalities and impl bindings. — Thanks @rvcas!

### Patch changes

- [df4e6ab](https://github.com/orbistry/alder/commit/df4e6ab1176da2149c39bb847d9f8aad597d4e21) Preserve source type-variable names in trait failures and add source-aware
  rendering for unsatisfied bounds, orphan impls, overlaps, kind mismatches, and
  associated-type cycles. Render canonical trait member, associated-type, derive,
  and duplicate errors with precise source labels as well. — Thanks @rvcas!
- [77dda67](https://github.com/orbistry/alder/commit/77dda671c2140f36af781535ecf1f0b289dad92e) Keep mutable and local bindings monomorphic, and subtract type variables held
  by the outer environment when generalizing top-level values. Report unresolved
  trait obligations over non-generalized variables as type ambiguity rather than
  suggesting an impossible generic bound. — Thanks @rvcas!
- [725ee34](https://github.com/orbistry/alder/commit/725ee34ee4e9951913f73d5cc42ca17b542009e2) Load first-party trait and primitive/container instance headers from the
  audited Alder source module `std/Traits.ald`. Builtin instances now participate
  in ordinary database matching and prerequisite resolution before intrinsic
  code-generation evidence is selected. — Thanks @rvcas!
- [838db93](https://github.com/orbistry/alder/commit/838db931669daf8fa6c4ff95e17e964446cd6076) Report contradictory associated-type equalities as a dedicated structured mismatch. — Thanks @rvcas!
- [6a6d7a4](https://github.com/orbistry/alder/commit/6a6d7a466e1bad50fdea3504857298d632f3bd9e) Implement the documented `Ord.compare -> Ordering` dictionary ABI. Generic
  comparison operators now inspect the tagged result, derived ordering composes
  selected field dictionaries through `compare`, and primitive comparisons keep
  their direct JavaScript lowering. — Thanks @rvcas!
- [e1d71c8](https://github.com/orbistry/alder/commit/e1d71c81b96d5afc8b473cf5e7ab0aaf3ed152f0) Resolve and retain trait evidence for every derived payload field, including
  nested builtin containers. Generated Show, Eq, Ord, Hash, and Json dictionaries
  now dispatch through the selected field dictionaries, and dictionary emission
  orders Eq superclasses before their dependents. — Thanks @rvcas!
- [794af50](https://github.com/orbistry/alder/commit/794af5063f5133c39e68ecdb18f34e9b493258fc) Infer record-payload enum constructors as their enum result type while checking each payload field. — Thanks @rvcas!
- [916932b](https://github.com/orbistry/alder/commit/916932b78abe4ac1c414697417d5f5384ec37361) Reject cyclic associated-type bindings with a structured projection-cycle error. — Thanks @rvcas!
- [b699950](https://github.com/orbistry/alder/commit/b699950207423f198107d2872dabf690d84235b3) Add source-aware miette diagnostics for compiler errors and warnings, render
  trait and parser failures with labeled snippets, and preserve Alder source in
  code generation snapshots. — Thanks @rvcas!
- [329bdbe](https://github.com/orbistry/alder/commit/329bdbea92a1ae1f3d4d7669a184c8b1ebfb9009) Make solver evidence obligations consume the stable requirement seeds produced by constraint generation. — Thanks @rvcas!
- [6bd5000](https://github.com/orbistry/alder/commit/6bd500018bfd701ee1d21c864c4534c6c2824bad) Render trait and inference failures as stable user-facing diagnostics with actionable bound guidance. — Thanks @rvcas!
- [11b51d9](https://github.com/orbistry/alder/commit/11b51d983968415374018be19fafd9803f58f774) Retain nested trait obligation chains and render ambiguous implementation
  candidates with all available source locations. — Thanks @rvcas!
- [630f635](https://github.com/orbistry/alder/commit/630f635691158c8def6ca564a4950b6df98eca57) Preserve optional enum payload fields through canonicalization and inference,
  omit them from derived JSON, and accept them as absent when decoding. Render
  derived record-payload constructors in Alder syntax and source field order.
  Derived hashes now include canonical type identity, declaration variant index,
  and every payload field. Derived JSON decoding rejects unexpected envelope and
  payload fields.
  Associated-type validation now rejects indirect projection cycles and reports
  the complete cycle through the structured diagnostic renderer. — Thanks @rvcas!
- Updated dependencies: alder-ast@0.2.0, alder-can@0.2.0, alder-constrain@0.2.0, alder-parse@0.2.0, alder-region@0.2.0, alder-source@0.2.0

