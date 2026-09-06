# Current hardening verification

Current status: compiler/runtime integration is committed in `556a21c`.
The latest full workspace pass before that commit includes 72 codegen, 199
driver, 72 kernel, 17 CLI, and 467 inference tests, plus strict Clippy and
formatting. Fresh committed-code tests and package verification are being run;
the older package results below are not final release evidence. Contract audits
are reconciled; final acceptance and clean-tree gates remain open.

## Earlier verification checkpoints

Historical status: the then-latest full integrated test pass and fresh package verification were
at `cc5d41e` plus the hardening worktree. All 17 packages verify; their CLI passes
the original review probes, four examples, seven integration projects, and
passing/failing test-mode cases. See `release-packaging-hardening.md` for the
authoritative current commands and outcomes. This is not verification of a
future clean committed tree or completion of the whole hardening objective.

Control-flow acceptance has been reconciled against current inference, flow
summaries, direct AST lowering, diagnostics, and packaged execution. The two
stale control-flow checklist entries are now checked with explicit evidence in
`control-flow-acceptance.md`. Independent generic/row/representation audits,
pending language decisions, and final commit review are not closed by that
bounded acceptance finding.

## Historical post-record-migration CLI recheck

Rebuilt `alder-cli` and copied only `src/` and `alder.jsonc` from the earlier
syntax-migrated probes into fresh `/tmp/alder-recheck.TECPbq` directories. No
build outputs or interface caches were copied. Invoked the absolute workspace
CLI from `/tmp` for each project.

- `trait`, `arrays`, `returns`, and `duplicate` each exit 1 with the expected
  generic-specialization, Number/String alias mismatch, missing-return, and
  duplicate-module diagnostics. The duplicate diagnostic names both files.
- `records`, `loops`, `option`, `extern`, and `deep` each exit 0; loops prints 42,
  and the others execute their assertions, including 20,000 recursive awaits.
- The original Node runtime probe prints `timer ran during 10000 fulfilled
  Promise awaits: true` against the current kernel source.
- `format` passes its length-13 assertion. Changing only assertion indentation
  makes `fmt --check` exit 1. `fmt` reports one changed file; execution and the
  next `fmt --check` both pass. Inspection confirms the template's three-space
  line is intact. The first unchanged-source formatting check was not counted
  as evidence of edit preservation.

All four repository examples also execute from `/tmp`: hello, pipes, traits,
and async emit their expected success messages. The deep/extern probes use the
approved explicit async syntax, not removed syntax. These checks follow the
full workspace pass containing the latest open-spread changes and the later
449-test solver audit. Subsequent fresh package verification now covers all 17
crates in `/tmp/alder-package-records.FkiXbJ`; its CLI executes all four examples
and the record_options, traits, explicit_async, and control_flow fixtures.
See `release-packaging-hardening.md` for source checks and limits. The branch is
still uncommitted and not release-ready.

## Earlier CLI counterexample run (historical)

Rebuilt the actual CLI with `cargo build -p alder-cli`. Read the original
`/tmp/alder-review.D3XIzP/README.md` and source probes. Copied only source and
project configuration into `/tmp/alder-final-repros.yMmwu3`, without copying
compiled artifacts or interface caches, and invoked the absolute CLI path from
`/tmp` rather than the repository directory.

| Objective | Probe and observed result |
| --- | --- |
| 1. Universal contracts | `trait`: exit 1, generic-specialization diagnostic labels the implementation's promise of independent `b` and its Number requirement. |
| 2. Shared mutable polymorphism | `arrays`: exit 1, String/Number mismatch at the attempted Array[String] alias. |
| 3. Formatter semantics | `format`: passes its exact length-13 assertion before and after formatting; one file actually changed; subsequent `fmt --check` succeeds. The three-space template line remains intact. |
| 4. Module identity | `duplicate`: exit 1, duplicate-module diagnostic includes both `util.ald` and `util/mod.ald`. |
| 5. Fallthrough | `returns`: exit 1, missing-Number-return diagnostic explains that while may run zero times. |
| 6. Fairness | Original `runtime.mjs` prints `timer ran during 10000 fulfilled Promise awaits: true` against current kernel source. |
| 7. Stack safety | `deep`: exit 0 after 20,000 recursive awaits. |
| 8. Record rows | `records`: exit 0, inferred x/y sum assertion succeeds. |
| 9. Option representation | `option`: exit 0, separately constructed Some(None) values compare equal. |
| 10. Local externs | `extern`: exit 0, adjacent client.js Promise returns 42, independent of shell cwd. |
| 11. Loop values | `loops`: exit 0 and prints 42 from `loop { break 42 }`. |

The deep and extern source copies use the approved explicit async syntax from
`/tmp/alder-current-repros.f86n1p`; they are not tests of removed inferred-async
syntax. The formatter probe reconstructs the original literal whitespace and
changes layout outside the literal so preservation is tested during a real edit.
An initial interior-spacing edit did not trigger formatting, since the formatter
preserves interior tokens; indentation of the assertion was then changed.

The initial run against pre-existing probe directories had the same outcomes.
Fresh copies remove ambiguity about old artifacts. `alder run` compiles, bundles,
and executes the result; the run implementation was inspected during this check.

## Scope of the evidence

These probes verify the eleven reported manifestations, not every associated
acceptance requirement. The preceding workspace unit/integration run passed
414 solver integration tests, 158 driver tests, 62 codegen tests, 53 kernel tests,
and all 12 CLI tests, with strict all-target/all-feature Clippy. Those suites
include additional regressions but are not a substitute for the requirement audit.

Also reran the kernel tests `immediately_ready_work_yields_to_host_timers`,
`host_timer_can_interrupt_immediately_fulfilled_awaits`, and
`task_composition_is_stack_safe_before_and_after_suspension`; all pass. The first
exercises Promise, completed join, fork, all, race, and finalizer paths, checking
timer progress before the loop completes rather than only at final settlement.

Remaining completion gates include:

- Reconcile each requirement in the main plan with current implementation,
  regression coverage, diagnostics, and documentation; historical unchecked
  boxes and historical failure notes are not authoritative current outcomes.
- Finish the generic/recursive/stored-evidence, row/overlay, late lifting and
  tuple-constraint audits recorded in the detailed plans.
- Record semantics are settled and implemented as ordinary Option fields;
  follow `plans/optional-arguments-hardening.md` for current migration evidence.
  The public unbounded traversal spelling remains a separate unresolved choice.
- Rerun affected examples, final full tests including doctests, and release
  packaging for the final source state. Fresh packaging in
  `/tmp/alder-package-identities.UpMP5z` verifies the current explicit-source-
  identity changes, with both workspace regressions, two integration projects,
  and all four examples passing against the packaged CLI. Later production
  changes still require refreshed verification.
- Review the complete dirty worktree, reconcile changesets and documentation,
  commit coherent checkpoints, and leave a clean ready-to-merge branch.

No merge, push, release, or tag operation is authorized or performed by this
verification pass.

Subsequent example/package check: hello, pipes, traits, and async all execute
with their expected success messages on the rebuilt workspace CLI. A fresh
17-crate package verification passed in `/tmp/alder-package-current.6yJzBM`;
the resulting CLI also runs all four examples and the traits/explicit_async
fixtures successfully from `/tmp`. See `docs/release-packaging-hardening.md`
for scope and limitations. Generated
trait-example caches are no longer tracked and remain available locally.

Subsequent acceptance reconciliation: added a stored-interface regression for
independently instantiated generic overridden/default methods and four actual
cross-module CLI assertions. All pass. The new evidence map is
`generic-contract-hardening.md`; the main checklist now records the implemented
rigidity and diagnostic checks rather than leaving them marked unimplemented.
The formatter requirements were also reconciled against parser-selected ranges,
all 15 formatter tests, full parsed-structure comparisons, and CLI execution and
no-write-on-invalid-input tests; section 3's listed acceptance checks are closed.

Validation after these regression additions: formatting, strict workspace
Clippy, all workspace unit/integration tests (159 driver and 414 solver
integration), and workspace doctests pass. Doctests retain three pre-existing
ignored examples. `git diff --check` passes and no pending snapshots were found.
No production source changed in this checkpoint, so the preceding package
verification still covers production code; final packaging/commit readiness
must nevertheless be reconciled against the eventual final tree.

Nested workspace follow-up found and fixed order-dependent package ownership
when one member's source tree contains another's. The source owner is now the
most specific containing root, shared by package and module-path calculation.
Commit `adb35ac` contains that fix, its discovery-order/artifact/interface
regression, module documentation, and a driver/CLI changeset. Full formatting,
Clippy, and `cargo test` including doctests pass (160 driver tests).

Fresh package verification at `/tmp/alder-package-nested.NWQCRV` passes all 17
crates. Its CLI passes nested workspace checks in both member orders, plus the
traits and explicit_async execution fixtures. A prior reused-target package run
had linked stale registry source despite exiting zero; do not use that run as
evidence for the fix. Details are in `release-packaging-hardening.md`.

Member-alias follow-up: `ca9ab0e` canonicalizes and deduplicates loaded workspace
roots. A permanent filesystem regression first discovered four modules for two
files through equivalent parent-path spellings; it now loads one member/two
modules and compiles its local import. Unix symlink aliases preserve the same
metadata and discovery. The rebuilt CLI checks the real aliased workspace at
`/tmp/alder-member-aliases.t8tFDT/workspace` and runs the member application,
both from `/tmp`, with exit zero. Formatting, Clippy, all workspace tests, and
doctests pass. The driver API example now carries project metadata and passes
as a compile-checked doctest; two pre-existing ignored doctests remain. Final
package verification must be refreshed for this newer production source.

Explicit identity follow-up removes production URI guessing and metadata-free
driver entry points. Graph/build preflight now requires both package and path
maps, rejects missing or partial metadata, and emits no interfaces/artifacts
on failure. Four regressions cover those failures, deterministic error ordering,
and identical logical code under file versus in-memory source URLs. Tests now
use visibly named fixture helpers for fixture metadata; the actual project
APIs retain no fallback. All 165 driver and the full workspace unit/integration
tests pass; strict Clippy passes. Rebuilt CLI checks of the nested and aliased
workspaces and execution of traits pass from `/tmp`. Full doctests pass with
two pre-existing ignored examples; the final CLI rebuild and all 12 CLI tests
also pass. No pending snapshots or diff-check failures remain. Packaging and
coherent commits remain final-goal gates.
