# Option constructor and pattern hardening

Status: built-in registration, representation-aware lowering, and guarded
refutable binding implemented; broader pattern/pin audit remains open.

Match-pattern permission is now explicit in canonicalization, rather than a
depth counter surrounding whole arms. A reproduced nested let pin was accepted
outside an actual match pattern; it now reports `PinOutsideMatch`. The same
boundary controls unqualified constructor lookup. Expression and markup match
patterns both use the explicit mode, including alternatives. Regression tests
cover nested let and lambda parameter rejection, and allow a lambda's own nested
match to use pins. The source-aware error snapshots identify the offending pin.

The language documentation places Some/None in the prelude and supports
Option::Some/Option::None. The canonical environment registered Result and
Ordering constructors, but not Option. An actual CLI module declaring
`let absent: Option[Number] = None` failed with unknown name None.

Option constructors now have arena-owned polymorphic annotations and canonical
builtin identity. Some lowers directly to the existing $optionSome helper;
None lowers to null. Pattern tests compare against null and extract exactly one
layer with $optionUnbox, including unit, nested None, and user enums named Some.
No new Option representation or runtime ABI was introduced. Direct AST emission
is preserved.

The actual CLI fixture `tests/e2e/traits/src/option_constructors.ald` covers
qualified/unqualified constructors, first-class constructor references,
None/Some/Some(None)/Some(()) matches, and present-value destructuring. The main
traits fixture also checks a qualified Option constructor wrapping a qualified
user-defined Some enum. A reviewed source-aware codegen snapshot checks nested
extraction; solver cases accept exhaustive patterns and reject wrong payload
types. Local let aliases remain monomorphic under the current generalization
policy; the fixture uses independently instantiated constructor references.

## Refutable binding safety

Code inspection found a broader issue: `bind_pattern` extracts payloads without
testing refutable patterns at its non-match callers (function parameters,
lambda parameters, top-level/local lets, and for-loop bindings). For example:

```alder
fn unwrap(value: Option[Number]) Number {
    let Some(number) = value
    number
}
```

The actual CLI reproduction printed null and exited successfully before the
fix. Non-match binding sites now use checked_bind_pattern: refutable patterns
run their existing pattern decision before any bindings are exposed, and failed
decisions call the existing source-located $matchFailure helper. Irrefutable
bindings retain direct extraction. Match arms retain their existing guard/test
path and do not double-check bindings.

The permanent pattern_bindings CLI fixture covers failed local and top-level
lets, function/lambda parameters, for bindings, array length, nested aliases,
same-arity enum variants with different payload types, async parameters, and
async bodies. Successful cases verify exactly-once source evaluation and lazy
async parameter checking. A small maintained JS probe executes a reusable task
twice and verifies its finalizer runs once per failed binding, before the defect
is observed. The test has bounded timeouts. A reviewed source-aware codegen
snapshot verifies failure precedes Option payload extraction.

The guard reuses the existing match decision logic rather than introducing a
second pattern implementation.

## Pin lexical scope checkpoint

An actual CLI test reproduced `(value, ^value)` resolving the pin to a new local
that had not yet been initialized, producing ReferenceError. Pattern traversal
now preserves pre-pattern lexical scopes for every nested pin expression.
Matched bindings remain visible in the guard and body, but do not change what
the pin resolves to. Pins with no enclosing binding report unknown name at the
pin operand. Fresh local/use identities and assignment tracking remain shared
through the temporary pin environment.

CLI cases cover both tuple orders, record fields, nested Some patterns, aliases
whose inner/outer types differ, and exactly-once pin calls in source order after
scrutinee evaluation and before a failing guard. Reviewed canonical error and
codegen snapshots verify source regions and distinct outer/new binding IDs.
Match-only permission leakage was subsequently addressed by explicit binding
modes, committed in `743a09c`. Broader static exhaustiveness remains an audit
item; these tests are not proof of the entire pattern implementation.

## Alternative binding checkpoint

`First(value) | Second(value) => value` compiled but crashed with ReferenceError
when Second matched: each alternative had fresh local identities while the
guard/body referenced only the first pattern's identities. Alternatives now
reuse the first pattern's local IDs by name, while preserving independent
pattern scopes and pre-pattern pin lookup. Different sets of bound names produce
a source-aware canonical diagnostic, including extra or missing bindings.
Inference unifies the types of corresponding bindings rather than overwriting
them, including aliases and array rest names. This prevents a String alternative
from supplying a binding checked as Number through another alternative.

CLI regressions execute both alternatives, guarded fallback, array-rest
bindings, aliases, and escaping closures. Solver regressions reject incompatible
plain, alias, and rest payload types. Reviewed colorless diagnostics show missing
and extra names; a source-aware emission snapshot shows the same binding ID in
each alternative and the shared guard/body. Expression and markup match
canonicalization use the same alternative-binding mode; deferred markup runtime
behavior is not claimed tested by the standalone fixture.

The existing documented decision chain retries guards per alternative, not
per arm. CLI regressions now verify overlapping alternatives with different
bindings: a false first guard retries with the second binding; a true guard
stops; two false guards fall through; a failed pattern skips its guard. An
awaiting guard preserves the same order across host-timer suspension. Recorded
effect arrays assert exact invocation order and count without timing assertions.
No semantic change was needed for these cases.

The solver currently performs static exhaustiveness checking only for Result
error rows, not ordinary enum/Option matches. Ordinary failed matches lower to
$matchFailure. The initial additional Option static-exhaustiveness expectation
was removed because it assumed a guarantee not provided by that existing pass;
the current tests do not claim general static exhaustiveness.

The recursive/cyclic value-operation audit remains open; this checkpoint does
not establish those laws.

Nested pin short-circuiting: pattern tests previously concatenated all nested
prefix statements before evaluating their combined Boolean test. This ran pin
effects even when an enclosing Option/array/constructor or earlier subpattern
failed. A shared test-composition helper now places effectful test prefixes
behind the preceding Boolean checks, using direct Oxc conditional statements.
Pure tests retain the compact logical conjunction. The CLI regression failed
before the fix and now checks absence, empty arrays, constructor mismatch,
earlier tuple/record mismatch, reached-pin order, and skipped/executed awaiting
pins. Reviewed codegen snapshots show nested shape checks guarding pin calls.
