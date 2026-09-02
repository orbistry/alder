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

- `alder-language-server` on tower-lsp: diagnostics, hover, go to
  definition, formatting, code actions.
- Unsaved buffers through `InMemorySource` overlaying the file system.
- A browser playground via a WASM build of the LSP is planned.

## Error reporting

miette diagnostics with Elm-quality messages, including the full
`Reporting/Error/Syntax.hs` hierarchy ported from Elm and
Levenshtein-based suggestions.
