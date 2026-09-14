---
cargo/alder-ast: minor
cargo/alder-parse: minor
cargo/alder-can: minor
cargo/alder-constrain: minor
cargo/alder-solve: minor
cargo/alder-codegen: minor
cargo/alder-kernel: minor
cargo/alder-bundle: minor
cargo/alder-driver: minor
cargo/alder-runtime: minor
cargo/alder-cli: minor
---

Implement the M6 web application workflow: checked-in reproducible HTML schema,
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
not a claim made by this changeset.
