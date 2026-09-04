# alder-driver

## 0.4.0 — 2026-09-04

### Minor changes

- [f87e540](https://github.com/orbistry/alder/commit/f87e540113feb6c1052b2d782a7ad6e1faa7d204) Add inferred lazy tasks, generator-based async lowering, Promise extern lifting,
  and a structured fiber runtime with interruption, scopes, finalizers, `all`, and
  `race`. — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-ast@0.3.0, alder-can@0.4.0, alder-codegen@0.4.0, alder-constrain@0.3.1, alder-solve@0.4.0

## 0.3.0 — 2026-09-04

### Minor changes

- [ac24445](https://github.com/orbistry/alder/commit/ac24445101bb7a8d5bef6076ff145b94e103c91a) Add row-typed `Result` errors, inferred and propagated error rows, exhaustive
  error matching, diagnostics, direct AST lowering, and typed JSON failures. — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-can@0.3.0, alder-codegen@0.3.0, alder-constrain@0.3.0, alder-solve@0.3.0

## 0.2.1 — 2026-09-03

### Patch changes

- Updated dependencies: alder-can@0.2.1, alder-codegen@0.2.1, alder-constrain@0.2.1, alder-solve@0.2.1

## 0.2.0 — 2026-09-03

### Minor changes

- [7757405](https://github.com/orbistry/alder/commit/7757405a85dbdd52f0a4d3109df65dfd2b34414a) Preserve implementation source locations across semantic interfaces and add validated package-instance-index persistence and hydration. — Thanks @rvcas!
- [2278da9](https://github.com/orbistry/alder/commit/2278da9f8e58a1c12f6bfdf0723af001c669fc41) Produce and persist solved module interfaces and deduplicated package instance indexes after successful builds. — Thanks @rvcas!
- [28c93de](https://github.com/orbistry/alder/commit/28c93de0f460222eb04001942292c875f7329dfb) Replace the legacy export-name cache with versioned semantic interface and
  package-instance-index files. Preserve complete trait, associated-type,
  dictionary, and public type metadata, validate hydration round trips, and use
  canonical SHA-256 fingerprints. — Thanks @rvcas!
- [7d53578](https://github.com/orbistry/alder/commit/7d53578e5aef1c152bda29fb55181c78fd9af45d) Implement the M2 core-language pipeline through direct Oxc AST generation,
  Rolldown bundling, the embedded standalone runtime, stdlib/kernel foundations,
  formatting, and test execution. — Thanks @rvcas!
- [b699950](https://github.com/orbistry/alder/commit/b699950207423f198107d2872dabf690d84235b3) Add source-aware miette diagnostics for compiler errors and warnings, render
  trait and parser failures with labeled snippets, and preserve Alder source in
  code generation snapshots. — Thanks @rvcas!
- [9789f23](https://github.com/orbistry/alder/commit/9789f237bd68714c67bffafa7e8836fc9a731a7f) Compile imported path-dependency sources into the same in-memory Oxc/Rolldown
  module graph, allowing dictionary factories selected from unimported sibling
  modules to bundle and execute without serializing generated JavaScript. — Thanks @rvcas!
- [70ab929](https://github.com/orbistry/alder/commit/70ab9292acb31029b43046e525bc9abaaa705e75) Load imported dependency package instance indexes into package-aware builds and deduplicate their implementations by stable identity. — Thanks @rvcas!

### Patch changes

- [b49583d](https://github.com/orbistry/alder/commit/b49583dbcb4896846de69824e3a235df6ec77315) Deep-copy solved interfaces across arena boundaries so every source module can
  release its parser, canonical, constraint, and solver allocations immediately
  after compilation. — Thanks @rvcas!
- [df4e6ab](https://github.com/orbistry/alder/commit/df4e6ab1176da2149c39bb847d9f8aad597d4e21) Preserve source type-variable names in trait failures and add source-aware
  rendering for unsatisfied bounds, orphan impls, overlaps, kind mismatches, and
  associated-type cycles. Render canonical trait member, associated-type, derive,
  and duplicate errors with precise source labels as well. — Thanks @rvcas!
- [77dda67](https://github.com/orbistry/alder/commit/77dda671c2140f36af781535ecf1f0b289dad92e) Keep mutable and local bindings monomorphic, and subtract type variables held
  by the outer environment when generalizing top-level values. Report unresolved
  trait obligations over non-generalized variables as type ambiguity rather than
  suggesting an impossible generic bound. — Thanks @rvcas!
- [70ce3d4](https://github.com/orbistry/alder/commit/70ce3d413172bbfaf11865e1bef9e6cad2450d6c) Retry module solving as sibling interfaces become available so package instances do not depend on graph-level build order. — Thanks @rvcas!
- [c52cdaf](https://github.com/orbistry/alder/commit/c52cdafeac8e9823dd1aa71b5883b8bad8997cbd) Render trait and impl signature parser failures from their innermost function,
  parameter, and type errors with syntax-specific miette labels and help. — Thanks @rvcas!
- [9f2343d](https://github.com/orbistry/alder/commit/9f2343d6d372b0d798d3b30e5f408d5beb8d81f3) Render superclass, invalid-termination, and defensive instance-cycle diagnostics
  with source snippets, and point local superclass cycles at the closing trait. — Thanks @rvcas!
- [3af5769](https://github.com/orbistry/alder/commit/3af576961e371c3a993f4e6176c36725e6f471b0) Canonicalize package trait headers independently of value and method bodies,
  then compile every module against one frozen package-wide header closure.
  Coherence and sibling-instance behavior no longer depend on source order or on
  whether another module's body type-checks. — Thanks @rvcas!
- [6bd5000](https://github.com/orbistry/alder/commit/6bd500018bfd701ee1d21c864c4534c6c2824bad) Render trait and inference failures as stable user-facing diagnostics with actionable bound guidance. — Thanks @rvcas!
- [11b51d9](https://github.com/orbistry/alder/commit/11b51d983968415374018be19fafd9803f58f774) Retain nested trait obligation chains and render ambiguous implementation
  candidates with all available source locations. — Thanks @rvcas!
- [7193977](https://github.com/orbistry/alder/commit/7193977e280cc2feb7d2beee160419ff3e552e9a) Reject orphan implementations during canonicalization while retaining the
  package-wide coherence check for imported metadata. — Thanks @rvcas!
- Updated dependencies: alder-ast@0.2.0, alder-can@0.2.0, alder-codegen@0.2.0, alder-config@0.2.0, alder-constrain@0.2.0, alder-parse@0.2.0, alder-region@0.2.0, alder-report@0.2.0, alder-solve@0.2.0, alder-source@0.2.0

