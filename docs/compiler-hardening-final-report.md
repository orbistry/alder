# Compiler hardening: final acceptance report

Scope: the eleven findings against `21994e0`, their related contract audits, and
the approved pre-1.0 language/async amendments. This report supersedes historical
open-work statements in the chronological plans. It does not claim M5–M10 or
general compiler soundness. The compiler/runtime integration is `556a21c`;
clean package verification covers `d9e7699`, with evidence recorded in `412fbee`.
Subsequent completion edits are documentation only.

## Original defects and requirement reconciliation

| Requirement | Root cause and implemented invariant | Verification and detailed acceptance map |
| --- | --- | --- |
| 1. Universal contracts | Flexible signature instantiation allowed method specialization. Deferred universal checking now rejects specialization, merging independent universals, and escape after all connected constraints settle. Inference positions remain flexible. Defaults, implementations, local annotations, HKT, associated equalities, bounds and recursive groups preserve their checked contracts before publication. | Invalid Convert probe rejects at its implementation; valid independent Number/String calls, negative signature tests, stored/dropped producers, and source-aware diagnostics pass. [Generic contracts](generic-contract-hardening.md), [dictionary evidence](dictionary-lowering-hardening.md). |
| 2. Mutation and polymorphism | Restricting only keyword-marked bindings missed shared mutable objects. Generalization accounts for assigned identities, restricted SCC peers, environment variables and connected deferred constraints. Instantiation copies only quantified identities; captured/shared variables remain shared through interfaces. | Original shared-array probe rejects; arrays/maps/sets/records/cells, aliases, nested closures, reusable tasks, SCCs and imported state have positive/negative tests. Independent factories remain polymorphic. [Mutation/generalization](mutation-generalization-hardening.md). |
| 3. Formatting | Trimming before recognizing literals changed payloads despite successful reparsing. Parser-selected verbatim ranges and physical-line token validation preserve meaning; unsafe candidates fail before CLI writes. | Fifteen formatter tests include templates/interpolation/escapes, macros, markup, comments, LF/CRLF, parsed-structure comparisons and independent idempotence. CLI executes an actually changed template and tests invalid-input write prevention. Packaged length-13 whitespace probe passes before/after a real edit. [Plan section 3](../plans/compiler-hardening.md). |
| 4. Module identity | URI/suffix-derived ownership admitted duplicate canonical identities and unstable resolution. Explicit owning-package/source-root identities are validated before interfaces/bundling; dependency and cache indexes preserve those identities. | Duplicate probe rejects and labels both paths. Reordered discovery, nested/relocated workspaces, repeated src segments, same-relative-path packages, cache/index agreement, re-exports and byte-identical initialization tests pass. [Module identity](module-identity-hardening.md), [re-exports](reexport-hardening.md). |
| 5. Return/control flow | Syntactic return detection confused a possible loop-body return with exhaustive return. Explicit continuation/return/break/continue/divergence composition models reachable paths and separates nested callable boundaries. | Missing-return probe rejects; branches, matches, guards/pins, loops, early returns, propagation, named/lambda/default/implementation/async bodies and diverging paths are tested in inference and compiled execution. [Control flow](control-flow-acceptance.md). |
| 6. Scheduler fairness | Resetting the budget at resumption let ready Promises starve timers. Only a host yield replenishes the shared operation budget; task/observer processing uses scheduled iteration rather than recursive reentry. | Bounded kernel tests cover Promise/join/fork/all/race/completion/cleanup, cancellation and reentrancy. Original extracted-kernel probe observes the timer during 10,000 ready awaits. [Async acceptance](async-hardening-acceptance.md). |
| 7. Stack-safe tasks | Nested generator delegation hid recursion from the scheduler. Explicit task frames preserve sequential execution in the same fiber and expose completion/failure/interruption. | Compiled 20,000-deep awaits pass before and after suspension; deep cancellation/unwinding, Result propagation, reusable laziness, scopes/finalizers and context inheritance have bounded tests. [Async acceptance](async-hardening-acceptance.md), [ABI/reference](effects-internals.md). |
| 8. Record rows | An open/closed flag lost row-tail identity. Real kinded row terms retain common/residual fields, shared tails and input/output relationships through unification, occurs checks, generalization, instantiation and owned storage. | Original x/y sum passes. Both access orders, spreads/overlays, optional fields, aliases, patterns, assignment, contradictions, cycles, stored consumers and recursive cross-feature cases pass. Record and error rows remain distinct. [Record rows](record-row-acceptance.md), [overlay plan](../plans/record-overlay-hardening.md). |
| 9. Option operations | Payload dictionaries received representation boxes. Centralized one-layer unboxing is now used by every relevant producer/consumer while preserving None versus Some(unit)/Some(None). | Original nested equality passes. Four-layer laws, custom payload dictionaries, records/enums, Eq/Hash/Ord/Show/JSON, mapping/apply/flatMap/traverse, lookup and round trips pass, including imported compiled tests. [Option operations](option-operation-acceptance.md). |
| 10. Local externs | Virtual imports lacked a physical resolution base. Bundling carries the Alder source origin and resolves relative JS modules beside that source, independently of shell cwd. | Packaged local Promise wrapper passes from /tmp. Nested/dependency package wrappers, typed fulfillment, throw/reject/malformed-return/AbortSignal behavior and declaration-context errors have CLI/kernel tests. Direct Oxc/Rolldown AST generation remains intact. [Module identity](module-identity-hardening.md), [async acceptance](async-hardening-acceptance.md). |
| 11. Loop results | Loops always inferred unit and did not join break payloads. Each loop expression owns a result variable; reachable breaks join it, bare breaks contribute unit, statement loops retain unit and exitless loops diverge. | Original Number-valued break prints 42. Nested targets, branches, incompatible breaks, return/propagation, awaited payloads and while-condition lexical exits agree in inference and lowering. [Control flow](control-flow-acceptance.md). |

## Approved language decisions

- `mut` is removed, including AST permission flags and migration-specific errors.
  Ordinary bindings/parameters allow writes; type safety and JavaScript-style
  aliasing remain. No compatibility shims or legacy interface readers were added.
- `async fn` and `async {}` add exactly one lazy Task layer, without flattening.
  Task return annotations alone do not permit await. Ordinary lambdas may return
  async blocks; no async-lambda prefix was added. Main may be explicitly async.
  Captures follow lexical binding identity; call arguments evaluate at construction.
- Ref, SynchronizedRef, cancellation-safe Semaphore, and ordered
  Fiber.map/forEach/tryMap/tryForEach are implemented. Ordinary traversal collects
  Result values; try traversal fails fast and cleans up siblings. Concurrency
  defaults to one, validates at execution, and supports `Fiber.unbounded`.
- Optional field/parameter shorthand is ordinary Option. Trailing Option
  arguments may be omitted as None. Supplied construction arguments/fields use
  direct matching or minimal Some lifting, never unrestricted implicit coercion.
  Ambiguous lifting is diagnosed. Existing aliases/assignments use ordinary types.
- `?` supports Result and Option at matching return boundaries, not an arbitrary
  Monad bind. `??` unwraps one Option layer and evaluates its default only for None.
- Tuple projections collect fixed-length constraints with minimum length two;
  sparse storage avoids source-index-sized allocations and preserves interfaces.
- Result errors are structural rows/groups. Named groups cannot define nominal
  custom instances. Selected structural Eq/Show/Json/Hash capabilities depend on
  payload capabilities; Ord is not automatically supplied.
- Array.iter returns an independent, advancing ArrayIterator. Iterator aliases
  share progress; separate iterators do not. Native live mutation semantics and
  permanent exhaustion are documented and tested.

The current contracts are recorded in SPEC, the language/traits/effects/codegen
guides, and the linked feature plans. These decisions were tested across parser,
canonicalization, solving, serialization, direct AST lowering and actual CLI use.

## Related defects and adversarial checks

The related audit did not stop at the original eleven examples. Confirmed fixes
include shared constraint escape through tuples/overlays/errors, superclass slot
and default-helper publication, imported omitted defaults, dictionary temporal
dead zones, recursive evidence binding, exponential multi-method dictionary
emission, primitive signed-zero Hash/Eq disagreement, cyclic value operations,
large-string hashing, contextual Option/row construction, callback argument leaks,
non-advancing array iteration, cache identity/index disagreement, match guard
retries, mutation invalidating tested pattern payloads, and operand ordering.

- [Dictionary audit](dictionary-lowering-hardening.md) maps selection/coherence,
  slot order, imported/stored defaults, closures across await, initialization,
  recursive evidence and bounded depth/output-size controls.
- [Evaluation order](evaluation-order-hardening.md) and
  [pattern captures](pattern-capture-hardening.md) map calls/pipes, aggregates,
  assignments, templates/spreads, lazy defaults, guarded extraction and exits.
- [Async acceptance](async-hardening-acceptance.md) maps Promise synchronous
  throws/rejection/malformed returns, reentrant interruption, late settlement,
  exactly-once completion/observers, child ownership, all/race losers, finalizers
  registered during/after closure and interruption during masked cleanup.
- [Source boundaries](source-boundaries-hardening.md) verifies precise Unicode
  snippet slicing, all diagnostic phases, and rejection rather than runtime
  placeholders for deferred constructs. Tables/schemas publish only provisional
  types; macro calls cannot publish callable stubs. Provider checking stays out
  of scope while its runtime context seam remains tested.
- [Collections](collection-runtime-acceptance.md), [JSON](json-hardening.md),
  [cyclic values](cyclic-values.md) and [stdlib inventory](stdlib-contract-audit.md)
  map representation laws, callback arity, exact exports and packaged sources.

These source-to-regression audits include alternate syntax/forms and combinations,
not just the original programs. No valid multi-parameter prerequisite-cycle
failure was reproduced: current source bounds are unary. That investigation is
documented without adding speculative syntax or claiming a fix.

## Validation and release evidence

- Full workspace tests/doctests pass: 1,310 parser, 467 inference, 199 driver,
  72 codegen, 72 kernel, 17 CLI, plus the other crate/annotation/mutation suites.
  Two existing illustrative doctests remain explicitly ignored.
- `cargo fmt --all` and strict all-target/all-feature Clippy pass.
- `cargo insta test --check --unreferenced reject -- --quiet` passes: no pending
  or unreferenced snapshots. Stale parser/report binaries had embedded an older
  temporary checkout path; rebuilding those crates corrected the initial false
  reference report. No snapshots were deleted. Exact renderer whitespace and an
  empty emitted-JS snapshot remain intentional, reviewed output.
- Clean-tree offline packaging verifies all 17 publishable crates without
  `--allow-dirty`. All 1,517 extracted source/stdlib/kernel files match. The lockfile
  is unchanged. The packaged CLI passes four examples, all 17 integration
  projects with expected exits, every original counterexample, actual formatter
  edits, test-mode success/failure, and the original kernel fairness probe.
- [Release evidence](release-packaging-hardening.md) records paths, process
  outcomes and 45-second CLI subprocess bounds. It is local host verification,
  not an assertion that remote Windows CI has executed.
- All changed publishable crates have per-crate Sampo entries. The integration
  includes the reviewed source-aware indoc snapshots and preserves normal CLI
  color; no generated-source string reparsing was introduced.

Effect v4 reference is pinned to `bd393d63c19bdd0ab212d95576cec89051c8501c`.
The runtime reimplements the needed scheduling/cancellation invariants; no Effect
code was copied and no dependency was introduced. The ABI and deliberate
differences are recorded in effects-internals. Elm remains the local reference
for structure and diagnostics; Alder's accepted semantics take precedence.

## Delivery and limits

Important checkpoints: `556a21c` integrates the cross-layer changes; `c9bb0b6`
fixes Coalesce; `d63bd86` completes dictionary evidence tests; `edbcd63` fixes
iteration; `7d2bef0` verifies source/deferred boundaries; `3bb8317` reconciles
design records; `d9e7699` updates active-pipeline guidance; `412fbee` records
fresh release evidence. Earlier defect checkpoints are retained in the main plan.

The hardening branch is ready to merge; delivery includes this report and the
completed checklist in a clean commit. Nothing has been pushed, merged,
published or tagged.
No major milestone beyond the hardening scope is implied.

Limits are deliberate documented contracts: synchronous computation is not
preempted; arbitrary foreign Promises need cooperative abort support; mutable
objects retain JavaScript aliasing; NaN retains non-reflexive primitive equality;
cyclic Hash/Ord/JSON follow the approved defect policy rather than a graph format;
the formatter is conservative, not a general reformatter. Runtime defect/all-
settled APIs, full provider checking and M5–M10 remain deferred. Finite tests and
this audit are not a proof of general type soundness or arbitrary user-instance
laws. There are no unresolved required hardening defects in the acceptance map.
