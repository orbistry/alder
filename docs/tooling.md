# Alder Tooling

**Status: current direction, everything provisional.**

## CLI

A single `alder` binary (crate `alder-cli`) that embeds V8 via
`deno_core`.

| Command         | Purpose                                                              |
| --------------- | -------------------------------------------------------------------- |
| `alder init`    | Planned: scaffold a package or application                           |
| `alder check`   | Type-check without emitting JavaScript                               |
| `alder build`   | Compile and bundle (rolldown) for the package target                 |
| `alder run`     | Run `standalone` targets on the embedded runtime                     |
| `alder dev`     | Dev server with HMR; miniflare for `cloudflare`, deno_core otherwise |
| `alder test`    | Run `test` declarations on the target's runtime                      |
| `alder fmt`     | Formatter                                                            |
| `alder lsp`     | Language server over stdio                                           |
| `alder db ...`  | Planned: migrations, push, studio (see `data.md`)                     |
| `alder deploy`  | Build/configure/deploy a Worker, or validate with `--dry-run`          |
| `alder publish` | Planned: publish a package to the registry                           |
| `alder docs`    | Planned: generate documentation                                      |

Compiler version proxying stays: `"compiler": "X.Y.Z"` in `alder.jsonc`
makes the binary exec the matching cached version.

### Current CLI output

The implemented `check`, `build`, `run`, `dev`, `deploy`, `test`, and `fmt` commands use static,
aligned statuses on stderr. Default output identifies projects and meaningful
work; `--verbose` (`-v`) adds module, phase, file, and compiler-selection details.
`--quiet` (`-q`) suppresses routine statuses and successful summaries, but retains
warnings, errors, and failed test results. Quiet wins if both flags are supplied.
These flags and `--color auto|always|never` can appear before or after the
subcommand. Arguments following `run --` belong to the program, not the CLI.

```text
    Checking async (examples/async)
    Finished check in 0.18s · 2 modules

   Compiling api (apps/api)
    Bundling apps/api
       Built apps/api/dist/main.mjs
    Finished build in 1.24s
```

Times are elapsed wall-clock seconds. Failure summaries follow detailed source
diagnostics and count actual primary diagnostics, not blocked modules or related
source annotations. `run` and `test` finish their build before showing `Running`
or `Testing`; compilation, bundling, and runtime failures are distinguished.
The test runner supplies actual executed counts through a separate runtime
callback, so its stderr result summary appears once (including zero-test runs).
User-program stdout/stderr, including output within tests, are never filtered by
verbosity or color settings. LSP stdout remains exclusively protocol traffic.

`fmt` reports actual changes; `fmt --check` reports correctness without writing.
Workspace checks identify each member and report aggregate module counts. Other
commands still require an appropriate member project; no new workspace execution
or compiler-version resolution behavior is implied.

Automatic color respects nonempty `NO_COLOR` and disables color when stderr is
redirected or `TERM=dumb`. Explicit `always`/`never` overrides automatic color.
Statuses and source diagnostics share this policy; source hyperlinks retain
their independent terminal-support policy. Cached compiler selection is silent
unless verbose; downloads and completed installations are reported only when
performed. The launcher forwards reporting flags unchanged, so older selected
compiler versions must themselves support those options.

## Dev server

- `cloudflare` target: pinned Miniflare installed separately with
  `alder cloudflare setup`, using user-provided Node.js >=22. No delegation to `wrangler dev`
  or Vite.
- `standalone` web applications: embedded V8/deno_http with HMR.
- HMR preserves compatible signal and store state; incompatible component
  signatures reset local state with an explicit console reason. Compile errors
  retain the current page and recover after source correction. Route and public
  file edits are watched. Stop and restart dev when switching runtime targets.
- [Web development](web-development.md) documents create/dev/build/run commands,
  browser checks, shutdown, and the explicit deployment authorization boundary.

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

miette diagnostics with nested parser errors modeled on Elm's
`Reporting/Error/Syntax.hs` hierarchy and Levenshtein-based name suggestions.
Elm-quality parser rendering is completed in
`plans/parser-diagnostic-parity.md`. Active parser errors retain
their detailed causes and nearby context; family coverage, limitations, and
acceptance evidence are recorded in `docs/parser-diagnostic-parity.md`.

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

`alder fmt` combines consecutive same-visibility imports into canonical groups:
bundled, local, then external sections, sorted by module path with aliases
preserved. A single import stays ungrouped; declarations and public/private
boundaries are never crossed. Attached comments move with their imports.
Initialization order is determined by canonical module identities, not layout.
Grouped entries retain individual diagnostic regions in check/build/test and
the language server, including unsaved edits and clearing corrected diagnostics.
The language server still does not advertise a formatting capability; formatting
is available through `alder fmt`.

CLI diagnostics are ordered by source file and primary source location, not
message text. Source labels for files inside the project are relative to its
root (for example, `src/main.ald:3:25`), including warnings and related reports.
Files outside the project retain their full paths. This is presentation-only;
editor URIs and internal source identities remain unchanged.
On terminals supporting OSC 8 hyperlinks, the short source label links to the
absolute `file://` URI, independent of the shell's working directory. Redirected
output stays plain text. Interactive terminals without hyperlink support use
shell-relative paths instead, so their automatic file detection still resolves
correctly. `FORCE_HYPERLINK=1` enables links explicitly, and
`FORCE_HYPERLINK=0` disables them. Line and column remain visible in the header;
opening at that position depends on the terminal/editor integration.
The diagnostic-restoration acceptance matrix and remaining work
are tracked in `plans/diagnostic-ux.md`.
