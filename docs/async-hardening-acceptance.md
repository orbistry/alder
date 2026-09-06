# Async hardening acceptance map

Evidence from `42c5423` plus the integrated hardening worktree, not an isolated
validation of that commit or final release approval. Historical incremental
checkpoints remain in `plans/async-concurrency-hardening.md`; references there
to absent primitives, inferred async, or the formerly failing tuple test do not
describe the current implementation.

## Explicit-async migration reconciliation

The migration's compiler and local package boundaries are verified in the
integrated worktree. SPEC declares optional `async` on named functions and
a distinct `async` block expression. Canonicalization grants await permission
only at those explicit boundaries and restores outer control state afterward;
ordinary lambdas reset that permission. Inference constructs one Task layer
from the explicit flag, and lowering wraps only explicit async bodies rather
than scanning ordinary functions for awaits. Stored interfaces retain the
resulting Task types, including nested layers, rather than needing a caller-side
execution-mode inference rule.

The current full workspace pass covers migrated fixtures, source-aware boundary
diagnostics, stdlib declarations, and codegen snapshots. Fresh package validation
passes all 17 crates; its CLI executes the async example, explicit_async fixture,
original deep-await/local-extern probes with current syntax, and both successful
and failing async test suites. See `release-packaging-hardening.md` for paths
and exact results. Public prose consistently describes explicit lazy execution;
the ABI wording now distinguishes explicit async factories from ordinary
functions that return an already-constructed task.

This closes the explicit-async migration checklist item, not the independent
named unbounded traversal decision, cross-feature constraint audits, or final
clean committed-tree validation.

## Explicit task boundaries

Source and canonical functions carry explicit async mode; async blocks have
their own expression and return/control-flow boundary. Plain Task-returning
functions do not gain await permission. Solver signatures add exactly one Task
layer, and direct AST lowering creates lazy generator factories. Lambdas remain
ordinary functions returning async blocks, without a separate async-lambda form.

Parser, canonicalizer, solver, and codegen regressions cover zero-await functions,
nested tasks, methods, block-local returns, and permission isolation. Driver
`stored_async_contracts_preserve_layers_and_captured_state` checks serialized
contracts and captured replacement restrictions. The `explicit_async` CLI fixture
executes call-time arguments, reusable lazy bodies, lexical/loop captures,
independent block returns and propagation, nested tasks, and async methods.

## Promise cancellation follow-up

The late-rejection audit reproduced an internal mapper running after its fiber
had finished interruption. `resume` already ignored stale results, but the
mapper ran before reaching that guard. Suspension now gives registration an
active-waiter predicate; Promise rejection checks it before calling foreign
mapping code. The rejection handler remains attached, so late rejection is
observed without side effects from the mapper. This does not add a public
rejection-mapping API or change typed Result fulfillment.

`cancelled_promise_does_not_run_a_late_rejection_mapper` failed before the fix
and passes afterward. It checks interruption identity, one abort, one cleanup,
zero continuations, and zero late mapper calls. The existing reentrant
registration regression also checks zero mapper calls across immediate
fulfillment/rejection, throws, malformed returns, and pending Promises.

The pinned Effect `tryPromise`/callback implementation was re-read at
`bd393d63c19bdd0ab212d95576cec89051c8501c`. This guard is Alder's explicit
cancelled-waiter invariant, not a copied Effect implementation or a claim that
Effect guards mapper execution the same way. Fresh package verification after
this change passes all 17 publishable crates; the extracted kernel also passes
the cancellation and exception harnesses. Exact paths and scope are recorded in
`release-packaging-hardening.md` at the `42c5423` integrated checkpoint.

## Test entry point evidence

The `test_failures` CLI fixture compiles four tests: synchronous typed Err,
an async-block typed Err after suspension, an async-block defect, and a later
async success. The observed output reports three failures and one success;
the bundled test entry returns process exit status 1. The CLI regression checks
that exit status, not the internal failure count.

The separate kernel regression
`test_runner_counts_failures_and_continues_after_async_exits` captures reports
and execution events. It checks all four bodies run in order, all report kinds,
the exact `1 passed; 3 failed` summary, and `$runTests` returning three failures.
This distinguishes failure counting from the launcher's boolean exit status.
No runtime or compiler change was needed. The language test example now puts
its await inside an explicit async block, matching the implemented boundary.

## Lifecycle audit reconciliation

The related lifecycle audit is complete for the specified contracts, using
current source review plus the permanent bounded regression matrix. This is
not a proof for arbitrary foreign code or whole-goal release approval.

- Explicit async representation: canonical await permission is scoped to named
  async bodies and async blocks; ordinary lambdas reset permission. Inference
  wraps the declared completed type in exactly one Task, and direct AST lowering
  uses the explicit flag. Stored contracts and compiled fixtures cover those
  boundaries, as mapped above.
- Promise failure: `handlePromise` contextualizes synchronous throws and
  malformed returns as foreign defects; active mapped rejections may return
  ordinary values, and mapper exceptions preserve origin/cause as defects.
  `promise_mapper_failure_preserves_origin_cause_and_cleanup` and
  `promise_then_getter_failure_is_a_defect_not_a_mapped_rejection` cover the
  exception branches and exactly-once cleanup. Cancellation and late settlement
  have the independent regressions described above.
- Completion/observers: `beginClose` and `complete` guard terminal transitions;
  completion removes parent ownership, snapshots and clears observers before
  notification, and resolves the exit once. The lifecycle regression checks
  reentrant observation, immediate child completion, and hostile thenables.
- Ownership/combinators: children are registered with their parent before
  starting. Partial construction failures drive registered children to an exit.
  All/race failure and interruption remove observers, interrupt children, and
  wait for cleanup before resuming. Dedicated scope, partial-construction, and
  selected-exit cancellation tests cover these paths, in addition to the
  lifecycle regression's ordered results and loser cleanup checks.
- Finalizers: scope closure joins children, drains a LIFO list including entries
  added while closing, and keeps draining after a finalizer defect. Closed-scope
  registration executes immediately with the stored exit. Finalizers run masked;
  pending interruption after an explicit mask is removed remains covered by the
  traversal and deep-unwind tests. The lifecycle regression checks registration
  during/after closure, nested closure order, and failure continuation.

All 63 kernel tests pass after the two exception-branch additions; neither
addition required a production fix. The broader source review and full workspace
validation after `92beacb` are recorded in the main plan. Refreshed integrated
package evidence includes the internal waiter fix; the named unbounded-
concurrency API and clean committed-tree validation remain separate obligations.

## Synchronization implementation

Ref, Semaphore, and SynchronizedRef are registered opaque types with maintained
stdlib signatures and direct bundle exports. Ref callbacks are synchronous;
SynchronizedRef callbacks return Tasks. These are cooperating-fiber primitives,
not cross-worker atomics or transactional protection for escaped payload aliases.

Kernel tests cover lazy/reusable operations, independent allocations, Ref's
commit-after-callback rule, serialized suspended cell updates, reads during a
write, pre/post-commit interruption, and alias mutations that are not rolled back.
Semaphore tests cover weighted FIFO ownership, queued/granted interruption,
invalid limits, and permit retention through child and finalizer cleanup.
Stored-interface tests and the explicit_async fixture cover their public types,
callback requirements, mutation restrictions, and executed operations.

Current diagnostic check: reran both stored Ref/SynchronizedRef shared-payload
snapshot tests. A String array supplied to a Number-array cell now reports
`expected Number, found String` at the offending String literal, not the whole
call. The historical reversed-direction notes in the concurrency plan are
superseded. This is verified behavior from the existing argument-context path,
not a new production fix or a claim about every diagnostic's direction.

## Fairness and stack safety

`scheduleDrain` replenishes the shared 1,024-step budget only inside the host
timer callback. Promise resumptions and new fibers do not reset it. The ready
queue drives execution; task frames expose sequential composition to this budget
without allocating a child fiber for each await. Individual synchronous steps
and bulk operations remain non-preemptible.

Kernel regression `immediately_ready_work_yields_to_host_timers` exercises
Promise, completed join, fork/join, all, race, and finalizers with operation-count
assertions. `host_timer_can_interrupt_immediately_fulfilled_awaits` checks timer
interruption and exactly-once cleanup. `task_composition_is_stack_safe_before_and_after_suspension`
runs 20,000-deep reusable tasks with and without Promise suspension and asserts
one fiber identity. `deep_task_failure_and_interruption_unwind_suspending_cleanup`
checks all 20,001 cleanup frames and one finalizer on defect and interruption.
Semaphore handoff and traversal tests separately check fairness and provider
context inheritance. The compiled async fixture retains deep-await checks.

The pinned Effect reference and deliberate runtime/ABI differences are recorded
in `docs/effects-internals.md`. No Effect code is copied and no full Effect
instruction algebra, checked-error channel, or provider checker is introduced.

## Traversal acceptance and remaining delivery

### Bounded traversal acceptance

The shared `fiberTraverse` worker loop allocates at most
`min(input.length, concurrency)` workers, not one waiting fiber per item. Each
worker claims an index only when it can start that callback. The lazy task
validates a positive safe integer limit (or explicit Infinity), then takes a
shallow `slice()` snapshot on each execution. Membership/order are fixed for
that run; element aliases remain shared. Map allocates ordered result slots;
forEach allocates no result array and requires unit completion.

Current kernel tests map the principal obligations:

- `fiber_public_options_are_lazy_reusable_and_validated` checks all four public
  adapters, per-run configuration reads, sequential defaults, limits, and no
  callbacks on invalid configuration.
- `fiber_traversals_snapshot_membership_but_preserve_payload_aliases` covers
  all four variants; `fiber_map_keeps_input_order_when_later_items_finish_first`
  checks completion-order independence.
- `fiber_map_is_lazy_bounded_ordered_and_snapshots_each_execution` also checks
  that ordinary map retains Err as a value rather than cancelling siblings.
- `fiber_typed_traversals_select_the_first_observed_error_not_input_order` and
  the sibling-cleanup tests check fail-fast selection and stopping new work.
- Cancellation during selected-error cleanup, masked interruption, defects,
  malformed callback results, timer fairness, and per-item provider isolation
  have separate bounded tests in `crates/alder-kernel/src/lib.rs`.

The stored traversal-function test in the driver retains callback/result types
and optional options through serialization and rejects non-unit forEach
callbacks. `tests/e2e/explicit_async/src/traversals.ald` executes all public
variants, ordinary Result collection, typed propagation, per-run laziness, and
parent interruption/typed failure waiting for both active item finalizers.
The freshly packaged CLI executes that project successfully.

All 63 kernel tests and doctests pass in the current integrated audit. This closes
the listed bounded traversal implementation/coverage requirement, not the named
unbounded configuration choice or whole-runtime correctness proof.

### Outstanding delivery

The four traversal worker implementations and optional-options adapters exist,
with kernel, stored-interface, and CLI coverage for ordered collection, typed
fail-fast behavior, parent cancellation, and cleanup joins. The public named
unbounded configuration is now approved and implemented as the Number value
`Fiber.unbounded`, used inside `{ concurrency: Fiber.unbounded }`. A compiled
CLI regression failed with unknown-name before implementation and now gates
three callbacks until all have started, then checks ordered results. The same
fixture exercises all four public traversal APIs with this value. The kernel
options regression uses the exported value and verifies peak concurrency of
three for every traversal adapter; canonicalization checks its monomorphic
Number annotation. Record
semantics are now settled: optional shorthand is an ordinary Option field,
with omission equivalent to None. The traversal options adapter now consumes
ordinary Option values; the record-options CLI fixture executes both omitted
and explicit None concurrency configurations with the sequential default.
The public value, built-in annotation lookup, and bundle export are delivered;
final packaged verification must include these new source changes.

Final joint compiler/evidence review, documentation/changeset reconciliation,
coherent commits, and clean-tree package verification remain goal-wide gates.
The current kernel suite contains 63 tests. Final validation must cover the
eventual committed source state, not historical incremental test counts.
