# Compiler implementation map

This map records the checked pipeline after union-find restoration and removal
of the inactive Elm-port remnants. Rust module declarations, not historical
design snippets, determine what is compiled.

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

The active core inference implementation is `alder-solve/src/inference.rs`, using
the shared type graph in `type_graph.rs`, with
trait/coherence support in `traits.rs` and constructor-specializing pattern
coverage/usefulness in `pattern_matrix.rs`. Inference checks all matches and
binding sites after solving, before publishing annotations. Graph representatives
use union-by-size and path compression; type constructors contain shared handles,
not recursively owned type trees. Diagnostic variable IDs are independent of the
weighted root. Generalization retains Alder's free-environment sets, mutation
restrictions, and explicit universal contracts—not Elm's rank-pool semantics.
Failed attempts discard their whole graph and deferred state before recovery.
The active constrain crate consists of
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

## Removed obsolete implementations

The unused driver `ModuleMeta`, `InterfaceCache::start_build`, and
`InterfaceCache::needs_rebuild` helpers have been removed. Current source
dependencies are rebuilt from source; only interface-only packages load saved
interfaces/indexes through `Project::build_dependencies`.
The unused error-discarding `load_package_index` convenience method was removed;
the project loading path retains `load_package_index_checked` and reports errors.

The following files were verified to have no reachable `mod`, `#[path]`, or
`include!` edge, packaging consumer, or active fixture role before deletion.
Their internal tests were never workspace test coverage. No referenced snapshots
were removed with these files; the local `elm/` reference remains untouched.

| Crate | Removed files under `src/` |
| --- | --- |
| alder-can | `accumulate.rs`, `module.rs`, `environment/dups.rs`, `environment/foreign.rs`, `environment/local.rs` |
| alder-constrain | `module.rs`, `expression.rs`, `pattern.rs`, `instantiate.rs`, `type_.rs`, `error_type.rs`, `error.rs`, `union_find.rs` |
| alder-solve | `solve.rs`, `annotation.rs`, `unify.rs`, `occurs.rs` |

`alder-can/src/environment.rs` remains active. The exported function
`alder_solve::solve` comes from `inference.rs`. There is no substitution-table
fallback or alternative solver. The empty unpublished `tasks/stub` package and
its workspace/CI references were also removed. See
`union-find-restoration.md` for the audit inventory and measurement evidence.

## Deferred syntax versus executable behavior

The provisional M2 checking model is not a claim that later milestones execute.
Build/Test codegen rejects queries (M7), styles and schema-generated forms (M8),
rather than emitting runtime placeholders. Components
with annotated props, direct state/derived lets, and final supported markup now
lower through `oxc_backend/web.rs` into the kernel's real DOM/SSR/hydration
operations, including typed composition, reactive directives/keyed lists,
resources and stores. Driver web passes generate route/load/action/remote types
and enforce client boundaries; generated entries wire browser hydration and both
server adapters. Standalone markup outside components, unsupported nested state,
and other unsupported executable forms retain explicit diagnostics. See
`web-internals.md` for the supported checking and execution contracts.

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
