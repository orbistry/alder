# M9: Tooling and ecosystem

The test runner grows up (power-assert, property tests, per-target
runners), the language server catches up with the new grammar and gets a
WASM playground, documentation generation from `///` comments, and
package publishing with semver enforced by API diff against the registry
protocol. The registry service itself is explicitly deferred past M9.

## Starting state

- M2b: minimal `alder test` (pass/fail), `alder fmt`, `alder run`, the
  comment side table (doc comments preserved with their kind).
- M5: macros (`assert` can stay compiler-known; property `Gen` derives
  are macros).
- `alder-language-server` exists as a tower-lsp skeleton with no features
  against the new grammar.
- `alder-config` uses pubgrub for resolution; `alder-cli` has the
  compiler-version proxy and GitHub-release download code from the Nash
  era.
- `docs/tooling.md` is the design.

## Exit criteria

- `alder test` runs `test` declarations and `tests { }` blocks with
  power-assert diffs (both sides of a comparison shown), property tests
  via a `Gen` trait with shrinking, filtering by name, and a per-target
  runner: deno_core for `standalone`, miniflare for `cloudflare`.
  `testDependencies` never reach production bundles.
- The language server provides diagnostics (syntax, canonicalization,
  type), hover with types (including inferred error rows and `Task`),
  go-to-definition across packages, formatting via `alder-fmt`, rename,
  and code actions for the compiler's suggestions; unsaved buffers via
  `InMemorySource`.
- A WASM build of the LSP drives a browser playground (Monaco) that runs
  `alder check` and, for `standalone` programs, `alder run` via a
  deno-free path (the JS output executed in the page).
- `alder docs` generates a static site from `///` and `//!` comments
  with type signatures, inferred rows, and examples that are checked by
  `alder test`.
- `alder publish` packs a package, computes its API surface, and enforces
  semver by diffing against the previous version's surface; `alder add`
  resolves and downloads packages with the Rust client. Both talk to the
  registry protocol defined here; a local file-system registry adapter
  makes it usable and testable before the hosted service exists.

## Settled decisions

- Own registry, Git-backed history, semver by API diff (Elm's model).
- The registry service is built later in Alder on Cloudflare (with
  PlanetScale for the database, not D1); it is not part of M9 and Alder
  must work well without it. M9 ships the protocol, the Rust client, a
  file-system adapter, and a GitHub-releases adapter for bootstrapping.
- `assert expr` is compiler-known (M1 grammar); `tests { }` is pruned
  from non-test builds (M2b).

## Open decisions (recommendation in bold)

1. API surface format for semver diffing. **A canonical JSON of every
   `pub` item's signature (types normalized, rows rendered), stored in the
   package tarball and in the registry index; diff rules: removed or
   changed signature is major, added is minor, otherwise patch.**
2. Registry protocol. **HTTP JSON: `GET /packages/{author}/{name}`
   (versions + surfaces), `GET /packages/{author}/{name}/{version}.tar.xz`,
   `PUT` for publish with a token; adapters implement the same trait
   locally.**
3. Property testing. **`Gen[a]` trait with derives for records and
   enums, `forall(gen, fn(a) Bool)`, integrated shrinking (Hedgehog
   style), seed printed on failure.**
4. Playground execution. **Run the compiled JS in the browser via an
   iframe with the kernel; no deno_core in WASM.**
5. Doc examples as tests. **Code blocks in `///` tagged ` ```alder ` compile and run under `alder test --docs`.**

## Work breakdown

### Wave 0: contract

Design panel producing `docs/tooling-internals.md`: test runner protocol
(discovery, registry, reporters, per-target execution, power-assert
lowering), `Gen`, LSP architecture over the driver (incremental
reanalysis, reverse-dependency invalidation, symbol index), WASM build
constraints, doc generator model, API surface format, registry protocol
and adapters, semver rules, lockfile format (`alder.lock`).

### Wave 1 (parallel)

- Test runner: power-assert lowering in codegen, reporters, filters,
  per-target execution, `Gen` and shrinking in `std/`.
- LSP: diagnostics, hover, definition, formatting, rename, code actions.
- Docs generator.

### Wave 2 (parallel)

- Registry client: protocol crate, adapters (fs, GitHub releases),
  `alder add`/`remove`/`update`, lockfile, `alder publish` with surface
  extraction and semver check.
- WASM LSP build and playground under `web/`.

### Wave 3: sweep

- Docs, SPEC M9 ticked, changeset, critic pass; dogfood: publish `std/`
  packages to the fs adapter in CI and resolve them from an e2e project.

## Tests to add (minimum)

- Runner: failing assert output snapshot; property failure with shrunk
  counterexample; per-target selection; `tests { }` pruned from build.
- LSP: request/response fixtures for each feature over a two-package
  workspace.
- Docs: generated page snapshots; doc example that fails to compile is
  reported.
- Registry: surface extraction snapshots; semver decisions for add,
  remove, change; resolution with pubgrub against the fs adapter; lockfile
  round trip.

## Risks

- The LSP needs incremental reanalysis that the driver was designed for
  but never exercised; expect driver changes.
- API surface normalization must be stable across compiler versions or
  semver diffs will lie; version the format.
- The registry service is deliberately absent; do not let "we need the
  server to test this" block the milestone. The fs adapter is the test
  target.
