# alder-cli

## 0.2.2 — 2026-09-04

### Patch changes

- [76d003f](https://github.com/orbistry/alder/commit/76d003f798a88998fed70574fc9a421b77ef3c26) Update the embedded Deno runtime stack to restore Windows CLI builds and add a
  Windows build to regular CI. — Thanks @rvcas!
- Updated dependencies: alder-runtime@0.2.1

## 0.2.1 — 2026-09-03

### Patch changes

- Updated dependencies: alder-bundle@0.2.1, alder-driver@0.2.1

## 0.2.0 — 2026-09-03

### Minor changes

- [7d53578](https://github.com/orbistry/alder/commit/7d53578e5aef1c152bda29fb55181c78fd9af45d) Implement the M2 core-language pipeline through direct Oxc AST generation,
  Rolldown bundling, the embedded standalone runtime, stdlib/kernel foundations,
  formatting, and test execution. — Thanks @rvcas!

### Patch changes

- [2278da9](https://github.com/orbistry/alder/commit/2278da9f8e58a1c12f6bfdf0723af001c669fc41) Produce and persist solved module interfaces and deduplicated package instance indexes after successful builds. — Thanks @rvcas!
- [eb54db5](https://github.com/orbistry/alder/commit/eb54db59dee8a66087010c71ab9a123154e289d2) Execute the complete Traits guide example and broaden end-to-end trait runtime
  coverage across higher-kinded dictionaries and first-class constrained calls. — Thanks @rvcas!
- [b699950](https://github.com/orbistry/alder/commit/b699950207423f198107d2872dabf690d84235b3) Add source-aware miette diagnostics for compiler errors and warnings, render
  trait and parser failures with labeled snippets, and preserve Alder source in
  code generation snapshots. — Thanks @rvcas!
- [9789f23](https://github.com/orbistry/alder/commit/9789f237bd68714c67bffafa7e8836fc9a731a7f) Compile imported path-dependency sources into the same in-memory Oxc/Rolldown
  module graph, allowing dictionary factories selected from unimported sibling
  modules to bundle and execute without serializing generated JavaScript. — Thanks @rvcas!
- [70ab929](https://github.com/orbistry/alder/commit/70ab9292acb31029b43046e525bc9abaaa705e75) Load imported dependency package instance indexes into package-aware builds and deduplicate their implementations by stable identity. — Thanks @rvcas!
- Updated dependencies: alder-bundle@0.2.0, alder-config@0.2.0, alder-driver@0.2.0, alder-fmt@0.2.0, alder-runtime@0.2.0

