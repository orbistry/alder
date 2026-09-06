# Evaluation-order acceptance evidence

This is a finite source-to-regression audit of the integrated hardening
worktree. It does not replace final committed-tree and release-package gates.

## Invariants and evidence

| Boundary | Lowering and executable regression |
| --- | --- |
| Calls and pipes | `call` captures the callee before arguments; pipe lowering captures the leading operand before its destination. `control_flow/src/call_order.ald` replaces local and record-field callees during argument evaluation and checks factory/argument/body event order. |
| Earlier operands before lifted statements | `sequence_values` materializes earlier expressions before a later operand's prefix. `control_flow` covers arrays, tuples, calls, captured scalar values, and shared object references. |
| Assignments | `place_pair` captures receivers/indexes once; compound assignment reads the old value before evaluating its RHS. CLI cases cover receiver rebinding, custom Num dictionaries, index mutation, awaited indexes/RHS, and propagation. |
| Short-circuit defaults | And/Or and Coalesce keep RHS prefixes inside the selected branch. `call_order.ald` checks skipped/reached side effects and awaited defaults. Coalesce uses exact None detection and unwraps only the present branch. |
| Records and templates | Record spread copies already-observed fields before later effects. Template interpolation converts each value at its source position. Existing records and control_flow CLI cases assert mutation visibility and event order, including tagged templates. |
| Matches | The scrutinee is evaluated once; alternatives retry their own guards. Pattern extraction captures values at the tested position, behind preceding shape checks. `pattern-capture-hardening.md` maps mutation, array-rest, alias, and awaited-pin regressions. |
| Exits and boundaries | Explicit labels/result slots preserve lexical loop targets; lambda/async lowering resets their control-flow state. `control-flow-acceptance.md` maps return/break/continue/propagation and unreachable operand checks. Provider cleanup remains a runtime try/finally seam, not provider checking. |
| Lazy tasks | Async call arguments evaluate once at construction; task bodies run on each await. `call_order.ald` mutates arguments afterward, awaits the task twice, and replaces a callee across an awaited argument. The explicit_async fixture separately checks lexical captures and generic dictionaries across suspension. |

## Confirmed Coalesce defect

The language guide already promises that `??` unwraps an Option with a default.
Inference instead required both operands to have the same type. A compiled
`Some(42) ?? 0` regression failed with expected Number, found Option[Number].
Lowering also used loose null equality and returned the present representation
without unboxing it: Some(unit) could execute the fallback, and nested Options
could retain an extra representation box.

Inference now constrains the left operand to `Option[a]` and the default/result
to `a`. Lowering evaluates the left once, tests strictly for null, executes the
fallback only for None, and calls the centralized `$optionUnbox` only in the
present branch. The default itself is never unboxed. Output is built directly
as Oxc AST nodes.

Two solver regressions cover inferred/annotated polymorphism, nested Options,
unit, and invalid operand/default types. The source-aware codegen snapshot
`coalesce_unwraps_only_the_present_branch` was reviewed for exact null detection,
branch-local effects, and one-layer unboxing. Actual CLI cases cover nested
Some(None), an absent outer Option whose fallback is Some(None), imported
generic calls at Number/String, early return, Result propagation, and await.

The focused codegen snapshot and actual standalone CLI suite pass. Formatting,
strict all-target/all-feature Clippy, and full workspace tests/doctests pass
(70 codegen, 199 driver, 72 kernel, 17 CLI, and 467 inference tests). No pending
snapshots remain. These results describe the integrated worktree, not an
isolated checkpoint or a freshly verified release package.
