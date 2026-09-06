# Control-flow acceptance evidence

This maps hardening objectives 5 and 11 to the active implementation. It is
evidence for the current working tree, not a general soundness or completion
claim. The operand-reachability changes are still uncommitted.

## Current acceptance reconciliation

The listed control-flow checks are verified in the integrated worktree at
`cc5d41e`; the historical follow-ups below record how defects were found and
fixed, not outstanding failures. Source review confirms that match lowering
evaluates the scrutinee once, tries each alternative with its own guard, and
leaves the labeled match after a successful body. `append_pattern_test` gates
effectful later pattern tests on the preceding match. `PatternFlow::guarded`
and inference's `alternative_reachable` use the corresponding rejection/retry
transition. Lambda and async inference reset and restore loop/reachability
state; canonicalization resets loop depth at function boundaries. Named and
method bodies use the same fallthrough/return-boundary checks.

The six AST flow tests pass again, including guard retry/stop and sibling
pattern sequencing. The full integrated workspace pass includes 459 solver
integration tests, 184 driver tests with diagnostic snapshots, and 68 codegen
tests. The freshly packaged CLI executes `control_flow`, including `pins.ald`
and `methods.ald`, from a source-only copy outside the checkout. Original
missing-return and loop-result probes also produce their intended outcomes.
See `release-packaging-hardening.md`, current checkpoint at `cc5d41e`.

This closes the specified control-flow regression matrix and its previously
stale checklist entries. It does not prove arbitrary program equivalence or
discharge the independent generic/row constraint audits. Coherent commits and
final clean-tree verification remain whole-goal gates.

## Invariants and boundaries

`alder-ast/src/flow.rs` distinguishes normal continuation from return, break,
continue, and divergence. Sequential composition ignores unreachable successors;
loops consume their own exits. Unknown conditions are conservative. This is
structural analysis, not interprocedural termination analysis.

`Infer::infer_loop_body` pushes and removes a result frame. `Expr::Loop` supplies
a fresh variable; while/for bodies supply unit. Reachable breaks join their
payloads into the innermost frame; a bare break contributes unit. Lambda and
async inference save and clear the enclosing frame stack, then restore it.
Canonicalization rejects exits crossing those function boundaries.

`with_reachability` combines a child condition with the enclosing reachability
and restores the enclosing state even if inference returns an error. A child
cannot make an already-unreachable context reachable. Array/tuple elements,
record fields/spreads, tag payloads, template interpolations, indexes, and
assignment operands now carry fallthrough in evaluation order. Unreachable
operands still undergo type checking; their breaks do not join live loop results.

Codegen uses a separate result slot and explicit label for each source loop.
While conditions are lowered before entering the while-body target scope.
Thus an exit in condition setup keeps its outer lexical target when that setup
is placed physically inside the generated while. This fix is committed in
`a9f1d44` and tested through actual bundled execution, including await/continue.

## Requirement map

| Requirement | Inspected regression evidence |
| --- | --- |
| Zero-iteration loops cannot satisfy a Number return | `zero_iteration_loops_do_not_satisfy_a_return_contract`; driver snapshot `renders_a_missing_return_after_a_zero_iteration_loop` |
| Branch exits and normal fallthrough are distinct | `explicit_returns_in_all_branches_have_no_unit_fallthrough`; CLI `choose`, `mixed`, `matched`, `awaited` execute both branches |
| Divergence needs no fake value | `diverging_loops_do_not_require_a_fake_return_value`, `skipped_exits_do_not_make_an_infinite_loop_produce_unit` |
| Separate loop results and statement-loop unit | `loop_break_values_determine_the_result_type`, `incompatible_breaks_and_statement_loop_values_are_rejected`; CLI nested while/for/loop cases |
| Nested function returns stay local | `returning_lambda_block_has_no_unit_fallthrough`, `explicit_async_block_return_does_not_constrain_outer_function`; CLI callback return |
| Method/default bodies enforce fallthrough | `method_bodies_reject_zero_iteration_return_paths` checks MissingReturn for sync/async defaults and implementations |
| Method exits and propagation execute correctly | `tests/e2e/control_flow/src/methods.ald` executes both default/override return branches and Some/None plus awaited Ok/Err propagation through loop break payloads |
| Unreachable breaks do not affect results | `unreachable_breaks_do_not_constrain_a_live_loop_result`, short-circuit/false-guard tests, aggregate and assignment operand regressions |
| Possible exits still agree | `potentially_reached_conditional_breaks_must_agree`, `potentially_reached_aggregate_exits_still_must_agree` |
| Break payload is evaluated once | CLI `loop_payload_once`, `break_payload_effects` |
| Lowering retains lexical targets | Codegen snapshot `while_condition_break_preserves_outer_target`; CLI `while_condition_exits_enclosing_loop`, `while_condition_continues_enclosing_loop` |
| Composite operand exits execute correctly | CLI array, annotated array, tuple, tag, template, tagged template, index, record, annotated record, and assignment exit assertions |

Solver regressions are in `crates/alder-solve/tests/inference.rs`; the CLI cases
are in `tests/e2e/control_flow/src/main.ald`. The CLI fixture compiles, bundles,
and executes from outside the repository, rather than merely comparing emitted
text. Infinite-loop acceptance cases are compile-only to avoid hanging tests.

## Historical acceptance follow-ups

The subsequent codegen comparison found a guard-retry mismatch in the new
pattern summaries: guards retry per alternative, but flow analysis had applied
the guard only after combining alternatives. The Number function with
`(_, _) | (^{ break "wrong" }, _) if false` incorrectly compiled. Its new
negative regression failed before the correction and now passes. PatternFlow
now applies the guard before composing alternatives; solver reachability uses
the same transition. All 426 solver tests pass, including the successful-guard
negative-reachability control. The rebuilt CLI executes the expanded control_flow
fixture successfully from `/tmp`, including the guard-retry pin returning 42.
The full workspace test run now passes for this correction, including doctests.
All six direct AST tests pass, including false-guard retry and
true-guard stop. The AST correction and those tests are committed as `008e94b`;
the companion solver reachability change remains in the larger worktree awaiting
coherent commit review. Strict workspace Clippy passes after these tests too.

The full workspace run completed successfully, including 418 solver tests and
doctests (two pre-existing parser/runtime ignores), alongside strict
all-target/all-feature Clippy. The subsequent method-only test addition passes
all 419 solver tests and the expanded actual CLI fixture. A subsequent production
fix threads the enclosing return contract through match-pin expressions, which
previously accepted a bare return in a Number function. All 420 solver tests and
the new runtime pin return/constructor-skip/Option-propagation cases pass; full
workspace verification must be refreshed for that fix. Structural pattern-exit
summaries now distinguish match/reject/exit outcomes: a pin break can no longer
make a value-producing loop masquerade as divergent. The matching negative
regression and positive skipped-guard/body/later-arm case pass, including CLI
execution. Nested sibling-pattern inference now carries matchability between
children. Positive tuple/array/record/constructor/error-tag tests and a
conditional negative control pass; the actual nested-pin CLI case passes too.
The combined full workspace test run has now exited successfully, including
doctests, and strict all-target/all-feature Clippy passed for these production
changes. Two subsequent AST unit tests directly check that sibling pattern
evaluation requires a match and alternative evaluation requires rejection;
all four AST unit tests pass. Formatting and `git diff --check` pass. The AST
flow implementation, its unit tests, and its changeset are committed as
`38a9ac9`; strict Clippy also passes after the added tests. Solver/CLI changes
still require commit review, and fresh final packaging remains outstanding.

Method and default bodies both use `infer_function`, including its structural
fallthrough check and `resolve_try_boundary`. The added four negative method
cases produce MissingReturn, and all twelve runtime assertions in methods.ald
pass through the actual CLI. This test-only addition requires no compiler fix.

Do not equate these checks with the whole control-flow objective. Audit
pattern/guard evaluation interactions and finish the cross-feature review.
The combined solver changes also require coherent commit
review and final package verification. No acceptance requirement is waived by
this evidence map.
