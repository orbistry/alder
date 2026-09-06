# Option propagation hardening

Status: core typing and direct AST lowering implemented; boundary/cleanup audit
and final hardening acceptance remain open. Approved semantics are recorded in
`plans/hardening-language-decisions.md`.

`Some(value)?` produces value; `None?` returns None from the enclosing Option
return context. Result propagation remains separate, without implicit conversion
between the two. Async bodies propagate through their completed-value return
context, not their outer Task wrapper.

Inference records Option propagation expression regions in SolveOutput. Direct
Oxc lowering evaluates the operand once, returns it when null, and otherwise
calls the existing representation-aware `$optionUnbox`. This preserves boxed
nested Options and keeps Option representation logic in the kernel helper.
The same lowering is used for pipe destinations and `.await?`.

When both operand and return types are unknown, a constraint waits until its
enclosing boundary has a body type. This permits `fn inferred(value) {
Some(value?) }` without prematurely forcing Result. Resolve that constraint
before return-row inclusion: waiting until the module pass broke the existing
exhaustive-match regression for an inferred Result-returning lambda. The final
implementation preserves that regression and its exact error-row behavior.
The otherwise-undetermined fallback retains the prior Result inference; audit
ambiguous and recursive cases before treating inference coverage as complete.

Evidence:

- A positive Option propagation solver regression failed before implementation;
  explicit, inferred, and async Option cases now pass.
- Negative tests reject Option-to-Result, Result-to-Option, and plain-value
  return contexts.
- Two reviewed source-aware codegen snapshots cover nested unboxing and piped
  async propagation. A reviewed colorless miette snapshot covers an incompatible
  Result return context; normal CLI colors are unchanged.
- Actual standalone CLI execution checks None, Some(None), Some(Some(42)),
  piped async success/failure, and skipped effects after None propagation.
- All 325 solver integration tests pass, including existing Result regressions.
- All 51 codegen and 132 driver tests pass; strict workspace Clippy, formatting,
  and diff checks pass. No pending snapshots were found.

Remaining: nested mixed async/lambda boundaries, unit and additional nested
payload cases, exactly-once operand evaluation with effectful calls, structured
cleanup on propagation, cross-module inference, recursive ambiguity, and full
workspace/release verification. Do not mark the broader goal complete from
these focused tests.

## Cross-module execution and cleanup checkpoint

The standalone CLI fixture now imports `option_flow.ald` and executes a generic
inferred Option helper at Number and String payloads, preserves Some(None), and
checks Some(()) versus None. Ordinary lambdas and lambdas returning async blocks
own their propagation boundaries; a nested Option task inside a Result-returning
function also has its own boundary. An effectful operand appends an event, proving
exactly-once evaluation; a later event is skipped only for None.

A reusable scoped Option task registers a finalizer that actually suspends with
Task.sleep(0). Propagating None skips subsequent task-body effects, waits for the
finalizer, and produces one cleanup event per execution. Running the task twice
produces exactly two cleanup events. These actual compiler/runtime checks extend
the earlier direct-AST snapshots; they are not kernel-only simulations.

Remaining coverage includes recursion/ambiguous inference, interruption during
propagation cleanup, broader combinations of conditional control flow, and final
workspace/release gates. Earlier listed unit, cross-module, nested-boundary, and
exactly-once operand cases are now covered by this CLI checkpoint.

## Corrected execution evidence and interruption checkpoint

The earlier cross-module checkpoint ran `standalone_e2e_projects_execute`, which
does **not** include `explicit_async`. Its passing result did not establish that
the new Option fixture executed. Running the actual dedicated test,
`explicit_async_laziness_capture_and_nested_tasks_execute`, exposed a legitimate
ambiguous Eq obligation in `preserve(Some(None)) == Some(None)`: neither side
determined the innermost payload. Giving the input an explicit
Option[Option[Number]] annotation fixes the fixture without changing inference.
The dedicated test now passes all the above assertions through compile, bundle,
and V8 execution. Future execution claims must identify the test that includes
the fixture, rather than relying on a similarly named suite.

Added a source-level interruption regression: a scoped task propagates None,
enters a suspending finalizer, and holds cleanup behind a captured release flag.
Two interruption tasks remain pending until release; afterwards both finish and
exactly one cleanup event exists. Post-propagation body effects never execute.
The shared CLI execution helper now has a 30-second async timeout for suspended
execution failures; this is a safety bound, not a performance assertion or a
preemptive guard against arbitrary synchronous V8 loops. The dedicated test
passes with this additional regression. No runtime change was needed.

Recursion/ambiguous inference, additional control-flow combinations, and final
workspace/release gates remain open.

Validation after this correction and interruption regression: full
`cargo test -- --quiet` exits 0, including all 12 CLI tests, 325 solver
integration tests, 132 driver tests, 51 codegen tests, and 50 kernel tests.
The three previously ignored doctests remain ignored. Strict workspace Clippy,
formatting, and diff checks pass, and no pending `.snap.new` files were found.
This supersedes earlier workspace evidence for this checkpoint; release
packaging and the remaining hardening requirements are not completed by it.

## Recursive carrier selection defect

A new positive regression failed before the fix: `first` propagates an
unannotated parameter, then returns a call to its recursive peer `second`;
`second` establishes Option through `Some(value?)`. Finishing `first`'s body
previously defaulted its still-unknown operand/return carrier to Result, so
checking `second` rejected Option against Result. This was premature selection,
not an incompatible program or a reason to require annotations.

When the operand, enclosing return, and body result are all still inference
variables, boundary resolution now retains the pending propagation constraint.
The existing SCC pass resolves it after every recursive member contributes its
constraints and before generalization. Known carriers still resolve at the body
boundary, retaining the earlier Result error-row inclusion behavior. Reviewed
Elm's `Type/Solve.hs` CLet ordering (solve header constraints before generalizing)
as the structural reference; carrier selection itself is Alder-specific.

The original regression now passes, including independent Number and String
uses of the inferred recursive function. Added a reversed-declaration case,
the corresponding Result case, and cross-module CLI calls covering success,
None, and Some(None). Truly undetermined carrier defaulting at the SCC boundary
and more involved recursive/control-flow interactions still require audit.

Validation for the recursive fix: all 328 solver integration tests, 12 CLI
tests (including the dedicated explicit async fixture), 132 driver tests, and
51 codegen tests pass. Strict workspace Clippy, formatting, and diff checks
pass; no pending snapshots were found. Updated the Option propagation Sampo
changeset. The preceding full-workspace run predates this solver change; final
whole-workspace and packaging gates remain required.

## Explicit exits and reachable fallthrough

Added passing regression coverage for recursive calls returned with an explicit
`return`, and Option inference driven entirely by explicit exits in named
functions, lambdas, and async bodies. These probes did not expose another
compiler defect; no inference change was made for this checkpoint.

Negative cases require an Option value on the successful propagation path:
`let item = value?` alone cannot satisfy an Option-returning ordinary or async
function, and a conditional explicit return cannot hide the other reachable
unit exit. The fixtures parse successfully before checking rejection.

Cross-module CLI execution checks both explicit-return branches, propagation
before those branches, and async explicit returns preserving Some(None) versus
None. All 331 solver integration tests and all 12 CLI tests pass after these
additions. The previous implementation fixes remain unchanged; this is
control-flow acceptance evidence, not completion of the broader hardening goal.
