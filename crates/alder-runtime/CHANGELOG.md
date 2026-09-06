# alder-runtime

## 0.3.0 — 2026-09-06

### Minor changes

- [52f2954](https://github.com/orbistry/alder/commit/52f29549f4881d0c493332ee8549df1adcf1fbbe) Add consistent Cargo-style CLI statuses, global quiet/verbose/color options,
  elapsed summaries, accurate diagnostic counts, and compiler proxy reporting.
  Expose optional semantic driver progress and an injected CLI renderer while
  preserving source diagnostics, hyperlinks, and program/LSP streams. Add opt-in
  structured runtime test results so CLI test summaries use actual executed counts
  on stderr without intercepting user output or duplicating failure summaries. — Thanks @rvcas!

## 0.2.2 — 2026-09-06

### Patch changes

- [f87e540](https://github.com/orbistry/alder/commit/f87e540113feb6c1052b2d782a7ad6e1faa7d204) Add inferred lazy tasks, generator-based async lowering, Promise extern lifting,
  and a structured fiber runtime with interruption, scopes, finalizers, `all`, and
  `race`. — Thanks @rvcas!

## 0.2.1 — 2026-09-04

### Patch changes

- [76d003f](https://github.com/orbistry/alder/commit/76d003f798a88998fed70574fc9a421b77ef3c26) Update the embedded Deno runtime stack to restore Windows CLI builds and add a
  Windows build to regular CI. — Thanks @rvcas!

## 0.2.0 — 2026-09-03

### Minor changes

- [7d53578](https://github.com/orbistry/alder/commit/7d53578e5aef1c152bda29fb55181c78fd9af45d) Implement the M2 core-language pipeline through direct Oxc AST generation,
  Rolldown bundling, the embedded standalone runtime, stdlib/kernel foundations,
  formatting, and test execution. — Thanks @rvcas!

