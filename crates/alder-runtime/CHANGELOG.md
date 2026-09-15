# alder-runtime

## 0.4.0 — 2026-09-15

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

