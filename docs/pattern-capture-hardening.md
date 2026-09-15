# Pattern capture under mutation

Status: committed in integration `556a21c`; final release gates remain open.

## Confirmed defect

Pattern tests checked an Option field as Some, then evaluated a later sibling
pin which replaced that field with None. `bind_pattern` subsequently reread the
original extraction path, so the supposedly Number-valued binding received None.
The compiled CLI regression's `assert(result == 42)` failed before the fix.

The invariant is that a successfully tested payload is the payload supplied to
its binding. Source evaluation proceeds left to right. A pin can mutate shared
state, but cannot retroactively replace an earlier captured match value.

## Lowering

`prepare_pattern` owns a per-pattern root and extraction-path map. Captures are
declared outside the conditional test chain and assigned only as their subpattern
is reached, behind parent and preceding-sibling checks. Child extraction starts
from the nearest captured parent. Tests and eventual bindings reuse those values.
Pins capture their subject before evaluating the pinned expression.

Array-rest capture performs the existing shallow slice at that pattern position.
Other object captures preserve reference identity. Alternative attempts get
independent maps and recapture after a failed guard. Pattern preparation saves
and restores outer compilation state; unchecked binding preparation temporarily
clears it so nested bindings cannot reuse another root's paths.

This changes neither the pin scope rule nor flow targets: pins are legal only
inside match patterns, refer to outer bindings, and may return/break/propagate
according to the surrounding body. Exploratory let/parameter-pin fixtures were
rejected correctly and removed, not used to justify expanding the language.

## Evidence

- `later_pin_mutation_cannot_invalidate_captured_payloads` is a source-aware
  direct-AST snapshot showing the Option payload captured before mutation.
- `examples/control_flow/src/pattern_capture.ald` executes the original failure,
  array-rest copying before later mutation, nested alias identity, and an awaited
  pin which replaces an already-matched Option.
- Existing CLI pattern cases exercise failed shape gates, skipped sibling pins,
  guard retries, async guards, outer pin scope, and nonlocal exits.
- Eight existing snapshots were reviewed for gated extraction, payload reuse,
  and per-alternative binding scope, then updated. The new snapshot was reviewed
  against the concrete mutation invariant, not accepted merely because it changed.

Focused CLI and 69 codegen tests pass, along with strict workspace Clippy and
full workspace tests/doctests (194 driver, 72 kernel, 17 CLI, 465 inference).
Formatting and whitespace checks pass; no snapshots are pending. This finite
matrix does not close all codegen evaluation-order or source-fidelity acceptance
requirements. Validation is for the integrated worktree, not an isolated commit.
