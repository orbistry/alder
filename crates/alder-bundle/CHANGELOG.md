# alder-bundle

## 0.5.0 — 2026-09-15

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
- [0bc4c06](https://github.com/orbistry/alder/commit/0bc4c06c91c71cdcb0f3db8aa7e13d311dc210f4) Preserve complete Rolldown output graphs and build production web applications
  with minified, content-hashed, lazily loaded route chunks and shared dependencies.
  Generate deterministic manifests, active-route preload links, hidden generated-JS
  source maps, and raw/gzip/Brotli size reports. Integrate hashed assets with staged
  standalone/Worker builds, immutable caching, build identity checks, and bounded
  client recovery. Keep development bundles eager and compatible with existing HMR. — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-codegen@0.6.0, alder-kernel@0.5.0, alder-runtime@0.4.0

## 0.4.1 — 2026-09-11

### Patch changes

- Updated dependencies: alder-codegen@0.5.1

## 0.4.0 — 2026-09-07

### Minor changes

- [2cc1d05](https://github.com/orbistry/alder/commit/2cc1d057460100bce03b09d5cda3f8e7975f1a87) Unify bundled, local, and external imports with grouped syntax, lowercase
  standard-library namespaces, explicit utility imports, identity-preserving
  namespace re-exports, and canonical comment-preserving formatting. Share public
  interfaces between prelude and explicit imports, reject conflicting bindings,
  and make module initialization independent of import declaration order.
  
  Interface format 8 replaces earlier contracts without compatibility readers. — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-codegen@0.5.0

## 0.3.1 — 2026-09-06

### Patch changes

- Updated dependencies: alder-codegen@0.4.1, alder-kernel@0.4.1, alder-report@0.3.0

## 0.3.0 — 2026-09-06

### Minor changes

- [83c2be3](https://github.com/orbistry/alder/commit/83c2be3acb3e9216511e9e91168a8365df08e992) Add lazy Ref cells with synchronous atomic reads, writes, updates, and modifications. — Thanks @rvcas!
- [f87e540](https://github.com/orbistry/alder/commit/f87e540113feb6c1052b2d782a7ad6e1faa7d204) Add inferred lazy tasks, generator-based async lowering, Promise extern lifting,
  and a structured fiber runtime with interruption, scopes, finalizers, `all`, and
  `race`. — Thanks @rvcas!
- [83c2be3](https://github.com/orbistry/alder/commit/83c2be3acb3e9216511e9e91168a8365df08e992) Add fixed-capacity semaphores with FIFO weighted requests and scoped,
  cancellation-safe permit ownership. — Thanks @rvcas!
- [83c2be3](https://github.com/orbistry/alder/commit/83c2be3acb3e9216511e9e91168a8365df08e992) Add synchronized shared cells with serialized task-based state transformations. — Thanks @rvcas!

### Patch changes

- [1efea6c](https://github.com/orbistry/alder/commit/1efea6c0a882752faf882525f5f292e2b20a6360) Require Json evidence for module encode/decode calls and dispatch through the
  selected codec, including custom instances. Remove unchecked JSON entry points
  and fix public export names for imported direct dictionary calls. — Thanks @rvcas!
- [54d1b63](https://github.com/orbistry/alder/commit/54d1b636097930bb115607e6b11b391954927339) Carry physical source origins alongside generated ASTs so local JavaScript extern modules resolve beside their Alder declarations. Preserve virtual module identities after AST transfer, order bundle inputs deterministically, and verify Promise fulfillment, foreign defects, and cancellation through local wrappers. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Expose Fiber.map, forEach, tryMap, and tryForEach with optional MapOptions,
  execution-time concurrency validation, and sequential defaults. — Thanks @rvcas!
- [41f55b6](https://github.com/orbistry/alder/commit/41f55b6201ca3abebac3fafd4e88ff0031d627a7) Retain Alder source and extern declaration regions through bundling, render unresolved externs as labeled shared diagnostics, and verify sibling JavaScript wrapper resolution across path-dependency packages. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Expose `Fiber.unbounded` as a Number value for explicit unbounded traversal
  concurrency, including typed built-in value lookup and the bundled runtime export. — Thanks @rvcas!
- [edbcd63](https://github.com/orbistry/alder/commit/edbcd633652e4097acc18656bca8cd1b891a219a) Add Array.iter and independent ArrayIterator cursors, replacing the non-advancing
  Iterator instance on arrays. Preserve shared source values, live iteration, and
  correct Option payloads through exhaustion. — Thanks @rvcas!
- Updated dependencies: alder-codegen@0.4.0, alder-kernel@0.4.0

## 0.2.2 — 2026-09-04

### Patch changes

- Updated dependencies: alder-codegen@0.3.0, alder-kernel@0.3.0

## 0.2.1 — 2026-09-03

### Patch changes

- Updated dependencies: alder-codegen@0.2.1

## 0.2.0 — 2026-09-03

### Minor changes

- [0258ff1](https://github.com/orbistry/alder/commit/0258ff1fef3e279249933be2a3c8e149ad28afcf) Adopt arrow lambdas and juxtaposed function return types, forward piped values
  to the first argument of existing calls, and add `Array.filter` for pipeline
  composition. — Thanks @rvcas!
- [7d53578](https://github.com/orbistry/alder/commit/7d53578e5aef1c152bda29fb55181c78fd9af45d) Implement the M2 core-language pipeline through direct Oxc AST generation,
  Rolldown bundling, the embedded standalone runtime, stdlib/kernel foundations,
  formatting, and test execution. — Thanks @rvcas!

### Patch changes

- [a9be91f](https://github.com/orbistry/alder/commit/a9be91f26e5dc7b825c8d15ade9f13c12ebca8da) Add the explicit `Ref.same` identity operation selected by the trait equality
  design, backed by JavaScript reference equality and covered at runtime. — Thanks @rvcas!
- Updated dependencies: alder-codegen@0.2.0, alder-kernel@0.2.0

