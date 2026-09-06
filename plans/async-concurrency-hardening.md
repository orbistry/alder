# Async and concurrency decisions

Status: explicit async, synchronization, and traversal kernels implemented;
public unbounded configuration is implemented; final integration/release gates remain open.
The user has approved `Fiber.unbounded` as the concurrency-limit value, used as
`{ concurrency: Fiber.unbounded }`, not as a whole options record.
The implementation now includes a monomorphic Number
annotation, kernel constant, and built-in bundle export. A CLI regression first
failed with unknown-name and now passes, including gated concurrent starts and
all four traversal APIs. Kernel coverage checks peak concurrency using the
exported value for every adapter. Final package verification remains required.
This extends compiler hardening, not the later web/data/provider milestones.
Current acceptance evidence is mapped in `docs/async-hardening-acceptance.md`.
Test-entry follow-up: compiled synchronous Err, asynchronous Err/defect, and
later asynchronous success are covered by a dedicated CLI fixture. Its exit
status is 1; a separate kernel probe verifies the three-failure count, report
contents, and continued execution in order. All 59 kernel and 16 CLI tests and
associated doctests pass; no production change was needed. The language test
example now uses the explicit async block required for await. This does not
complete the named unbounded configuration implementation or final integration gates.
The incremental checkpoint log below includes superseded implementation states;
inferred async and absent primitives are historical, not current behavior.

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

Public wiring checkpoint: Ref, Semaphore, and SynchronizedRef now have committed
opaque type/module registration, packaged stdlib signatures, direct bundler
export mappings, user documentation, and per-crate changesets. Three positive
inference probes are included. Focused inference (including the existing
synchronous-callback/alias rejection probe), packaged-signature/source-copy
checks, and bundle tests pass. The two copies of each stdlib source are byte
identical. This checkpoint deliberately excludes traversal options and the
remaining cross-module/value-restriction acceptance work; it does not establish
that the whole hardening branch is ready to merge.

Cancellation prerequisite reviewed and committed as `36e654e`: suspension
ownership is installed before registration, reentrant interruption waits for
its cleanup hook, and Scope/All/Race/partial-construction cancellation joins
owned cleanup before resuming the caller. Four bounded regressions are included.
The pinned Effect callbackOptions/asyncFinalizer invariant was rechecked; no
source was copied. All 53 current kernel tests and kernel doctests pass. The
semaphore and synchronized-cell implementation remains a separate checkpoint.

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
integration remains separate from this kernel checkpoint. Committed as
`2cdb02a`. Current full workspace tests and doctests pass (54 kernel tests,
426 solver integration tests; two existing doctest ignores), as do formatting,
strict all-target/all-feature Clippy, and diff checks. No pending snapshots were
found. This validation is of the current worktree, not a claim that all pending
hardening changes have been committed or that the branch is ready to merge.

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

Kernel checkpoint: added SemaphoreAcquire suspension, a FIFO weighted waiter
map, and per-execution ownership records established before acquisition. The
scoped wrapper releases synchronously in finally, after the scope join, and
release is idempotent across cancellation-hook and generator cleanup paths.
The initial test failed because the API was absent; both new bounded tests now
pass. Coverage includes capacity limits across 12 tasks, finalizer-held permits,
lazy/fresh allocation, task-factory defects, ordinary Err values, invalid
capacities, cancellation while queued and after grant before resumption, and
removing a head waiter without stranding a smaller successor. All 23 kernel
tests pass. Compiler/stdlib exposure, invalid request counts, cancellation during
protected finalization, provider inheritance, fairness under queued handoffs,
and release packaging remain required; this kernel slice is not completion.

Source-level checkpoint: registered Semaphore's opaque zero-argument type and
module, packaged both stdlib declarations, and added direct bundler exports.
The positive solver test first failed on missing Semaphore; it now verifies
independently typed protected results. The CLI fixture runs three reusable
protected tasks against a one-permit gate and checks exclusive access through
suspending scope finalizers. Both tests pass, as do all three packaged-builtin
checks. Invalid request counts now have kernel coverage proving no body starts,
no waiter remains, and capacity stays intact. Added public documentation and a
Sampo changeset. Cancellation during finalization, provider context, fairness,
cross-module/diagnostic coverage, full validation, and packaging remain pending.

Semaphore lifecycle/fairness probes: added a gated finalizer test proving
interruption retains the permit until cleanup completes, repeated interruption
does not release early, and a queued successor runs only after cleanup. A
four-worker, 2,048-handoff probe checks host timer progress by operation count
and provider context inheritance/isolation across the protected scopes. All
four focused semaphore tests pass with ten-second safety timeouts; no wall-clock
performance threshold is asserted. Cross-module/diagnostic gates and broader
validation remain required. SynchronizedRef and bounded traversal are still
unimplemented, so this evidence does not complete the concurrency amendment.

### SynchronizedRef implementation contract

Current kernel review: rechecked pinned Effect `modifyEffect`/`updateEffect`.
The implementation reads under the write permit, invokes the lazy callback,
then commits only after its task returns normally. The three kernel tests
cover 32 serialized suspended updates, queued set/update order, interruption
on either side of commit, reads while the write lock is held, shared payload
aliases, and post-commit cleanup defects. Alias mutations and completed commits
are intentionally not rolled back. This review does not close public API,
interface, or final release acceptance. Kernel operations and these tests are
committed as `1e9bb8c`. The focused tests and strict Clippy were rerun and passed;
the full workspace/doctest pass recorded for `2cdb02a` covers this same production
source state (selective staging did not modify the worktree source).

Initial API: `make(a) Task[SynchronizedRef[a]]`,
`get(SynchronizedRef[a]) Task[a]`, `set(SynchronizedRef[a], a) Task[()]`,
`update(SynchronizedRef[a], fn(a) Task[a]) Task[()]`, and
`modify(SynchronizedRef[a], fn(a) Task[(b, a)]) Task[b]`.
All operations are lazy/reusable. Each make execution creates a fresh cell and
one-permit semaphore while preserving its argument's alias identity.

All writes use that semaphore. Invoke an update callback only after acquiring
the permit, reading the latest stored value then. Commit its replacement after
the callback task completes normally; synchronous throws, task defects, or
interruption before commit leave the stored binding unchanged. Hold the permit
through the protected scope's cleanup. A failure or interruption after commit
does not roll it back, nor are prior alias mutations transactional.
Reads do not acquire the permit: get returns the last committed binding even
while a transformation is suspended. This allows a callback to read its own
cell without deadlocking; a nested write to the same cell is non-reentrant.

Result payloads are ordinary values. To return a typed failure without changing
state, modify can complete with `(Err(error), oldValue)`; it does not implicitly
interpret Err or invent a checked-error channel. This choice preserves Alder's
Result semantics. Ref.update remains synchronous; SynchronizedRef.update's
callback explicitly returns a task. Both opaque cell types remain invariant.

Rechecked pinned Effect SynchronizedRef.ts get/modifyEffect at commit
bd393d63c19bdd0ab212d95576cec89051c8501c. Alder uses scoped semaphore protection
and its existing generator frames; no reference source is copied. Effect's
separate checked-error channel is not reproduced.

Kernel checkpoint: implemented all five operations using the existing Ref read
and one-permit scoped semaphore for writes. The test first failed on absent
operations, then passed with 32 suspending updates preserving every increment,
lazy callbacks, fresh allocations, unchanged bindings on synchronous/suspended
defects, modify returning an ordinary Err without changing state, and set/get
after failure. All 26 kernel tests pass; formatting/diff checks pass. Source
exposure, cancellation before/after commit, reads during suspension, queued
set/update ordering, alias/value-restriction/interface probes, and release
validation remain required. This is a kernel-only checkpoint.

Source checkpoint: added SynchronizedRef's opaque type/module, five declarations
in both stdlib copies, and bundler mappings. The initial positive solver probe
failed on missing type/module registration; it now passes alongside negatives
for a synchronous callback and incompatible shared-array alias. Compiled CLI
coverage passes for three reused suspending increments, a suspending modify
with distinct returned/replacement values, and set/get. All three packaged
builtin checks pass; formatting/diff checks pass. Added public documentation
and a Sampo changeset. Cancellation/commit boundaries, cross-module inference,
source-aware diagnostics, and full release validation remain pending.

SynchronizedRef lifecycle audit: a gated two-case regression checks interruption
before commit and during post-commit finalization. Reads complete with the last
committed value while the lock is held; queued set/update operations do not
start until cleanup, preserve FIFO ordering, and observe the preceding write.
Both paths clean up once and restore the permit. A separate alias probe checks
callback self-reads, shared payload identity across fresh cells, alias mutations
surviving a failed update, and post-commit finalizer defects preserving committed
state while releasing the lock for later writes. All 28 kernel tests pass;
formatting and diff checks pass. These are bounded runtime properties, not a
substitute for pending cross-module/source-diagnostic and release gates.

Stored synchronization checkpoint: serialized and dropped a producer containing
generic SynchronizedRef factories, a Semaphore factory/protect wrapper, and an
inferred shared array payload constrained by a separate writer. A fresh consumer
accepts independently typed cells/protected tasks; a String write to the shared
Number payload fails without publishing interfaces/artifacts. A separate driver
regression rejects a synchronous update callback. Both source-aware colorless
snapshots were reviewed: the callback correctly reports Task[Number] versus
Number. The shared-write diagnostic's initially reversed direction was corrected
by the later argument-context work; current verification is recorded below.
Both focused tests pass; formatting/diff checks pass. Full driver
and workspace validation, packaging, and remaining hardening are still required.

### Ref implementation contract

Kernel commit review: rechecked the five operations and three Ref regressions
against the pinned Effect Ref.ts make/get/set/modify/update implementation
(`bd393d63c19bdd0ab212d95576cec89051c8501c`). The kernel-only checkpoint is now
committed as `667d9be`, with its own changeset; public stdlib/type registration
and bundler mappings remain in the coordinated compiler worktree. All six
Ref/SynchronizedRef-filtered kernel tests pass in the current tree. The full
workspace and fresh package validations also pass for this source state.

The initial API is `make(a) Task[Ref[a]]`, `get(Ref[a]) Task[a]`,
`set(Ref[a], a) Task[()]`, `update(Ref[a], fn(a) a) Task[()]`, and
`modify(Ref[a], fn(a) (b, a)) Task[b]`. The modify pair is (returned value,
replacement state). Ref is an opaque invariant mutable cell; no structural
access to its storage is exposed to Alder. Generalization must not allow one
allocated cell to be instantiated at incompatible payload types.

All operations are lazy and reusable. Each execution of make allocates a fresh
cell; its argument retains ordinary call-time evaluation and aliasing. Reads
observe execution-time state. Update/modify invoke their synchronous callback
exactly once per execution and commit only after it returns successfully.
No suspension or fiber scheduling occurs between reading and committing.
Callback defects propagate unchanged without replacing the stored binding;
mutations to an aliased payload are not transactional and cannot be rolled back.
Typed Result values are ordinary data, not an implicit failure channel. A Task
can likewise be stored as data; synchronous callbacks are never auto-awaited.
Cancellation before execution prevents the operation, while an operation that
has already committed is not rolled back by later cancellation.

Reference inspected: Effect v4 Ref.ts make/modify/update at pinned commit
bd393d63c19bdd0ab212d95576cec89051c8501c. Alder uses its existing task frames,
not Effect.sync or MutableRef, and exposes no unsafe eager constructor. No
reference source is copied. Compiler registration, source-level typing,
interfaces, CLI coverage, and remaining primitives are separate required gates.

Kernel-only checkpoint: implemented the five operations above and a V8 runtime
regression proving lazy callbacks, repeat execution, independent allocations,
execution-time reads, unchanged state after callback defects, modify pair order,
and 1,600 updates across 16 cooperating fibers without lost updates. The focused
`alder-kernel` test passes; formatting and diff checks pass. This is not yet an
available Alder stdlib API, and cancellation/alias probes plus compiler and
release validation remain pending.

Compiler/CLI checkpoint: registered the opaque builtin Ref type and added all
five signatures to both audited stdlib copies. The positive inference test first
failed with unknown Ref/type members; it now passes, including independent
generic allocation and modify's separate result type. A negative shared-array
cell/alias test confirms incompatible payload use produces a type mismatch.
The real CLI fixture initially exposed missing Ref module exports in the
bundler; added explicit kernel export mappings and reran successfully. It now
executes fresh allocation, lazy repeated updates, modify/get/set, and concurrent
updates through compiled Alder. Updated SPEC's stale inferred-async description
and added a Ref changeset. Cross-module cell contracts, rendered diagnostics,
cancellation/alias probes, full validation, and release gates remain required.

Ref follow-up evidence: three focused kernel tests pass, including a bounded
interruption test before task execution and after committed update/suspension.
Repeated interruption runs cleanup once; committed state remains. Alias probes
confirm independent cells can share the original payload, callback mutations
before a defect are not rolled back, and Task/Err payloads remain ordinary data.
A serialized/dropped/rehydrated producer interface supports independent generic
Ref allocations and preserves an explicitly Number-array shared contract. Its
invalid String write fails without publishing an artifact/interface; reviewed
the colorless source-aware diagnostic snapshot. At this historical checkpoint
the mismatch direction was reversed and labeled the whole call. The later
argument-context work corrected both issues, as verified below. This test does
not prove every inferred escaping-cell case.

Inferred-cell audit: five focused solver tests pass. New negatives retain
monomorphism for a function cell captured by a lambda and for a reusable
allocation task whose array argument is shared across executions. A positive
inferred async factory allocating its array inside the body supports independent
Number/String cells. Strengthened the stored-interface producer to infer its
shared payload contract from a separate writer, removing the explicit return
annotation; the positive/negative consumer and unchanged rendered snapshot pass.
All 18 kernel tests pass, including fairness, stack safety, and cleanup probes.
Documented the public cell operations and allocation/alias distinction in
language.md. Full workspace tests remain non-green due to the separately tracked
tuple projection regression; semaphore/SynchronizedRef/traversal work remains.

## Bounded Fiber traversal

Current defect review: ordinary map/forEach previously only stopped new work
when a callback defect was observed. Active siblings were interrupted only after
the failing item's scope completed cleanup. A gated regression failed with
"Top-level await promise never resolved": failing-item cleanup awaited a gate
that was released only after sibling interruption. All traversal modes now
register active workers, record the first defect before interrupting siblings,
and join the workers before rethrowing that defect. Stop-induced sibling
interruption cannot replace the originating defect; parent cancellation still
escapes the coordinator's join. The bounded regression covers map and forEach.
The current validation pass completed successfully: all 55 kernel tests, the
full workspace tests and doctests (two existing ignores), formatting, strict
all-target/all-feature Clippy, and diff checks. No pending snapshots were found.
Fresh release packaging remains required after this runtime change.

Reference review: rechecked pinned Effect `forEach`, `iterateEagerImpl`, and
`forEachConcurrent` at bd393d63c19bdd0ab212d95576cec89051c8501c. Alder uses fixed
worker tasks and per-item scopes, not Effect's eager-exit optimization or full
Cause representation. Ordinary Result values remain data; only tryMap and
tryForEach select typed errors. No source was copied. Public options and their
unbounded spelling remain a separate checkpoint.

Traversal implementation decisions: take a shallow array snapshot at each task
execution, before invoking callbacks. Mutations before execution are observed;
later array replacement/push/removal does not change that execution's items.
Payload objects retain alias identity. Snapshotting is ordinary synchronous
bulk work, not preemptible by the cooperative scheduler. Each execution gets
fresh result storage and scheduling state.

Shallow-snapshot regression: all four kernel traversals now run an object-payload
probe with limits one, two, and Infinity. Replacing/pushing input members during
the first callback does not change that execution's membership; changing a
payload remains visible through its original alias. Reusing the same task sees
the new input membership, allocates independent collected output, and does not
overwrite the previous output. The focused test and all 51 kernel tests pass;
no runtime implementation change was needed. This is kernel evidence only:
public stdlib traversal bindings, optional configuration adapters, and their
CLI/interface coverage remain unfinished. Record-presence equivalence and the
public spelling of explicit unbounded configuration still await an unambiguous
user answer; the latest bare “Yes” was not assigned to either question.

The kernel accepts an explicit normalized limit: positive safe integer or
Infinity for deliberate unbounded execution, defaulting to one. Reject invalid
limits with a RangeError defect instead of clamping. The user has now approved
trailing optional parameters and the public order
`(values, callback, options?: MapOptions)` for all four traversal operations.
Omission means sequential execution; unwrap optional options in the public
adapter before normalizing the kernel limit. This supersedes the earlier pending
map/mapWith choice. See `plans/hardening-language-decisions.md` for the approved
parameter semantics and call examples; implementation remains pending.

Public declaration prerequisite: reproduced packaged signature loading rejecting
a source-local `Options` alias as unknown. The loader now uses canonical headers
to install local alias definitions before checking public function signatures.
Its regression covers a public alias referring forward to a private generic
record alias, payload substitution, retained alias identity, and an optional
function parameter. Modules with no aliases keep their existing loading path.
This does not yet expose qualified builtin type names to callers: resolving
`Fiber::MapOptions`, publishing the traversal signatures and bridges, and testing
their stored interfaces and CLI execution remain required next steps. No public
record-presence or unbounded-configuration decision is inferred by this change.
Validation: all 81 alder-can tests pass, including the packaged-source parity
and signature tests; alder-can all-target/all-feature Clippy with denied warnings,
workspace formatting checks, and diff checks pass. The full workspace suite was
not rerun for this declaration-loader checkpoint.

Qualified builtin alias follow-up: `std/Fiber.ald` and its packaged copy now
declare `pub type MapOptions = { concurrency?: Number }`. Qualified type lookup
loads public canonical stdlib headers lazily, caches them in the environment,
and expands aliases using the existing substitution machinery. Lookup checks
the builtin package identity, not merely a matching module path. Alder's actual
qualified type syntax is `Fiber::MapOptions` (not dot access).
Inference accepts omitted/numeric fields and rejects a String field. A driver
test serializes a dependency exporting a Config alias and read function, drops
the original interface, and checks positive and negative consumers after reload.
Its reviewed colorless diagnostic labels the incorrect String literal in the
consumer source. Traversal functions and runtime bridges are still unfinished;
the public type declaration does not establish their runtime availability.
Validation for this follow-up: 82 canonicalizer tests, 9 solver unit tests,
390 inference tests, and 144 driver tests pass. All-target/all-feature Clippy
with denied warnings passes for those three crates. Formatting/diff checks pass,
and there are no pending snapshots. Full workspace, CLI traversal execution,
and packaging gates remain outstanding for this change.

Public traversal checkpoint: all four declarations now use the approved
values/callback/optional-MapOptions order. Bundle exports route through maintained
kernel adapters which unwrap Option, read an own concurrency field once at each
execution, and call the existing normalized worker pool through the stack-safe
task protocol. Omission/None/empty records select one. The new kernel probe first
failed on the absent adapters, then passed lazy reads, changed limits on reuse,
sequential defaults, invalid-bound rejection before callbacks, and Infinity.
Revisited the pinned Effect forEach suspension/normalization and concurrent
ordered-output code; no code copied, and Alder retains strict validation and
typed Result semantics.

The explicit_async CLI fixture now executes all four APIs, including bounded
active callbacks and per-item cleanup, repeated tasks with changed input,
sequential forEach, omission/None/empty options, direct destination awaits in
pipes, ordinary Result collection, and sequential typed fail-fast behavior.
Two reviewed diagnostic snapshots reject synchronous map callbacks and
non-unit forEach callbacks. Named public unbounded configuration, traversal
stored-interface coverage, more source-level cancellation/failure cases, full
workspace validation, and packaging remain required; the existing kernel
cancellation tests alone do not establish those source-level gates.

Validation at the public checkpoint: 52 kernel tests, 82 canonicalizer tests,
146 driver tests, 390 inference tests, 9 solver unit tests, and all 12 CLI tests
pass in the workspace run, including the piped `tryMap(...).await?` case.
The remaining workspace unit/integration suites also pass. Workspace
all-target/all-feature Clippy with denied warnings, formatting, and diff checks
pass; no pending snapshots remain. The full `cargo test -- --quiet` command
completed with exit zero, including doctests (the existing driver, parser, and
runtime doctests remain ignored). Packaging and the remaining acceptance work
are not claimed done.

Traversal boundary follow-up: a stored-interface regression exports factories
returning each of the four builtin traversal function values, serializes and
reloads their interfaces, and checks consumers. Independent Number/String
instantiations, omitted/None/record options, and typed error rows survive reload;
the reviewed negative snapshot still rejects a Number-completing forEach
callback. This checks inferred higher-order contracts, not just source wrappers
with explicit annotations.

The explicit_async CLI fixture now interrupts all four public traversal APIs
under concurrency two while both worker finalizers are held behind a release
flag. A separate readiness counter ensures finalizer registration has completed
before interruption. The test observes interruption still pending, zero finished
cleanups, and only two started callbacks; after release, interruption completes
with exactly two cleanups and no additional callbacks. The CLI test's existing
30-second timeout bounds a regression without relying on elapsed-time assertions.
The final readiness-gated fixture passes, all 147 driver unit tests pass, and
driver all-target/all-feature Clippy with denied warnings passes. Formatting and
diff checks pass. No compiler/runtime implementation change was needed in this
follow-up; the full workspace run from the preceding checkpoint was not rerun.
Named unbounded configuration and remaining acceptance audits still remain open.

Typed-failure CLI follow-up: both tryMap and tryForEach now run a gated test
where input one returns `Err(:later)` while input zero remains suspended. Both
callbacks first register finalizers and signal readiness. After enabling the
failure, the test waits for both finalizers to enter, verifies that the traversal
has not returned and no queued inputs started, then releases cleanup. Joining
the traversal observes exactly the selected error, two completed finalizers,
and only two started callbacks. This covers first-observed error selection and
sibling cleanup through the compiler/bundler/runtime boundary, under the
fixture's existing 30-second timeout. The focused CLI test passes; no runtime
change was necessary. Full workspace validation from the earlier public API
checkpoint remains the latest full run, not a rerun after this fixture addition.

Rechecked pinned Effect internal/effect.ts forEach and forEachConcurrent at
bd393d63c19bdd0ab212d95576cec89051c8501c. Preserve lazy enumeration, ordered
output, and bounded callbacks; deliberately reject invalid bounds rather than
Effect's clamping and retain distinct public discard/error-propagating APIs.
No source is copied. Kernel map work can proceed independently of public
configuration syntax; all four required variants and their cancellation gates
remain in scope.

Initial kernel map checkpoint: implemented a bounded worker pool, per-item scope
join, execution-time shallow snapshot, and ordered result slots. Only the worker
limit is allocated upfront, not one waiting fiber per item. A regression first
failed on the absent API, then passed for lazy callbacks, maximum two active
items, cleanup before reuse, repeat execution with changed input, and ordinary
Err collection. All 29 kernel tests pass; formatting/diff checks pass. This does
not yet prove out-of-order completions, callback failure scheduling, cancellation,
or fairness. No public map signature is exposed while configuration spelling is
pending, and forEach/tryMap/tryForEach remain required implementation work.

Map failure audit: reproduced a healthy worker consuming more input while a
failed callback's finalizer remained gated. Added shared stop state at the
callback throw boundary and checked it both before claiming and before invoking
an item; scope-exit failures also stop the pool. The gated regression now passes
without weakening cleanup ordering. A separate parent-interruption probe checks
only two callbacks start under limit two, both suspending cleanups finish before
caller cleanup, repeated interruption remains idempotent, and no children remain.
All three focused map tests pass; the preceding full kernel run passed 30 tests.
Out-of-order completion, fairness, invalid limits, and all remaining variants
still need acceptance coverage.

Map ordering/limits/fairness checkpoint: six focused map tests now pass.
Gated completions deliberately finish indices 1,2,3,0 while retaining output
order 0,1,2,3 and admitting no extra item until a slot opens. Other probes verify
empty input invokes no callback, invalid normalized limits fail before callbacks,
default concurrency peaks at one, explicit Infinity permits all three test
items, and a host timer progresses before 1,024 of 2,048 immediately completing
items. No wall-clock performance threshold is used. Formatting/diff checks pass;
forEach/tryMap/tryForEach and public signature choice remain outstanding.

Unit traversal checkpoint: added a distinct kernel forEach entry sharing the
bounded worker loop, with no item-result allocation. An internal AllDiscard
join reuses all's ownership/failure/cancellation machinery without allocating
worker-result storage either; ordinary Fiber.all still collects results.
The public API has no boolean discard option. The first test failed on the
absent function; it now verifies lazy execution, bounded active work through
finalizers, unit completion, empty inputs, and rejection of non-unit results
including Err data. All 35 kernel tests pass, preserving map ordering/failure/
cancellation and existing all/race tests. Formatting/diff checks pass. Dedicated
forEach cancellation/diagnostic gates and both typed-error variants remain open.

Typed traversal kernel checkpoint: added tryMap/tryForEach to the bounded pool.
The first regression failed on the absent entry point; it now proves an Err
at index one interrupts a suspended index-zero sibling before the error item's
gated finalizer completes. The traversal remains pending until cleanup, returns
the original Err identity, starts no later callbacks, and detaches all children.
Success collection preserves input order; tryForEach requires Ok(unit) and does
not collect unit arrays. A separate bounded probe covers both variants' parent
interruption, exactly-once cleanup, malformed Results, and empty inputs.

Typed stop state is distinct from a runtime defect. The coordinator interrupts
registered worker fibers immediately, suppresses only its own stop-induced
worker interruption, and still joins them through AllDiscard. Parent cancellation
escapes that join unchanged. Observed defects take precedence over a selected
typed Err; this does not introduce a Cause/all-settled API or change the existing
scope policy for failures during interruption cleanup. Revisited the pinned
Effect forEachConcurrent implementation; no source was copied. All 37 kernel
tests pass. Public configuration/signatures, simultaneous failure and cleanup-
defect probes, typed traversal fairness, CLI/interface coverage, changesets,
packaging, and full hardening validation remain open.

Typed traversal follow-up: four additional bounded regressions pass. Parent
interruption during an already-selected Err's gated finalizer still waits for
cleanup, publishes the parent's original interruption, and notifies its observer
once. A selected-error item's failing finalizer produces a defect, not an Err;
falsy thrown values (including undefined) also remain runtime failures and stop
new callbacks. Both typed variants yield to a host timer within bounded operation
counts over 2,048 items, with immediate and immediately resolved Promise bodies.
All four traversal variants isolate provider context per item, including when
a prior item deliberately leaves a context installed; finalizers retain their
own item's context. All 41 kernel tests and strict kernel Clippy pass, as do
formatting and diff checks. Competing typed failures, deferred interruption,
public source-level exposure, and the original full acceptance remain open.

Reentrant registration finding: while auditing interruption boundaries, a new
Promise probe reproduced an uninstalled suspension cancellation hook when an
extern synchronously interrupted its running fiber. Abort was missed and late
settlement could still reach the active waiter. Suspension now installs ownership
before invoking registration, invalidates resume immediately on interruption,
and waits for registration to provide cleanup. Extending the probe to synchronous
throws/malformed returns reproduced a second missing-abort path; the Promise
adapter now returns its abort hook on those paths too. Resolve, reject, pending,
throw, and malformed cases pass with original interruption identity, exactly one
abort/finally, no continuation, and no remaining children. All 42 kernel tests
and strict kernel Clippy pass. Revisited pinned Effect tryPromise/callbackOptions
and its asyncFinalizer invariant; no source copied. This closes the demonstrated
registration gap, not deferred interruption or the full extern/runtime audit.

Deferred-interruption and competing-error evidence: two new bounded regressions
pass. Wrapping each of the four traversal variants in uninterruptible retains a
parent interruption while item work/cleanup completes, permits observation of
the traversal value inside the mask, and delivers the original interruption
when the mask is removed. The typed variants' Err does not erase that request.
Two simultaneously settled typed errors are selected in observation order for
both possible input-index orders, even when the selected item's finalizer is
gated; no later callback starts and all children detach. All 44 kernel tests
pass. Public API configuration/exposure, CLI/interface tests, release packaging,
and the original compiler hardening requirements remain unfinished.

Workspace validation checkpoint after registration/traversal probes:
`cargo clippy --all-targets --all-features -- -D warnings` exits 0 and formatting
passes. `cargo test --quiet` exits 101 at the existing
tuple_projections_accumulate_before_fixing_arity regression (279/280 solver
integration tests pass). Earlier suites in that run include all 11 CLI, 119
driver, 14 formatter, 44 kernel, and 1,304 parser tests passing. Tests after
the failing suite/doctests are not claimed rerun. No pending snapshot files
were found. The tuple failure remains required work, not an accepted snapshot.

Semaphore prerequisite finding: inspected the pinned Effect Semaphore.ts
waitForPermits/withPermits and SynchronizedRef.ts modifyEffect at
bd393d63c19bdd0ab212d95576cec89051c8501c. Acquisition must establish cleanup
ownership before interruption can resume the caller; a protected operation must
finish its owned cleanup before releasing a permit. Alder's existing Scope
cancellation hook interrupted its child but did not await its exit. A bounded
regression reproduced caller cleanup preceding a child's suspending cleanup.
Suspension cancellation hooks now propagate optional asynchronous cleanup to
interruptUnsafe, and Scope returns its child's exit Promise. The regression and
all 19 kernel tests pass. No Effect source was copied. Audit all/race cancellation
hooks for the same ordering contract before building protected operations;
semaphore signatures, bounds/failure policy, waiter handoff tests, and actual
implementation remain pending.

Combinator follow-up: a bounded four-case probe reproduced caller cleanup
running before child cleanup under parent interruption of all/race. It covers
both pending work and interruption during cleanup after a selected all failure
or race winner. The cancellation hooks now return a join of every owned child;
removed the incorrect settled guard, since selected does not mean cleaned up.
The existing suspension barrier delivers interruption only after that join.
All 20 kernel tests pass, including repeated interruption and empty child sets
at terminal completion. The child-construction-failure suspension remains a
related cancellation path requiring its own probe; public join/interrupt wait
semantics and late finalizer registration also require separate audit.

Partial-construction follow-up: reproduced cancellation bypassing the existing
child-exit join after a later task factory throws. A bounded white-box test
attaches a gated finalizer to an already-owned, unstarted child, interrupts the
parent while that cleanup is pending, and checks both all/race. The failure
suspension now returns the same cleanup Promise from its cancellation hook.
All 21 kernel tests pass: caller cleanup observes zero attached children, the
child finalizer completes first, and partially constructed bodies never run.
Formatting and diff checks pass. This closes the demonstrated construction
path, not the remaining semaphore implementation or full runtime audit.

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
