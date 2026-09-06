# Operations on cyclic values

Alder's shared-reference mutation can create cyclic values, for example an enum
containing an array that later receives that enum. This does not change aliasing
semantics or make arbitrary-depth synchronous traversal stack-safe.

Compiler-provided operations handle a repeated value on their active traversal
path as follows:

- Eq uses cycle-aware structural comparison while retaining payload equality.
- Show renders `<cycle>` at the recursive edge.
- Derived Hash, Ord, and JSON encoding throw a TypeError whose message is
  `Hash: cyclic value`, `Ord: cyclic value`, or `JSON: cyclic value`.

These errors are runtime defects, not typed Result errors. No graph-reference
hash, ordering, or JSON wire format is introduced. Acyclic outputs are unchanged.
Shared children in separate branches are traversed normally, not treated as
cycles. Active tracking is cleared after success or exceptions, so mutation and
later calls cannot reuse stale results.

Cycle checks follow the operation's traversal, not a preliminary scan of every
reachable object. For example, ordering can decide on an earlier differing
constructor or payload without inspecting a later cyclic field. User-provided
instances retain their own semantics; delegating back into a derived operation
participates in its active traversal. Checks do not turn arbitrary recursive
user code into a terminating algorithm.

Option representation remains centralized in its payload operations. An erased
Some and its payload may be the same JavaScript object, so container delegation
is not itself a recursive graph edge. Derived and structural traversal domains
are kept separate for this reason.

## Verification

The evidence below includes the integrated hardening worktree and its CLI
fixtures; it is not isolated-commit or final release verification.

`crates/alder-kernel/src/lib.rs` includes four granular derived-cycle tests and
`structural_cycle_guards_allow_shared_children_and_later_mutation`. They cover
repeated calls, payload exceptions during traversal, mutation, and shared versus
duplicated children; structural Show/Hash additionally cover raw arrays/records.

`tests/e2e/hash_equality/src/cycles.ald` executes generic enum cycles through
Array/Option, nested Options, mutual enum cycles through Array/Result, and
derived Ord through a user-defined Children ordering and Option dictionaries.
The sibling JS helper checks the exact error class/message. Both cyclic errors
and successful operations after removing the cycle are checked.

These regressions and source-path checks establish the selected cycle policy,
not exhaustive runtime correctness. Final integrated packaging and release gates
are tracked in `plans/compiler-hardening.md`.
