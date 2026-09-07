# Option constructor and pattern hardening

Status: built-in registration, representation-aware lowering, and guarded
refutable binding implemented; broader pattern/pin audit remains open.

Current checkpoint validation: all 63 codegen tests and doctests pass, as do
the CLI's ten refutable-binding failure cases and success/cleanup mode, the
standalone e2e fixtures, formatting, and strict workspace Clippy. Five source-
aware codegen snapshots were reviewed. The previous full workspace run covers
this same production source; selective staging did not change its behavior.
Elm's decision-tree match-before-extraction structure was rechecked as a
reference; Alder retains its ordered effectful-pin semantics and direct Oxc ASTs.

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

## Nested recursive dictionary evidence

The current audit reproduced an undefined `$self` reference for a monomorphic
`Node` enum with an `Array[Node]` field. Direct recursive field evidence was
rewritten to the emitted dictionary name, but recursion inside container or
structural evidence fell back to the generic factory's `$self` identifier.
Eq, Show, and Hash were all affected, including acyclic values.

Derived-field lowering now scopes the emitted self-dictionary name through
the entire recursive evidence walk and restores the previous context afterward.
Generic factories retain their `$self` binding; ordinary trait method lowering
keeps its existing default. A codegen regression first failed on the free
identifier and now passes. The compiled `hash_equality` fixture executes Eq,
Show, and Hash through `Array[Option[Node]]`, including distinct equal trees and
an unequal tree. This is direct AST lowering, not source substitution.

Validation: all 68 codegen, 184 driver, and 16 CLI tests plus associated
doctests pass. Strict workspace Clippy, formatting, and whitespace checks pass.
The new codegen fix and changeset remain uncommitted; this validation does not
include a successful cyclic-equality case.

### Cyclic derived equality

After the dictionary-reference fix, this valid program reproduced a separate
stack overflow before the active-pair fix below:

```alder
#[derive(Eq)]
enum Node { Link(Array[Node]) }

pub fn main() {
    let children: Array[Node] = []
    let node = Node::Link(children)
    array.push(children, node)
    assert node == node
}
```

The actual CLI reproduction is `/tmp/alder-cycle-probe.iJC3Iw`; it now exits
successfully. Equality had looped through `$equalDerived` and the array payload
dictionary without detecting an active comparison pair.

Derived Eq calls now carry their canonical nominal type name. During one
synchronous comparison, the kernel tracks active left/right pairs by nominal
name. Revisiting an active nominal pair closes that recursive comparison;
other fields still run their selected dictionaries. Returning or throwing
removes the pair, and exiting the outer comparison clears the session. There
is no object-identity success shortcut or cross-call result cache.

Erased Option/container pairs are deliberately not guarded: the same runtime
pair can occur at different nested Option layers before reaching a payload.
Kernel tests verify distinct equal cycles, symmetric unequal cycles, NaN
inequality even for the same object, mutation between comparisons, cleanup
after a throwing payload dictionary, and distinct nominal comparison domains.
Compiled CLI cases cover cyclic Array/Option payloads and independently
instantiated generic cycles with Number and String payloads. The twelve affected
codegen snapshots now include canonical nominal names in derived Eq calls;
the recursive Chain snapshot retains its emitted dictionary reference.

This adds cycle detection, not a trampoline for arbitrary-depth acyclic values.
Mutually recursive generic Left/Right graphs now have actual CLI coverage for
Number/String payloads, symmetric equality/inequality, and graph mutation after
a successful comparison. Hash/Show/JSON/Ord cycle behavior is now implemented;
see `docs/cyclic-values.md` for the approved marker/error policy and verification.
No scheduler or Effect protocol changes are involved;
the pinned runtime reference was rechecked for scope and no code was adapted.

Validation after the active-pair change: all 60 kernel, 16 CLI, 68 codegen,
and 184 driver tests and associated doctests pass. Strict workspace Clippy,
formatting, and whitespace checks pass. The compiler/kernel ABI change and
its changeset are uncommitted and require fresh release packaging.

Nested pin short-circuiting: pattern tests previously concatenated all nested
prefix statements before evaluating their combined Boolean test. This ran pin
effects even when an enclosing Option/array/constructor or earlier subpattern
failed. A shared test-composition helper now places effectful test prefixes
behind the preceding Boolean checks, using direct Oxc conditional statements.
Pure tests retain the compact logical conjunction. The CLI regression failed
before the fix and now checks absence, empty arrays, constructor mismatch,
earlier tuple/record mismatch, reached-pin order, and skipped/executed awaiting
pins. Reviewed codegen snapshots show nested shape checks guarding pin calls.
