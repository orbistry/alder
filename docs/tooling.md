# Alder Tooling

**Status: current direction, everything provisional.**

## CLI

A single `alder` binary (crate `alder-cli`) that embeds V8 via
`deno_core`.

| Command         | Purpose                                                              |
| --------------- | -------------------------------------------------------------------- |
| `alder init`    | Scaffold a package or application                                    |
| `alder check`   | Type-check without emitting JavaScript                               |
| `alder build`   | Compile and bundle (rolldown) for the package target                 |
| `alder run`     | Run `standalone` targets on the embedded runtime                     |
| `alder dev`     | Dev server with HMR; miniflare for `cloudflare`, deno_core otherwise |
| `alder test`    | Run `test` declarations on the target's runtime                      |
| `alder fmt`     | Formatter                                                            |
| `alder lsp`     | Language server over stdio                                           |
| `alder db ...`  | Migrations, push, studio (see `data.md`)                             |
| `alder deploy`  | Generate config, run migrations, deploy                              |
| `alder publish` | Publish a package to the registry                                    |
| `alder docs`    | Generate documentation                                               |

Compiler version proxying stays: `"compiler": "X.Y.Z"` in `alder.jsonc`
makes the binary exec the matching cached version.

## Dev server

- `cloudflare` target: a vendored miniflare shipped as compiler support
  files (not a static part of the binary). No delegation to `wrangler dev`
  or Vite.
- `server` and `tui` targets: deno_core with HMR.
- HMR preserves signal and store state across component reloads.

## Testing

- `test "name" { ... }` declarations and module-level `tests { }` blocks
  (see `language.md`). Test-only imports are pruned from other builds.
- `assert expr` is compiler-known and reports both sides of comparisons.
- Property tests through a `Gen` trait.
- `testDependencies` in `alder.jsonc` never reach production bundles.

## Packages

- Alder has its own registry, Git-backed, like Elm's.
- Semver is enforced by diffing exported types between versions: a
  changed `pub` signature forces a major bump.
- Dependency resolution uses pubgrub (already a dependency of
  `alder-config`).
- npm code is reachable only through `extern` declarations inside Alder
  packages; there is no direct npm dependency in `alder.jsonc`.
- Workspaces (`"type": "workspace"`) hold several members with per-member
  targets.

## Macros at build time

Macros and `comptime` blocks are compiled to JavaScript and executed in
the compiler's embedded V8 during the build. Output is cached per module.
**Open:** sandboxing and determinism guarantees.

## Language server and editor

- `alder-language-server` uses tower-lsp-server's native Tokio stdio transport.
  Diagnostics are implemented; hover, go to definition, formatting, and code
  actions remain planned.
- Full-text open/change/close notifications maintain versioned unsaved buffers
  through `InMemorySource` overlaying the file system. Each update checks the
  containing projects afresh, including dependents, without writing build
  artifacts or interface caches. Older document versions are ignored.
- Diagnostics include errors, unused warnings, UTF-16 ranges, codes, help text,
  and secondary requirement locations. Empty publications clear stale results.
  Save and client-supplied watched-file notifications recheck disk dependencies.
  The server does not yet register file watchers or provide incremental checking.
  File-backed projects are required; untitled documents are not checked.
- A browser playground via a WASM build of the LSP is planned.

## Error reporting

miette diagnostics with Elm-quality messages, including the full
`Reporting/Error/Syntax.hs` hierarchy ported from Elm and
Levenshtein-based suggestions.

The active compiler emits unused-local/parameter, module-value and unused-import-binding
warnings. Import usage includes type and trait references, constructors, and
qualified value access; public re-exports count as intentional uses. Warnings
do not remove code. In particular, an import with no directly referenced names
may still be needed for initialization effects or trait instances. Local discard
hints preserve needed initializer effects and distinguish `_`, unnamed array
rests, and aliases. Module-value reachability starts at exports, `main`, and
references in evaluated initializers, tests, and trait/impl bodies. Unused private
recursive functions can warn; helpers needed by initializers do not. This is a
conservative module-local value analysis, not whole-program dead-code elimination
or unused-type analysis. Declaration warnings do not suggest invalid `_` function
names, and warnings never remove code.

CLI diagnostics are ordered by source file and primary source location, not
message text. The diagnostic-restoration acceptance matrix and remaining work
are tracked in `plans/diagnostic-ux.md`.
