# Async and concurrency decisions

Status: approved direction recorded September 5, 2026; implementation in progress.
This extends compiler hardening, not the later web/data/provider milestones.
Existing inferred-async behavior is not the final contract below.

## Implementation evidence

- Parser/source AST: `async` is reserved; named functions, trait signatures,
  and implementation methods retain an explicit `is_async` flag. Async blocks
  have a distinct `Expr::Async` node. Ordinary lambdas can return these blocks,
  including lambdas annotated with their actual `Task[...]` result.
- Added twelve granular parser regressions, including zero-await and nested
  blocks, postfix await, public main, methods, and malformed declarations.
  `cargo test -p alder-parse --quiet`: 1304 tests passed. The 56 changed existing
  snapshots were mechanically checked against HEAD: their only change is the
  added `is_async: false` field. New snapshots retain actual Alder source.
- Canonical AST and scope checking carry explicit async mode for functions and
  methods, reject await in plain functions (including Task-annotated ones), and
  prevent lambda permission inheritance or break/continue across async blocks.
  Async creation falls through without executing body exits; dependency and
  dictionary-requirement walks still inspect the body. All 76 canonicalizer
  tests passed, including eight additional boundary regressions.
- Solver function seeds and body checking now use explicit mode, always adding
  exactly one Task layer. Lambda annotations describe their actual return type.
  Async blocks own return types and loop/reachability state. Trait method schemes
  add the Task layer too. Seven focused inference tests pass, with reviewed
  snapshots proving zero-await wrapping, nested tasks, outer-return isolation,
  annotated lambdas, and trait calls. Removed the superseded inferred-async test
  snapshot; its regression now uses explicit syntax.
- Direct lowering uses explicit function/method mode and emits async blocks as
  lazy `$task(function* () { ... })` expressions. Lambdas stay ordinary closures;
  task and function boundaries isolate loop targets. The existing task/frame
  ABI is unchanged. Re-read effects-internals and the pinned Effect interpreter
  and generator suspension entry at bd393d63c19bdd0ab212d95576cec89051c8501c;
  no reference code was copied.
- All 43 codegen tests pass. Four new snapshots were reviewed for lazy factories,
  nested task layers, and binding capture. Two previous async fixtures now use
  explicit syntax; their generated JS is unchanged (snapshot metadata refreshed).
  The new explicit_async CLI fixture passes through compilation, bundling, and
  V8 execution, asserting call-time arguments, lazy/reused bodies, captured
  reassignment, lambda blocks, nested tasks, and independent block returns.
- `cargo check --workspace` and strict workspace Clippy pass. This is an
  implementation checkpoint, not full async acceptance: audit extern declarations and test
  entry points, migrate remaining inferred-async fixtures/examples, and expand
  adversarial/runtime/interface tests before claiming async acceptance.
- Solver fixture migration now passes all 270 integration tests. Eight old
  fixtures moved to explicit async functions or lambda-returned async blocks;
  existing inferred annotations remain unchanged. Four snapshot descriptions
  were refreshed without changing their result payloads.
- Extern audit reproduced lost async mode: a bodiless `async fn answer() Number`
  incorrectly exposed Number. Shared callable-result construction now adds one
  Task layer for both externs and trait signatures, before abort validation.
  A new inference snapshot and the CLI fixture's local Promise wrapper verify
  the Task result and AbortSignal delivery. The external JS file is maintained
  source, not generated compiler output.
- Two reviewed colorless rendered diagnostics cover await in a Task-annotated
  plain function and a lambda nested in an async function. Both point at the
  actual source and suggest explicit async bodies. Remaining driver/CLI/example
  fixture migration, test-entry semantics, and full acceptance gates remain open.
- Driver and CLI fixture migration now passes all 115 driver tests and all 11
  CLI tests. Three driver snapshots were reviewed: the top-level await diagnostic
  follows the explicit-body rule; the other two retain their original non-Task
  and non-Result type errors, with only explicit syntax added to the source.
  Removed their stale initial `.snap.new` files after review.
- Migrated async/extern/control-flow execution projects and examples/async.
  CLI coverage retains the 20,000-level recursive task success, suspended, and
  typed-error cases. `cargo run -p alder-cli -- run examples/async` succeeds.
  Tests use a tail `async { ... }` block, which the existing `$runTests` passes
  to `$runMain`. Removed the final await-inferred codegen branch for tests.
  Broader test-result validation, async capture/interface audits, primitives,
  bounded traversal, documentation audit, and packaging remain required.
- Current migration checkpoint: full `cargo test --quiet` (including doctests)
  exits 0, strict workspace Clippy exits 0, formatting and `git diff --check`
  pass, and no pending snapshots remain. This verifies the migrated suite,
  not completion of the broader hardening or concurrency requirements.
- Additional capture/interface audit: serialized a producer interface, dropped
  it, deserialized into a fresh consumer, and verified independently instantiated
  async identity calls plus Task[Task[Number]] preservation. A captured binding
  specialized by an async writer stays Number-only in the consumer; the rejected
  String call has a reviewed source-aware diagnostic and produces no interface
  or artifact. The focused driver test passes.
- Expanded CLI capture assertions pass: for-loop bindings are distinct per
  iteration and observe later rebinding, while-loop tasks share the outer counter,
  and record alias writes are visible to deferred reads. Updated language.md's
  primary async contract, including blocks, lambdas, capture, and entry points.
  Removed obsolete await-scanning helpers: execution mode has no inferred path.
- Execution boundary regressions also pass for `?` completing only an async
  block (including repeat execution), fresh arrays allocated inside reusable
  async function bodies, and zero-await async trait default methods. These
  extend the explicit_async standalone fixture without changing runtime ABI.
- Checkpoint validation rerun after capture/error-boundary coverage: strict
  workspace Clippy and full cargo test (including doctests) exit 0; driver tests
  now total 116. Formatting/diff checks pass, no pending snapshots remain, and
  the 56 existing parser snapshots were rechecked for only the added false flag.
  Release packaging and the remaining acceptance work are not claimed complete.
- Formatting and `git diff --check` pass. Full-workspace validation, rendered
  diagnostic snapshots, runtime/CLI coverage, and release gates remain pending.

## Explicit lazy async bodies

- Introduce both `async fn` and `async { ... }` using the same keyword.
- Calling an async function creates a lazy, reusable task. Constructing an async
  block likewise creates a task; neither starts the body.
- An async function's return annotation describes its completed value. `async`
  adds exactly one outer `Task`: `async fn load() Result[Data, e]` has callable
  result `Task[Result[Data, e]]`.
- No automatic flattening. An async function annotated `Task[Number]` produces
  `Task[Task[Number]]`; await the inner task explicitly to return its value.
- Plain functions execute immediately and may return existing task values.
  A plain function annotated `Task[a]` is not thereby an async body.
- `.await` requires an enclosing async body. Do not infer execution mode from
  the presence or absence of awaits. Async bodies remain lazy when they contain
  no awaits, including after refactoring.
- `return` and `?` inside an async block complete that task, not its enclosing
  function. `break`/`continue` cannot escape across the async boundary.
- Named methods use the same explicit async distinction as named functions.
- Do not introduce separate async-lambda syntax. Use an ordinary lambda whose
  body returns an async block: `url -> async { fetch(url).await? }`.
  If annotated, the lambda's return type is its actual `Task[...]` type, not
  the completed-value annotation used by `async fn`. The async block owns its
  awaits, returns, and error propagation; lambda invocation creates the task
  without executing the block. Add parser/type/codegen and CLI coverage for
  these lambdas, including use with bounded Fiber traversal.
- Keep main's declared execution mode explicit. The standalone launcher handles
  a synchronous main or executes its returned task in the root fiber; root
  fiber execution does not implicitly authorize awaits in plain functions.

## JavaScript-style capture and mutation

- Async blocks capture lexical bindings like ordinary JavaScript closures, not
  value snapshots. Rebinding before execution is observed by the task.
- Arrays and records retain shared-reference behavior. Repeated executions may
  observe different shared state; reusability does not promise purity.
- Preserve argument evaluation at call time for async function calls and body
  execution at task execution time. Do not defer evaluation of call arguments.
- This combines with the approved removal of `mut`: retain type-safe assignment
  and alias-aware generalization, not mutation permissions or borrowing.
- Cover scope lifetime, shadowing, loop captures, escaping/reused tasks, and
  reassignment of captured polymorphic bindings in tests.

## Explicit synchronization primitives

Introduce a small library foundation informed by the pinned Effect v4 source:

- `Ref`: shared cells with atomic synchronous state transformations.
- `SynchronizedRef`: serialized state transformations that may suspend.
- A cancellation-safe semaphore for bounded access to shared resources.

Atomic here concerns cooperating fibers in Alder's runtime, not cross-worker
shared-memory atomics. Lexical capture does not provide synchronization.
Protection applies only to operations using the primitive; an escaped mutable
payload is not magically protected from direct writes through other aliases.
Specify exact signatures and failure behavior before implementation. Test
lost-update prevention, interruption of waiters, permit release/finalization,
and failure during a suspended state transition. Do not port Effect wholesale.

### Semaphore implementation contract

Current ownership review: rechecked `Semaphore.ts` (`waitForPermits` and
`withPermits`) at the pinned Effect commit. Alder deliberately uses strict
weighted FIFO rather than Effect's availability-based observer traversal;
request ownership and scope joins replace the mask/onExit machinery. No source
was copied. Five focused kernel regressions pass, including a new gated test
that holds the permit through a forked child's cleanup on both normal parent
completion and interruption. This supplements direct-finalizer coverage; a
queued successor cannot enter until the child cleanup completes. Public
integration remains separate from this kernel checkpoint.

Initial public signatures: `make(permits: Number) Task[Semaphore]` and
`withPermits(semaphore: Semaphore, permits: Number, task: Task[a]) Task[a]`.
Both are lazy and reusable. Capacity and requested permits must be positive
safe integers; a request cannot exceed the fixed capacity. Invalid arguments
fail at execution with a RangeError defect, never an invented typed error row
or a permanently unfulfillable waiter. No resizing or manual acquire/release
API is exposed in this foundation.

Requests are FIFO, including weighted requests: smaller requests do not bypass
an older request waiting for more permits. Cancellation removes a waiting
request; cancellation after grant but before resumption releases its ownership
exactly once. The request's ownership record exists inside a generator's
try/finally before it is submitted, so no grant-to-cleanup-registration gap
exists. Waiters suspend their current fiber; no extra waiting fiber is created.

After acquisition, run the protected task in an owned scope. Hold permits until
that scope's children and finalizers have completed on success, typed Err,
defect, or interruption. Return values and defects are preserved; Err remains
ordinary data. Nested acquisition is not reentrant and can deadlock when a
task requests permits it already holds. This is cooperative coordination,
not cross-worker synchronization. Resource alias writes outside the protected
task remain unprotected. Reference: pinned Effect withPermits establishes
cleanup before restoring interruption. Alder uses explicit request ownership
and existing scope joins instead of Effect's mask/onExit instruction machinery;
no source is copied.

## Bounded Fiber traversal

Provide these distinct operations (schematic types, not final declarations):

| Operation | Callback result | Traversal result |
| --- | --- | --- |
| `Fiber.map` | `Task[b]` | `Task[Array[b]]` |
| `Fiber.forEach` | `Task[()]` | `Task[()]` |
| `Fiber.tryMap` | `Task[Result[b, e]]` | `Task[Result[Array[b], e]]` |
| `Fiber.tryForEach` | `Task[Result[(), e]]` | `Task[Result[(), e]]` |

- Support a concurrency setting, e.g. `{ concurrency: 8 }`. Default to
  sequential execution; unbounded concurrency must be explicit. Specify the
  configuration type and validation of invalid bounds before implementation.
- Bound active item operations and invoke each callback only when a slot opens.
  Do not create one waiting fiber per input and call that bounded scheduling.
- Construct traversal lazily; preserve input order in collected results,
  independently of completion order. Specify mutable-input iteration behavior
  rather than silently relying on incidental JavaScript iteration details.
- `forEach` does not collect an array of units and does not silently discard
  arbitrary result types. No boolean `discard` changing the return type.
- `map` treats each callback result as an ordinary value. If `b` is a Result,
  collect every success/error without cancelling siblings for an `Err`.
- `tryMap` and `tryForEach` explicitly interpret typed errors: stop on the
  first observed `Err`, stop scheduling new work, interrupt active children,
  and await their cleanup. Already completed side effects are not rolled back.
- Ordinary defects remain failures, not typed errors: clean up active children
  before completing a failed traversal. Do not silently inspect arbitrary
  payloads for `Err` in ordinary `map`.
- Parent cancellation stops new scheduling, interrupts children, and awaits
  cleanup in all variants, including error-collecting `map`.
- Preserve fairness, stack safety, scope ownership, provider context, and
  exactly-once completion. Test limits, ordering, empty inputs, callback throws,
  simultaneous completions/failures, and cancellation with bounded probes.

## All-settled distinction

`Fiber.map` over `Task[Result[a, e]]` is the agreed mechanism for collecting all
typed successes/errors. A broader settled-outcome API for defects and individual
interruption was discussed but is not approved for implementation yet. It would
need an explicit outcome type and must not defeat parent cancellation.

## Delivery gates

- Update SPEC, source/canonical ASTs, parser, inference, flow analysis, interfaces,
  direct AST lowering, runtime/stdlib, formatter, diagnostics, docs, and examples
  wherever affected. Migrate inferred-async examples and tests.
- Keep the original hardening invariants and regression cases; adapt their
  syntax without removing the failures they were designed to detect.
- Add source-aware, colorless diagnostic snapshots and actual CLI/cross-module
  execution tests. Validate typed error behavior and nested task representation.
- Revisit the exact Effect reference in `docs/effects-internals.md`, record
  deliberate divergences, and preserve attribution for adapted code.
- Run formatting, strict Clippy, full tests, affected release packaging, and
  changesets. These decisions are not implemented merely because recorded here.
