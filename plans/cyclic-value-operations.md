# Cyclic value-operation audit

Part of compiler hardening requirement 9 and the related runtime audit. The
compiler permits cyclic heap graphs through recursive enums and mutable arrays;
do not remove those aliasing semantics to simplify value operations.

## Current equality evidence

The nested recursive evidence-binding fix removes free `$self` references in
monomorphic derived dictionaries. Active nominal-pair tracking then prevents
repeated traversal of an already-active cyclic comparison, while still invoking
payload dictionaries. No completed result is cached across comparisons.

The `hash_equality` CLI fixture includes generic/non-generic cycles and nested
Option payloads. Its `mutual.ald` module adds independently allocated mutually
recursive Left/Right graphs with Number and String payloads. It checks reflexive
and symmetric equality, unequal payloads, and mutation of the graph after a
previous successful comparison. The focused CLI regression passes. Direct
kernel checks cover NaN, exceptions, and distinct nominal comparison domains.
All 16 CLI tests and associated doctests pass with the new mutual-recursion
fixture; formatting and whitespace checks pass. No production change was needed
for this additional coverage.

These are cycle-detection checks, not stack-safety claims for arbitrary-depth
acyclic values or exhaustive equivalence proofs.

## Approved user decision

The existing Hash byte format and JSON representation describe nested values,
not graph back-references. The user approved the following policy:

- Show: display a cycle marker.
- Derived Hash, Ord, and JSON encoding: report explicit runtime errors when
  the operation encounters an active cycle.
- Eq: retain cycle-aware structural comparison and payload semantics.

This policy is approved. Derived Show/Hash/Ord/JSON now track active traversal
paths, emitting `<cycle>` for Show and `TypeError("<operation>: cyclic value")`
for the other three. Structural Show and Hash also track recursive visits.
Container and source-level Ord acceptance now have compiled regression coverage,
as mapped in `docs/cyclic-values.md`; this is not a claim that the full
value-operation audit is finished.
Full graph hashing/ordering/serialization would need separate definitions,
especially compatibility with equality rather than accidental allocation or
graph-traversal identity. Never silently invent such formats or narrow a
documented promise. Preserve existing acyclic outputs.

The operation reproductions, bounded regressions, derived/container delegation,
cleanup, shared-acyclic checks, and semantics documentation are now present.
Final integrated verification, coherent commits, and package gates remain.

## Derived-operation implementation evidence

Four granular kernel regressions failed before the change (Show exhausted the
stack; Hash/Ord/JSON failed the explicit-TypeError assertion) and pass afterward.
They exercise repeated cyclic calls, mutation back to an acyclic graph, payload
exceptions inside the active traversal, and shared versus duplicated acyclic
children. Tracking uses operation/domain-specific weak sets and `finally`
cleanup, never completed-result caching. The synchronous ABI is unchanged.

The `hash_equality` CLI fixture now includes `cycles.ald` and a local JS helper
that checks exact error class/message. It executes derived Show/Hash/JSON on
an Alder-created generic enum cycle through Array and Option, nested Option
showing, subsequent mutation, and shared acyclic children. Its first run exposed
a missing explicit unit annotation in the test extern; correcting that fixture
made the CLI regression pass without further production changes.

Source/container follow-up: compiled Ord crosses an explicit user-defined
Children ordering, mutable arrays, derived OrderedNode, and nested Options.
It reports the expected cycle defect and compares successfully after mutation.
Mutually recursive Left/Right enums exercise record and positional variants
through Array/Result, then check Show/Hash/JSON after replacing the cyclic edge
with Err. The initial fixture incorrectly used String as a Result error type;
changing it to the approved `[:finished]` error row made it valid. No production
change was needed. A fifth kernel regression covers structural Show/Hash on raw
array and record cycles, then mutation and shared-versus-copied children.

Remaining: final integrated verification, docs/changeset reconciliation, coherent
commits, package gates, and the broader requirement-9 operation/law audit.

Follow-up validation is terminal and green: full workspace tests/doctests
(68 kernel, 17 CLI), strict all-target/all-feature Clippy, formatting, and
whitespace checks. No pending snapshots. The structural kernel regression and
`docs/cyclic-values.md` were committed as a focused checkpoint; expanded source
fixtures remain part of the pending integration. No production changes were
needed for this follow-up, so packaging remains due for the earlier cycle-guard
and unbounded-value source changes rather than a new runtime implementation.

Validation: full `cargo test --quiet` completed with exit 0 (67 kernel and 17
CLI tests, plus the complete workspace and doctests). Strict all-target/all-feature
Clippy, formatting, and whitespace checks pass. Validation covers the integrated
worktree; the cycle guards, four kernel regressions, and changeset were committed
as a separate checkpoint, while CLI fixtures remain in the ongoing integration.
The initial zero-context selective staging misplaced added hunks; the local
commit was corrected immediately without changing the tested worktree. The
corrected staged kernel passed Node syntax checking and all four exact staged
regression harnesses before amendment. Future selective staging must preserve
context or reconstruct from the parent, not reuse new-line offsets from a
larger worktree diff after dropping intervening hunks.
