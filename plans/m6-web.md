# M6: Web vertical slice

Components with compile-time-tracked signals, typed markup checked against
an HTML schema, SSR with hydration, SvelteKit-style folder routing with
generated `PageData` and typed `Routes`, remote functions and
`+page.server.ald` as the server boundary, server hooks with typed context,
request-scoped module stores, page options, `alder dev` on vendored
miniflare, `alder deploy` generating `wrangler.jsonc`, and Cloudflare
bindings via traits and attributes. The slice ends with a deployed page on
Workers and the same app running self-hosted on `standalone`.

## Current scope and sequencing

The full application implementation is now present. The detailed
[acceptance ledger](m6-acceptance.md) records direct regressions and real-browser,
standalone, Miniflare, HMR, prerender, and deployment dry-run evidence. Workspace
formatting, strict Clippy, and tests pass. A real preview deployment is still an
open external gate: no exact destination/account authorization was supplied.
Do not equate the implemented deploy command or a dry run with that exit criterion.

Current user instructions are in [web-development.md](../docs/web-development.md)
and [the full example](../examples/web-full/README.md). The counter-slice account
below is historical, retained to explain the implementation sequence.

The active goal is now the **entire M6 milestone**, building on the completed
counter slice below. Connect filesystem routes, `alder dev`, SSR, browser
hydration, and reload-on-save early, then continue through all remaining waves.
Track requirement-level implementation and verification in
`plans/m6-acceptance.md`; a working first browser loop does not complete M6.
Macros/comptime (M5) and statically checked services/layers dependency injection
remain deferred and are not prerequisites for this work. Preserve existing
context behavior; do not implement new DI syntax, provider checking, or migration.
Later hooks/bindings work must not claim the deferred DI guarantees.

The first deliverable is a small Alder counter component with typed props,
typed markup, an event handler, local `state`, and a derived value: render it
on the server, hydrate the existing nodes, and update text/attributes in the
browser without re-running the component or replacing the hydrated DOM.

Acceptance for this first slice:

- [x] Write `docs/web-internals.md` contracts for markup/props/event checking,
  state dependencies, derived values, component ownership/disposal, DOM lowering,
  SSR escaping, hydration identity, and mismatch behavior before implementation.
- [x] Compile the counter from Alder source through the real compiler pipeline;
  no hand-written JavaScript substitute or identity/ordinary-function stubs.
- [x] Reject invalid markup, props, and handlers with source-aware diagnostics
  for the supported surface. Keep explicit diagnostics for unsupported features.
- [x] Verify initial SSR output, safe escaping, hydration node reuse, event
  attachment, state/derived updates, independent component instances, and cleanup.
- [x] Add direct compiler and kernel regression tests, plus a fixture under
  `tests/e2e/` for optional manual CLI/browser confirmation.
- [x] Pass workspace formatting, strict Clippy, and tests; update docs, SPEC
  progress, and a changeset for the implementation without marking all M6 done.

Reactive directives/keyed lists, routing, remotes, hooks/stores, async resources,
the M6 form/action surface, HMR, development tooling, both runtime adapters, and
deployment are all in the active goal. The historical counter-only scope is
superseded. Macros/comptime, the DI redesign, and unrelated M7/M8 work remain out
of scope; do not use those deferred milestones to silently omit M6 guarantees.

## First-slice implementation (historical)

The counter now compiles through parse/canonicalization/inference, direct Oxc
lowering, Rolldown, and V8. The checked-in seven-element HTML subset and `onClick`
`fn() ()` handler contract are intentionally smaller than the eventual schema
and event-object design below. `html.renderToString` is available after explicit
import; browser hosts call kernel mount/hydrate entry points. No platform
bootstrap or adapter was added. Detailed boundaries and test locations are in
`docs/web-internals.md`.

Verification (2026-09-14): `cargo fmt --all`,
`cargo clippy --all-targets --all-features -- -D warnings`, and
`cargo test --quiet` pass. The focused coverage includes 26 driver web tests
and five kernel web tests. A manual
`cargo run --manifest-path ../../../Cargo.toml -p alder-cli -- run` from
`tests/e2e/web` also passed and printed the expected SSR counter with values
2 and 4. Hydration was verified with the deterministic DOM shim, not a real
browser or platform deployment. No CLI subprocess tests were added.

At that checkpoint, follow-on component work still included the full schema, custom elements,
component tags/children composition, directives/keyed lists, richer event types,
and reactive patterns excluded by first-slice diagnostics. That checkpoint did
not complete routing, resources, forms, HMR, or deployment. Current implementation
and verification supersede that scope; DI and macros remain intentionally deferred.

## Starting state (historical)

- Hardening rejects executable `state` expressions and `component` declarations
  with source-aware diagnostics until this milestone implements their actual
  signal/lifecycle semantics. Check mode retains provisional syntax and typing.
  Replace these explicit codegen guards when the corresponding runtime and
  lowering are implemented; do not restore identity/ordinary-function stubs.
- Standalone markup also has an executable-codegen guard. The old descriptor
  lowering silently replaced directive children with undefined and was removed;
  implement actual rendering rather than relying on that historical output.
- M2b: codegen, kernel, `alder run`/`build`. M4: fibers, existing runtime
  context, error rows. Built-in derives remain available without M5 macros.
- Parser: markup with `@if`/`@for`/`@match`, `component`, `state(...)`,
  keyword-insensitive element names.
- `docs/web.md` is the design; `docs/runtime.md` Cloudflare and Targets
  sections; the two-by-two target table.

## Exit criteria

- `alder dev` serves a routed app with HMR that preserves signal state;
  `alder build` produces a Worker (cloudflare) or a server bundle
  (standalone) plus client assets; `alder deploy` deploys to Cloudflare.
- A `+page.ald` with `load`, a `+page.server.ald` with a server `load`
  and an action, a `+layout.ald`, a `+server.ald` JSON endpoint, an
  `+error.ald`, and a `*.remote.ald` module all work end to end with
  typed `props.data`, typed `event.params`, and typed remote stubs.
- Markup is checked against an HTML schema: unknown elements, unknown
  attributes, wrong attribute value types, and invalid nesting are
  compile errors; custom elements with dashes are allowed.
- Components run once; `state` bindings are signals; derived `let`s that
  read state are memoized; `@if`/`@for`/`@match` compile to reactive
  regions; keyed `@for` reconciles.
- SSR renders to a string on the server, the client hydrates without
  re-rendering, and server-only code is unreachable from the client
  bundle (reachability error with path).
- `hooks.server.ald` `handle` provides typed context to every `load`,
  action, and remote function.
- Module stores are per-request on the server and singletons in the
  browser.
- Page options `prerender`, `ssr`, `csr`, `trailingSlash` inherit down
  the tree; prerendered routes are emitted at build time.

## Settled decisions

- Route discovery is a compiler pass over `src/routes/`, producing `Routes`,
  `PageData`, and server/client entry points; it does not require user macros
  or `comptime`. Future compile-time filesystem APIs are separate user features.
- Svelte 5 runes model: compile-time dependency tracking, components run
  once, `state(x)` bound to an ordinary writable `let`.
- JSX-shaped typed markup with `@` directives (TSRX rules: statements in
  directive bodies are setup and do not render).
- SvelteKit file conventions exactly (`+page.ald`, `+page.server.ald`,
  `+layout.ald`, `+layout.server.ald`, `+server.ald`, `+error.ald`,
  `hooks.server.ald`, `hooks.client.ald`, `*.remote.ald`); no
  `alder.jsonc` render-mode default; page options as exported values.
- JavaScript required in the browser; no progressive enhancement.
- Remote modules, server page/layout companions, endpoints, and server hooks
  are server boundaries; no per-function boundary attribute.
- Cloudflare concepts are traits plus attributes; bindings arrive through
  context; `alder deploy` owns wrangler config and migrations.
- Dev server: vendored miniflare for cloudflare, deno_core with HMR for
  standalone; never `wrangler dev` or Vite.

## Resolved implementation decisions

1. HTML schema: checked-in Rust tables generated offline from a pinned,
   compressed WHATWG index with a source digest. ARIA and `data-*` are supported;
   refresh is explicit and ordinary builds never fetch schema data. See
   `tools/html-schema.md` for the input and commands.
2. Events: schema-typed event records, with zero-argument compatibility;
   handlers return Unit or Task[Unit]. InputEvent fields are restricted to
   statically known text controls. Runtime defects reach owner-scoped client hooks.
3. Reactivity: per-expression compiler-generated dependencies and direct DOM
   subscriptions, no virtual DOM. Keyed regions retain owners and nodes.
4. Resources: `html.resource(() -> task)` yields typed Loading/Ready/Failed,
   owns cancellation, suspends SSR, and transfers resolved values for hydration.
5. Transport: validated tagged JSON graphs preserve enums, nested Options,
   Map/Set/BigInt/Date, identity and cycles; executable/accessor/host values are
   rejected and traversal budgets apply symmetrically. Only browser-reachable
   stores enter hydration snapshots.
6. Queries: browser argument-keyed cache per page, bounded to 256 entries and
   30 seconds. Commands invalidate their module, actions invalidate all modules,
   explicit refresh bypasses caches, and navigation rejects late old-page cache
   writes. Server calls do not share query caches across requests.
7. Navigation: universal loads run in the browser. Ancestor universal loads also
   run on the server when downstream server loads need their trusted data;
   explicit server return records are transferred, never browser-derived data
   sent back as trusted server input. Initial SSR/CSR bootstrap and HMR resolve
   data on the server. Exact semantics are in `docs/web-internals.md`.
8. Error boundaries render inside their same-directory layouts and exclude
   descendant layouts. A failed enclosing layout needs an ancestor boundary;
   otherwise server output is a sanitized 500, and client replacement preserves
   its last good tree.
9. Build/deploy: public assets and generated outputs are staged with rollback;
   prerender uses local target providers. Deploy requires an exact Worker/account,
   preserves existing variables/secrets, and never invents resource IDs or
   destructive migration history. The live-preview acceptance gate is separate.

## Work breakdown

### Wave 0: contract

Write `docs/web-internals.md`, settling the first-slice contracts first. The
remaining contracts below are addressed when their respective waves begin;
they do not gate the counter slice:

- Markup checking: schema tables, component prop typing from records with
  optional fields, children typing, event typing.
- Reactivity compiler: dependency tracking rules over the canonical AST
  (which reads are tracked, memo boundaries, when a block re-runs),
  emitted DOM operations, keyed reconciliation, SSR string builder,
  hydration walk.
- Routing pass: file discovery, `Routes` and `PageData` generation, param
  typing, layouts, error boundaries, page option inheritance, prerender.
- Server boundary: reachability analysis from the client entry, remote
  stub generation (typed HTTP with the `Result` carried), `+page.server`
  loads/actions, hooks with existing runtime context. Public service/provider
  syntax and static DI guarantees remain deferred to the DI plan.
- Stores: request scoping via the fiber context map.
- Cloudflare: `DurableObject`, `Queue`, `Workflow` traits; binding
  attributes; `wrangler.jsonc` generation; miniflare vendoring layout
  under the compiler's support files; deploy flow.
- Kernel modules: `dom`, `ssr`, `hydrate`, `signals`, `router`,
  `resource`, `stores`.

### Wave 1: components

Land the counter slice described above first, then expand to the full component
criteria. Resources and form primitives below are follow-on work, not counter
acceptance requirements.

- Markup checker (`alder-can`/`alder-constrain`).
- Reactivity compiler and DOM codegen (`alder-codegen`).
- Kernel `signals`, `dom`, `ssr`, `hydrate`, `resource`.
- `std/` `Html` module, event types, `Form` primitives (M8 fills forms).
- Tests: a browser-less DOM (a small DOM shim in the kernel test suite or
  `deno_dom`) driving component tests under deno_core in `cargo test`.

### Wave 2: routing and server

- Routing pass and generated modules.
- Server boundary and remote stubs; hooks; stores.
- Page options and prerender.
- Kernel `router`, request pipeline, error boundaries.

### Wave 3: platform

- Cloudflare traits/attributes, bindings through context,
  `wrangler.jsonc` generation, `alder deploy`.
- `alder dev`: miniflare vendoring and process management; deno_core HMR
  for standalone; HMR protocol preserving signal state.
- `standalone` server adapter over `deno_http`.

### Wave 4: sweep

- e2e: the docs' example app (users list, user page with server load,
  remote delete, sign-up form action stub, hooks auth) deployed to a
  Cloudflare preview and run under `standalone`; playwright-style browser
  tests through deno_core's headless option are out of scope; use
  request-level tests plus the DOM shim.
- Docs, SPEC M6 ticked, changeset, critic pass.

## Testing policy

- Rust unit/integration tests call compiler, driver, and kernel APIs directly.
  Do not add tests that spawn the Alder CLI, `cargo run`, or `cargo build` to
  exercise fixtures; do not restore the removed CLI subprocess E2E harness.
- Exercise emitted JavaScript in the existing runtime test harness and use a
  deterministic DOM shim for fast component/hydration regressions in CI.
- Manual CLI invocations on `tests/e2e/` fixtures are allowed for development
  confirmation. Record commands and results separately from automated coverage.
- Platform process/deployment smoke checks belong to explicit, opt-in manual
  validation, not the ordinary `cargo test` acceptance path. Deployments still
  require authorization for the specific target.

## Tests to add (minimum)

- Markup errors: unknown element, unknown attribute, bad value type, bad
  nesting (`<p><div>`), custom element allowed, keyword-named elements.
- Reactivity: a derived value re-runs only when its dependency changes;
  keyed list reorder preserves nodes; `@match` region swaps; SSR output
  snapshot per component; hydration attaches without mutation.
- Routing: `Routes` snapshot for a fixture tree; `PageData` merge across
  two layouts; param typing; `+error` fallback; prerender output.
- Boundary: server-only import from a component is rejected with a path;
  remote stub round-trip with a `Result` error; hooks provide/`use`.
- Cloudflare: generated `wrangler.jsonc` snapshot; a Durable Object impl
  type-checks via direct compiler APIs; boot the built worker in miniflare
  as an explicit manual platform smoke check, outside `cargo test`.

## Risks

- The reactivity compiler is the hardest single piece in the roadmap;
  build it against the DOM shim with exhaustive small tests before wiring
  SSR.
- Vendoring miniflare inside the compiler's support files needs a build
  pipeline for npm artifacts; decide early whether the `alder` release
  bundles them or downloads on first `alder dev`.
- HMR that preserves signal state needs module identity across reloads;
  design it with the codegen, not after.
