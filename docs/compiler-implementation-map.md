# Compiler implementation map

This map records the checked pipeline and inactive Elm-port remnants as audited
on the compiler-hardening branch. Rust module declarations, not filenames or
historical design snippets, determine what is compiled.

## Active checking path

The driver's `report::parse` entry point is implemented in
`alder-driver/src/report/syntax.rs`. It renders the active `alder-parse` nested
error hierarchy against retained source text; syntax quality and coverage are
tracked separately in `docs/parser-diagnostic-parity.md`.

`alder-driver/src/compile.rs` canonicalizes with `alder_can::canonicalize`, builds
requirement seeds with `alder_constrain::constrain`, builds a package-aware
`TraitDatabase`, and calls `alder_solve::solve`. Successful checked annotations
feed interface construction. Build/Test modes then call
`alder_codegen::emit_solved_module`; Check mode does not emit an artifact.

The active core inference implementation is `alder-solve/src/inference.rs`, with
trait/coherence support in `traits.rs` and constructor-specializing pattern
coverage/usefulness in `pattern_matrix.rs`. Inference checks all matches and
binding sites after solving, before publishing annotations. It does not use the old rank-based
union-find solver files listed below. The active constrain crate consists of
its `lib.rs` contract and `requirements.rs` traversal, not an Elm constraint tree.

Two compiled lower-level APIs must not be confused with this path:

- `alder_solve::run` infers core annotations but does not validate coherence or
  resolve collected trait obligations. The `infer` integration-test helper uses
  it; missing-instance/where-bound tests must use the full `solve_input` helper.
- `alder_codegen::emit_module` has no solved dictionary evidence. Production
  driver compilation uses `emit_solved_module`; unsolved emission tests alone
  cannot establish trait-call correctness.

## Progress reporting boundary

`alder_driver::build_with_reporter` accepts a presentation-independent
`progress::Reporter` via `Arc`. Events identify semantic phases and actual module
body-compilation starts, including unsuccessful compilation attempts. They are
available regardless of terminal verbosity and may arrive from a blocking worker;
callbacks must be thread-safe and return promptly. `build_with_dependencies`
remains the silent compatibility entry point used by editor and embedding callers.
Diagnostics remain structured in `BuildResult`, not embedded in progress events.

The CLI's `reporting::Output` owns styling, anstream stderr output, injected
writers, elapsed summaries, and verbosity. Proxy, formatting, bundling, and
program launch statuses use this renderer directly. The runtime separately offers
`execute_tests`/`TestEvent` for actual test results without intercepting console
streams. No editor progress notifications, JSON backend, or browser/WASM backend
is implemented by these seams.

## Unlinked Elm-port files

The driver's compiled `ModuleMeta`, `InterfaceCache::start_build`, and
`InterfaceCache::needs_rebuild` helpers also have no active build callers.
They are not evidence of timestamp-based incremental reuse. Current source
dependencies are rebuilt from source; only interface-only packages load saved
interfaces/indexes through `Project::build_dependencies`. The optional
`load_package_index` convenience method is likewise not the checked project
loading path, which uses `load_package_index_checked` and reports errors.

The following files exist but have no reachable `mod`, `#[path]`, or `include!`
edge from their crate root. Their internal tests are not workspace test coverage.
They are retained historical material, not active alternatives to the pipeline.

| Crate | Unlinked files under `src/` |
| --- | --- |
| alder-can | `accumulate.rs`, `module.rs`, `environment/dups.rs`, `environment/foreign.rs`, `environment/local.rs` |
| alder-constrain | `module.rs`, `expression.rs`, `pattern.rs`, `instantiate.rs`, `type_.rs`, `error_type.rs`, `error.rs`, `union_find.rs` |
| alder-solve | `solve.rs`, `annotation.rs`, `unify.rs`, `occurs.rs` |

For example, `alder-can/src/environment.rs` is active, but the old files in the
similarly named directory are not declared by it. Likewise, the exported
function `alder_solve::solve` comes from `inference.rs`, not `solve.rs`.
No legacy files were deleted or re-enabled by this audit. Recheck crate roots
and callers if these boundaries change.

## Deferred syntax versus executable behavior

The provisional M2 checking model is not a claim that later milestones execute.
Build/Test codegen rejects queries (M7), reactive state/components/markup (M6),
and styles (M8), rather than emitting runtime placeholders. Parsing and the
existing provisional Check-mode handling remain available.

Source macro invocations and comptime items fail in canonicalization. Therefore
the remaining canonical-AST MacroCall Any branch and ignored Comptime item in
later phases are not reachable from ordinary source compilation; codegen also
defensively rejects macro calls.

Table/schema declarations publish provisional opaque types, not runtime values;
macro declarations publish no callable interface value. These declarations emit
no code. Their absence from runtime output is not implemented database or macro
execution. Query/macro use sites are guarded as above. The driver regression
`deferred_declarations_publish_only_provisional_types` checks all three build
modes and a consumer using an imported opaque schema type. Companion tests reject
runtime table references and macro invocation without publishing an artifact or
interface for the failed consumer. See `source-boundaries-hardening.md`.

Provider context is an intentionally implemented runtime seam, separate from
unfinished provider checking. Do not disable it as though it were a web stub.
