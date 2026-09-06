# alder-constrain

## 0.5.0 — 2026-09-06

### Minor changes

- [8cb8b18](https://github.com/orbistry/alder/commit/8cb8b181878b325ebd12585d03f7b0172d8c7ef1) Check pattern exhaustiveness and redundancy uniformly across enums, Option,
  Result, nested payloads, tuples, records and array prefixes. Reject refutable
  bindings and parameters at compile time, report uncovered patterns and covering
  source locations, and preserve effectful guard and pin behavior. — Thanks @rvcas!
- [8dc960c](https://github.com/orbistry/alder/commit/8dc960c1cfd21d9fce64470714dae904d8f2d2b8) Retain available record fields in missing-field diagnostics and offer conservative, unambiguous typo suggestions. — Thanks @rvcas!
- [30635be](https://github.com/orbistry/alder/commit/30635be9554d9419a6fae026b0a01f88be6726b2) Retain structured recursive type equations for occurs-check failures and explain
  why structural cycles cannot have finite types. Use shared readable variable
  names and preserve call, branch and array-element diagnostic context. — Thanks @rvcas!
- [ea12495](https://github.com/orbistry/alder/commit/ea1249529f95314799f5241bc725bc9d7541a65d) Preserve owned nominal identities in diagnostic types and render consumer import names, re-export aliases, or explicit module/package provenance instead of ambiguous short names. — Thanks @rvcas!
- [db5af22](https://github.com/orbistry/alder/commit/db5af22dcae75be7c815d00a3a329fb2a5578d48) Preserve structured mismatch types until diagnostic rendering, including distinct generic variables and open record/error rows. — Thanks @rvcas!
- [5567bf1](https://github.com/orbistry/alder/commit/5567bf17dd7b016c2cc1f6b108f5db863f887a64) Retain structured generic restrictions and explain concrete specialization, independent-variable equality, open-row restrictions, and shared-storage escape without speculative signature changes. — Thanks @rvcas!
- [b0bb65b](https://github.com/orbistry/alder/commit/b0bb65bf4f36b5fde164fe8b8ae0f6cb6472db1d) Preserve structured record comparisons and distinguish missing from unexpected fields, including conservative typo suggestions and enclosing nested-record shapes. — Thanks @rvcas!
- [875607f](https://github.com/orbistry/alder/commit/875607f62375e37441f8a9522e97b8955fa443ef) Explain immediate type expectations with contextual source labels and originating annotations, and report call arity with argument counts. — Thanks @rvcas!

### Patch changes

- [52732c2](https://github.com/orbistry/alder/commit/52732c2173f38376da3372582de0347b8105a3ac) Preserve structured types in missing-return, invalid Result error-kind, and associated-equality diagnostics so nested types use resolved import names at reporting time. — Thanks @rvcas!
- [00451d1](https://github.com/orbistry/alder/commit/00451d103570fbec7a31237cb86d2fbc01a0b1d0) Label the earlier local requirement when associated-type equalities conflict. — Thanks @rvcas!

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

