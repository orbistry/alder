# alder-constrain

## 0.4.0 — 2026-09-06

### Minor changes

- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Infer contextual outer Option wrapping jointly across call arguments and emit
  the resolved Some layers directly through the centralized kernel representation.
  Report incompatible inference preferences with a source-aware diagnostic. — Thanks @rvcas!

### Patch changes

- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Report out-of-range tuple reads and writes at the index with the tuple length
  and valid zero-based bounds instead of a misleading unit-type mismatch. — Thanks @rvcas!
- [0304ee5](https://github.com/orbistry/alder/commit/0304ee5bf8bf0a6525a371556a86d5be161a54e4) Reject specialization and escape of declared generic function and trait method
  contracts, with source-aware diagnostics for invalid implementations. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Reject ordinary types in converted Result error annotations even when the
  function only forwards its argument without constructing or matching a Result.
  Validate unused alias and enum payload declarations too, and report invalid
  error arguments with a source-labeled error-row diagnostic.
  Check bodyless trait signatures, associated-type bindings, and error-group
  payloads without requiring a use site.
  Validate fixed error slots in partial Result constructors and match structural
  error rows when resolving higher-kinded trait implementations. — Thanks @rvcas!
- [0db756d](https://github.com/orbistry/alder/commit/0db756d7014df5c506fafe13df0b93306896d1fa) Represent explicit async function declarations and lazy async block syntax. — Thanks @rvcas!
- [138d0d0](https://github.com/orbistry/alder/commit/138d0d0f10b38f2d6065ef790a5a46c61b584950) Distinguish normal fallthrough from control-flow exits, reject missing function results after zero-iteration loops, accept returning branches and lambdas without artificial unit values, and preserve loop-tail and break-payload effects in generated JavaScript. — Thanks @rvcas!
- [a708527](https://github.com/orbistry/alder/commit/a70852762e00f2168b7cb3462ad722cc4393efe1) Keep shared top-level state monomorphic, preserve safe factory polymorphism, and prevent interfaces from turning unresolved shared types into independent generics. Diagnose incomplete shared export types. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Detect cyclic structural error-group expansion and report the recursive source
  reference instead of aborting the compiler with a stack overflow. — Thanks @rvcas!
- Updated dependencies: alder-ast@0.3.0

## 0.3.0 — 2026-09-04

### Minor changes

- [ac24445](https://github.com/orbistry/alder/commit/ac24445101bb7a8d5bef6076ff145b94e103c91a) Add row-typed `Result` errors, inferred and propagated error rows, exhaustive
  error matching, diagnostics, direct AST lowering, and typed JSON failures. — Thanks @rvcas!

## 0.2.1 — 2026-09-03

### Patch changes

- Updated dependencies: alder-can@0.2.1

## 0.2.0 — 2026-09-03

### Minor changes

- [7d53578](https://github.com/orbistry/alder/commit/7d53578e5aef1c152bda29fb55181c78fd9af45d) Implement the M2 core-language pipeline through direct Oxc AST generation,
  Rolldown bundling, the embedded standalone runtime, stdlib/kernel foundations,
  formatting, and test execution. — Thanks @rvcas!

### Patch changes

- [838db93](https://github.com/orbistry/alder/commit/838db931669daf8fa6c4ff95e17e964446cd6076) Report contradictory associated-type equalities as a dedicated structured mismatch. — Thanks @rvcas!
- Updated dependencies: alder-ast@0.2.0, alder-can@0.2.0, alder-parse@0.2.0, alder-region@0.2.0

