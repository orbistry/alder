# Independent record overlay implementation

Status: transport, deferred inference, partial-field exposure, and several
contract checks are implemented and tested. The three joint-audit criteria below
now have source-path reviews and regression evidence. Final integrated compiler
acceptance and clean committed-tree gates remain open. This is part of requirement
8 of compiler hardening, not a new language milestone or a waived limitation.

## Reproductions

The two original solver reproductions now pass:

- `independent_open_spreads_preserve_disjoint_fields`: `merge({ x: 20 },
  { y: 22 })` previously failed with missing `y`.
- `independent_open_spreads_preserve_right_biased_overwrites`: `merge({ value:
  42 }, { value: "right" })` previously failed with String/Number mismatch.

Both use the ordinary inferred definition:

```alder
fn merge(left, right) { { ..left, ..right } }
```

The original `Infer::infer_record` unified the inherited tails of both spreads.
That requires the inputs to share their residual shape and overlapping payload
types. The runtime instead copies properties in source order. Removing the
unification and keeping either tail would lose the other input's fields and
would not be a fix.

Elm's `Type/Unify.hs`, `unifyRecord` and `gatherFields`, were inspected again.
They reconcile equality of record types, including residual rows. They do not
provide Alder's ordered two-input overlay operation.

## Required invariant

Preserve an ordered relationship between every operand and the resulting
record, independently of when input tails become known. Every declared field
exists with its ordinary type; optional shorthand means Option. A later field
replaces earlier values even when it contains None. Fields not overwritten
remain present. Fresh contextual defaults are earlier operands, never fallback
branches in field reads. Preserve final overrides that discard otherwise
incompatible intermediate types. This supersedes historical presence/fallback
descriptions in the original investigation log below.

## Implementation direction to validate

### Recursive universal field-contract follow-up

`recursive_overlay_cannot_prove_a_universal_field_from_its_result_obligation`
rejects a Number promise when a recursive merge feeds its result back into the
left parameter and an independent universal right row may overwrite `value`.
The diagnostic is specifically GenericSpecialization. The positive test
`recursive_overlay_preserves_a_final_field_with_a_shared_universal_tail` writes
Number after each merge and preserves a shared residual tail, accepting a String
input field that is always overwritten before the recursive call.

The initial positive fixture was not a valid universal contract: placing the
overwrite after the recursive operation did not prevent the operation itself
from feeding a changed payload into its monomorphic recursive parameter. Moving
the overwrite inside still required compatible row shapes: an independent right
tail could add fields absent from the declared left tail. The final fixture
uses matching known fields and a shared universal tail. No production defect
was established or fixed by those rejected fixtures. Both final tests pass and
preserve the distinction between an overwritten value and a recursive parameter
contract. They supplement, rather than close, the general cyclic review.

### Mutual-recursion follow-up

Two focused solver tests extend the single-function recursive cases to an
even/odd mutually recursive SCC. An empty overwrite cannot hide incompatible
recursive payload growth: it rejects with InfiniteType or a payload mismatch.
A required `nested` overwrite accepts independent Number and String calls.
Both tests pass without a production change. The first positive fixture used
a Bool seed in the String recurrence and correctly failed; it now uses a String
seed, preserving the recursive parameter's existing type contract.

This is mutual-SCC inference coverage, not stored-interface/runtime evidence for
these new cases and not a proof of general symbolic/cyclic entailment. Those
remaining obligations are unchanged.

Stored/runtime follow-up now supplies the missing boundaries for these specific
cases. `stored_mutual_overlays_preserve_recursive_payload_requirements`
serializes and drops the producer, then reloads it independently for valid and
invalid consumers. Independent Number/String calls succeed; the empty-overwrite
consumer fails without publishing an interface or artifact. Its reviewed
colorless snapshot embeds the actual consumer source and labels the imported
call, not the producer. The `record_options` CLI fixture imports
`mutual_overlays.ald` and executes both entry functions at zero and several
recursive steps. Its dedicated CLI integration test passes. No production
change was needed; general symbolic/cyclic entailment remains open.

### Trait method contract boundary

Two new solver regressions exercise deferred overlays across trait schemes.
Both a default method and an override reject returning `merge(left, right).value`
as Number when the independently universal right row may contain a String
`value`. Rejection is specifically GenericSpecialization, not an incidental
parse or missing-instance error. The positive counterpart adds a guaranteed
final Number overwrite and accepts both default and overridden methods with
independently instantiated extra fields and incompatible discarded payloads.
Both tests pass without a production change. They close this specific
trait-method promise check; arbitrary symbolic/cyclic entailment is not inferred
from these results, and stored/runtime counterparts remain separate evidence.

Stored/runtime follow-up: the new driver test
`stored_trait_overlay_methods_preserve_independent_row_arguments` serializes
and drops the producer, then checks default/override calls through a fresh
consumer with independent extra fields and discarded overwrite payloads.
The traits fixture's `overlay_contracts.ald` executes both method dictionaries
from its importing main module, with Number and String markers and different
row instantiations, and checks both input records remain unchanged. The fresh
packaged CLI at `/tmp/alder-package-current.xhI60N/debug/alder` runs this expanded
fixture successfully from `/tmp`; the stored-interface test also passes.
These supply the corresponding positive stored/runtime evidence without a
production change. They do not claim general symbolic/cyclic entailment.

### Identity and recursive-instantiation probes

New source-level negative probes reject contradictory reads from an input and
its shallow-copy result, including repeated input spreads and surrounding empty
spreads. Recursive-instantiation probes pair an empty overwrite (which cannot
break recursive payload growth) with a guaranteed `nested: Number` overwrite
(which can). The initial positive fixture incorrectly supplied a Number where
the recursive parameter already required a record; it was corrected to a
consistent initial parameter shape, not by changing compiler behavior.

A serialized/dropped producer interface is tested with fresh positive and
negative consumers. Reviewing the first negative diagnostic exposed an unrelated
missing `marker` field, so that fixture now uses an empty overwrite to target
the recursive payload constraint. Imported CLI cases exercise the positive
Number and String instantiations with zero and several recursive steps.
The refined negative reports Number versus `{ nested: Number }` at the imported
consumer's call, rather than an unrelated missing field. Its colorless snapshot
contains the actual consumer source. All 459 solver integration tests, 14 solver
unit tests, 184 driver tests, and 15 CLI tests pass, including associated
doctests and the new execution cases. Strict workspace Clippy, formatting, and
whitespace checks pass with no pending snapshots. These probes do not establish
general symbolic/cyclic entailment or change the implementation.

The first broad validation attempt encountered stale compiler artifacts after
the isolated Hash checkpoint shared the worktree target directory. Worktree
types were confirmed present; affected compiler-crate build artifacts were
cleared and validation passed after rebuilding. Future isolated source exports must use a distinct
target directory, not the worktree's target.

Transport checkpoint (uncommitted): `Annotation.record_overlays` stores ordered
operand types, a result type, and a source region. Arena copying and owned
dehydration/hydration preserve the relation. The initial wire format was 4;
the current format is 6 after tuple-shape and ordinary Option-field migration.
The transport regression now obtains metadata from an inferred generic merge,
serializes it, destroys the hydration arena, copies it to a new arena, and
verifies exact round-trip equality. Reversing operand order
changes the semantic fingerprint. Workspace checking and strict Clippy pass.
Inference emits overlays whenever any operand remains open, selects their connected
variables during generalization (alongside error-row relations), preserves them
in free-variable accounting, instantiates them with the scheme's replacement
map, and publishes/hydrates them through annotations. A fixed-point pass solves
overlays once all operand shapes are closed, using ordinary rightmost-overwrite
rules. The original 207 solver tests passed before adding the new
negative contract test below. Strict Clippy passes. Do not commit or present
this initial integration as a completed fix.

The previously failing
`independent_open_spreads_cannot_strengthen_a_declared_universal_contract`
now passes: a function with `{ r | value: Number }` and
`{ s | other: Bool }` inputs returns `merged.value` as Number, but universal
`s` may contain `value: String`. The unresolved overlay escapes generic contract
validation without proving the declared promise. Field checking now walks
operands right-to-left: a required rightmost field proves the payload, while an
unrestricted universal tail cannot acquire an implicit field restriction. These
payload checks run before any generic contract is finalized, so later unification
cannot specialize an already-validated contract. A second reproduced failure
allowed independent universal input tails to masquerade as the left result tail;
that case now fails, while a shared-tail result remains valid. All 211 solver
integration tests pass, along with strict Clippy.

Next work must address partial-shape propagation, deeper overlay dependencies,
unresolved non-generalized constraints, full tail-equation entailment, and
solver interactions before further acceptance claims. Do not simply reject
every unresolved overlay: inferred generic merge itself must remain usable.

Use an explicit deferred record-overlay relationship, separate from ordinary
row equality and error-row inclusion. Do not encode overlay by equating input
tails or dropping one of them. An ordered operand list and a result type can
retain intermediate relationships without prematurely merging unknown fields.

Before choosing the final representation, account for all these boundaries:

1. Inference must retain the relationship while operands are unresolved, expose
   enough result shape for subsequent field reads, and discharge it when input
   shapes become known. Constraints from those reads must not arbitrarily
   require a field on both inputs. Right-hand overwrite priority is essential.
2. Generalization must collect connected overlay variables, including hidden
   intermediates, without generalizing captured mutable state. Local aliases,
   recursive groups, higher-order forwarding, and factories must retain the
   same relationship.
3. Instantiation must rename all overlay variables with the same replacement
   map as the function type. Distinct calls cannot share inference state.
4. Declared universal contracts must not acquire hidden restrictions merely
   because an overlay cannot yet be resolved. Validate the contract rather
   than exporting an unchecked extra assumption. Distinguish legitimate
   inferred constraints from unproved declared promises.
5. Published annotations and owned interfaces must retain the relationship.
   If annotations gain a new constraint field, audit every constructor, copy,
   free-variable traversal, replacement, trait scheme comparison, and
   serialization path. Version the wire format when its representation changes.
6. Field reads use the stored type directly. Constructor defaults and contextual
   lifting metadata must agree with overlay solving; a generic function cannot
   change its runtime representation according to a caller-only specialization.
7. Termination and occurs checks must cover overlay cycles. Diagnostics from
   imported constraints must label the current consumer's source, not stale
   producer regions.

The existing `Scheme.error_row_inclusions`, connected-component selection in
`generalize_global`, `instantiate_scheme`, annotation hydration, and owned
interface conversion provide integration examples, not interchangeable
semantics. Their directed set-inclusion solver cannot simply be reused for
ordered property replacement.

## Acceptance work

### Joint-audit criteria

Keep these distinct when assessing completion; passing a recursive example
alone does not discharge all three:

1. **Transport/retention reviewed:** unresolved inferred relations must remain
   attached to their connected scheme
   variables and be instantiated together, including monomorphic local aliases
   and captured state. A concrete consumer must not silently drop an unsatisfied
   relation when it becomes executable.
2. **Reviewed:** declared universal promises must follow from the declared input shapes, not
   from fields introduced as obligations on an opaque intermediate result.
   Current normalization deliberately does not treat opaque cyclic producer
   fields as overwrite evidence.
3. **Reviewed:** cyclic relations must terminate and retain payload/type relationships.
   The solver's cycle-aware expansion and existing single/mutual recursive
   tests are evidence; the source review below distinguishes a valid
   deferred relation from an unchecked contradictory equation.

These restate the existing inference, contract, and occurs-check requirements;
they do not add a requirement for a general theorem prover or waive any
previously identified gap. The existing transport, source, stored-interface,
and runtime regressions should be used as evidence for their actual boundaries,
not repeatedly described as missing coverage.

### Universal and cycle source review at 21b4510 plus the integrated worktree

The acceptance review distinguishes constraint retention from proving a declared
promise. `check_universal_overlay_fields` follows ordered producer operands,
checks expected fields right-to-left, and rejects an unconstrained universal tail
before treating a possibly overwritten earlier field as evidence. Expected
universal result tails are also checked against independent operand universals.
Known selected payloads go through `check_value`; final generic-contract checks
reject specialization, identification, and escape after the joint solving loop.
This covers ordinary and trait method contracts through the same registered
universal-variable path, rather than a separate unchecked trait shortcut.

Normalization preserves input order, removes only shadowed contributions, and
does not use open cyclic producer fields to discard earlier closed writes.
Active-path producer indices stop cyclic expansion, while the visited set avoids
re-expanding a shared DAG. Cyclic result comparisons use the original ordered
inputs rather than assuming associativity proves equality of arbitrary fixed
points. The bounded-expansion unit test and the recursive universal regressions
exercise these distinct paths; the latter include a valid shared-tail overwrite
and an invalid independent-tail promise.

There is no blanket rejection of an open recursive relation. It remains a
constraint, as reviewed under criterion 1. Guaranteed selected fields are checked
against the result by `expose_overlay_fields`; once inputs close, the entire
ordered merge is checked before removing the relation. Those equalities reach
`bind` and its structural occurs check, including record fields and row tails.
`unify_records` also rejects unequal residual fields on an already-shared tail.
Thus stopping graph expansion is not itself acceptance of a cyclic payload
equality. The solver repeats when a relation is discharged, a field is exposed,
or substitution bindings increase, not merely when fresh variables are allocated.

The inspected regression matrix includes single and mutual recursive payload
growth, overwrites that break that growth, independent concrete instantiations,
opaque/universal field promises, default/overridden trait methods, and serialized
producer/consumer boundaries. Fresh runs pass 48 overlay integration tests and
the bounded-expansion unit test, plus all 31 stored-driver selections. The stored
recursive negative cases assert failure without interfaces/artifacts and snapshot
the consumer's source; positive runtime boundaries are recorded above.

This concludes the three specified overlay audit criteria with implementation
reasoning and finite regression evidence, not a theorem of complete constraint
satisfiability, general compiler soundness, or whole-goal release approval.
Historical statements below that those reviews were still pending describe
earlier checkpoints. Broader generic/evidence integration and final validation
must still use the eventual committed source tree.

### Retention audit at 21b4510 plus the integrated worktree

Criterion 1 is supported by a source-path audit, not just the presence of scheme
metadata. `solve_record_overlays` removes a relation only after closed operands
are merged and checked against its result; open relations remain in the pending
set. `generalize_global` selects connected relations to a fixed point, including
otherwise hidden intermediate variables. It subtracts protected environment
variables only after collecting that component, or clears quantification for a
restricted binding. `scheme_free_vars` includes operands and results.

`instantiate_scheme` applies the function type's replacement map to every
operand/result and keeps unquantified variables shared. Annotation export uses
one name map; annotation import uses one variable map. Both set the deferred
constraint's diagnostic region to the consumer reference on instantiation.
Owned conversion, hydration, and arena copying preserve operand order and the
result, rather than reconstructing a relation from the visible function type.

Inspected permanent tests include higher-order forwarding, composed-result
rejection, independently instantiated global aliases, monomorphic local factory
closures, captured mutable arrays, and stored generic/associative consumers.
The stored generic test destroys the producer, copies through another arena,
accepts independent calls, and rejects a concrete wrong overwrite without
publishing an interface or artifact. The metadata test additionally checks exact
round-trip equality and operand-order-sensitive fingerprints.

Fresh runs pass: 46 solver integration tests selected by `overlay` plus its one
unit test, 11 selected by `independent_open_spreads`, all 31 driver tests selected
by `stored_`, and the separate metadata storage/arena-copy test. These selections
overlap other requirements and are not counts of distinct retention properties.
No implementation or snapshot changes were needed. This closes the retention
review; it does not establish universal entailment or the consistency of every
retained cyclic equation, which remain criteria 2 and 3.

### Guaranteed fields on open input records

Reproduced another contradictory contract after the closed-field fixes:
`padded(left, right: { r | x: Number })` returning `{ ..left,
x: "discarded", ..right }` was treated differently from `{ ..left, ..right }`.
A generic caller could promise Number and String for their same inherited
`value`. The regression was accepted before the fix, then correctly rejected.

Right-to-left normalization now also records known fields on acyclic expanded
open input leaves as guaranteed overwrites. It keeps those open records intact
and removes only shadowed earlier closed fields. Cyclic opaque producer fields
may be obligations rather than established input facts, so they are not used
as overwrite proof. This does not resolve general cyclic entailment.

The stored-interface regression rejects the same contradiction after producer
serialization/destruction; its reviewed colorless snapshot labels the second
consumer call, with actual source embedded. Positive solver and cross-module
CLI tests cover an Option-typed guaranteed field containing None, preservation
of unrelated fields, right-hand overwrites of other fields, and exactly-once
evaluation of the discarded initializer. This changes only type normalization;
codegen remains untouched. Final package evidence predates this solver change.

Validation: all 456 solver integration tests, 14 solver unit tests, 183 driver
tests, 14 CLI tests, associated doctests, formatting, and strict all-target/
all-feature Clippy passed. Whitespace checks are clean; no pending snapshots.

### Shadowed closed fields across open operands

Reproduced a further contradictory generic contract: `{ ..left,
..{ x: "discarded" }, ..right, x: 0 }` and `{ ..left, ..right, x: 0 }`
could independently promise Number and String for the same inherited `value`.
The original attempt with two explicit `x` declarations correctly failed
canonicalization; the valid spread-literal reproduction reached inference and
was incorrectly accepted before the fix.

Normalization now removes earlier closed fields guaranteed overwritten by a
later closed field, walking right-to-left before adjacent closed grouping.
Unknown operands remain intact; other fields of partially shadowed closed
records remain intact. This changes only constraint comparison, not runtime
initializers. The solver negative regression passes with all 14 unit and 452
integration tests. Stored-interface positive/negative coverage includes a
partially shadowed record and an intervening open overwrite; CLI coverage
checks final payloads and exactly-once source-order side effects. All 177 driver
tests, 14 CLI tests, associated doctests, and strict all-target/all-feature
Clippy passed after the change; formatting and whitespace checks also passed.
The subsequent full `cargo test --quiet` run exited 0, including all 54 kernel
tests and workspace doctests (two documented ignored doctests). No pending
snapshots were found.
Broader entailment/cycle acceptance
and final-tree packaging remain outstanding.

Adjacent-closed grouping defect: an unused inferred function could require
Number from `split(record).value` and String from `grouped(record).value`, where
split appends `x: 0, y: true` and grouped spreads `{ x: 0, y: true }`. Neither
changes value, but leaf lists retained two closed operands versus one combined
operand, so equality failed to connect the contradictory result constraints.
The new regression failed with successful solving before the fix.

Expanded acyclic operand lists now merge adjacent closed records using ordinary
right-biased field insertion. They never merge across an open operand, and
cyclic equality still uses the conservative direct-input comparison. This is
type normalization only; runtime evaluation/copying is unchanged. The negative
passes after the fix; paired positives preserve rightmost heterogeneous writes
and unknown-row barriers. All 451 solver integration tests and 14 unit tests
pass. A cross-module CLI fixture compares both groupings and checks inherited
and overwritten values. A stored-interface consumer rejects the contradiction
without artifacts/interfaces; its reviewed colorless snapshot labels the local
second call and contains the actual Alder source.

This resolves a concrete entailment gap, not general cyclic entailment. The
previous package checkpoint predates this production change and must be
refreshed before final delivery. All 176 driver tests, 14 CLI tests, and their
doctests pass. Strict workspace Clippy, formatting, and whitespace-aware diff
checks pass; no pending snapshots remain. Added a closed-overlay-normalization
changeset. The full workspace test run predates this production change.

Empty-input normalization finding: inferred padded/plain merges were allowed
to demand Number/String from the same final field because closed empty operands
made their normalized lists different. A permanent negative reproduced this.
Expansion now removes only closed empty records, the overlay identity. Open
rows with no known fields are retained, since they can contribute arbitrary
overwrites. The negative and a positive public generic padded-merge test pass;
the positive instantiates previously unknown middle/right rows with Number and
String overrides. Runtime expressions are still evaluated and copied in source
order; this is type-level normalization only. Full symbolic/cycle entailment
remains separate required work.
Validation: 291/292 solver integration tests pass, with only the known tuple
projection failure; both solver unit tests, all 121 driver tests, strict
workspace Clippy, formatting, and diff checks pass. No snapshots changed.

Repeated-input normalization finding: an unused inferred function required
Number from `{ ..left, ..right, ..left }` and String from
`{ ..right, ..left }`. It was accepted because normalized leaf operand lists
still retained the earlier left occurrence. Expansion now retains only the
rightmost occurrence of each identical input shape; earlier duplicates add no
field-type/presence alternative beyond that occurrence. The negative regression
now rejects. A paired positive verifies `{ ..left, ..right, ..left }` and
`{ ..right, ..left, ..right }` still produce distinct Number/String results.
Both tests and the bounded graph-expansion unit tests pass. The solver-wide run
passes 289/290 tests, failing only the known tuple regression. This extends
acyclic overlay equivalence, not general cycle solving or a codegen optimization:
runtime source evaluation order and actual copying are unchanged.

Associative constraint finding: a new unused inferred function computed
`{ ..{ ..left, ..middle }, ..right }` and `{ ..left, ..{ ..middle, ..right } }`,
then required the first value field to be Number and the second String. The
compiler accepted and published both contradictory overlay constraints because
result equality compared only immediate operand lists. The solver now compares
expanded ordered leaf operands for acyclic overlay graphs, identifying those
equivalent results before generalization. Expansion reports opaque cycles;
cyclic comparisons retain the prior direct-input equality check rather than
assuming arbitrary recursive equations have the same solution. The negative
regression now rejects. A paired positive checks independent Number/String
instantiations and rightmost overrides under both parenthesizations. This does
not discharge general cycle entailment or the remaining interface/CLI gates.
Validation: both new regressions and all 119 driver tests pass; strict workspace
Clippy and formatting pass. The solver-wide run after the negative fix passed
282/283 tests, failing only the already tracked tuple-projection regression.
No snapshot changes were needed for this overlay fix.
Follow-up acceptance: added a colorless driver snapshot with actual Alder source
for the contradiction. Its label identifies the conflicting overlay, although
showing both field-annotation origins remains a diagnostic improvement. A third
solver regression preserves different final Number/String writes rather than
equating every similar composition. The records CLI fixture now imports a
generic function returning both parenthesizations and attempts independent
Number/String calls. This actual CLI run initially FAILED: associated_merges was rejected
with UnresolvedSharedExport, despite the equivalent local inference positive.
The executable fixture exposed late overlay-result unification allocating fresh
residual row tails after SCC generalization. Solve overlay equalities before
computing that SCC's free variables and quantified schemes. The CLI fixture now
passes, as does an isolated public-function regression with no local calls to
incidentally trigger solving. All four focused overlay tests and 120 driver
tests pass; the solver-wide run before adding the isolated export test passed
284/285 tests, failing only the known tuple regression. General symbolic/cyclic
entailment and broader release acceptance remain unfinished.
Generalization safety follow-up: paired regressions reject re-instantiating a
captured shared array through either associated result after a Number writer,
while accepting independent Number/String factories allocating their arrays
inside the function. All six focused associative-overlay tests pass. The actual
records CLI fixture also mutates one associated result and reads through the
other, then checks a separate differently typed invocation leaves the first
array unchanged. CLI execution, all 14 formatter tests, and strict workspace
Clippy pass. These probes preserve shallow alias semantics; they are not proof
of all late-constraint or mutable-interface cases.
Stored-interface follow-up: serialized an inferred public function returning
both parenthesizations, destroyed the producer interface, and built fresh
consumers from the deserialized interface only. Independent Number/String calls
pass. A generic consumer requiring Number and String from the two equivalent
result fields rejects without publishing an artifact or interface. Reviewed the
new colorless snapshot: it retains actual Alder source and labels the consumer's
return contract, not a producer location. This complements the existing arena-
destruction/copy and CLI tests; broad cyclic entailment remains open.
Validation after this probe: all 121 driver tests, strict workspace Clippy,
formatting, and diff checks pass. No pending snapshot files remain.

The entries below record successive investigations, not independent claims of
current failures. Later fixes supersede the earlier reproduction state. The
remaining gates are unresolved non-generalized constraints, symbolic tail and
cycle entailment, trait-scheme boundaries, and final release/commit validation.

Bare-pipe call-site timing: a regression reproduced optional-field access after
`(left, {}) |> merge_pair` failing with Number versus Option[Number]. Ordinary
calls and tagged calls already solved newly closed overlays; the bare-function
pipe path did not. It now runs the same overlay discharge before returning the
call's result type. Imported CLI cases distinguish an absent optional field
from a present nested None through this pipe path.

Spread execution ordering (active, uncommitted): actual CLI execution reproduced
a later block operand's setup being hoisted before an earlier spread copied its
source. Record emission previously concatenated all setup prefixes before the
object literal. It now materializes earlier field expressions before later
setup, and materializes a shallow spread snapshot rather than merely an object
reference. Each snapshot is copied once into the final object, avoiding repeated
copies of an accumulating record. CLI assertions check exactly-once source order,
copy-before-mutation, explicit field observation, and the distinct ordinary-call
argument timing through an imported merge. All 39 codegen tests pass with no
snapshot changes. Broader expression-prefix ordering still needs audit.

Declared tail replacement (active, uncommitted): paired regressions verify a
known String overwrite cannot be hidden behind a return type promising the
unchanged universal input tail. Explicitly declaring the replacement field in
the result accepts an input with a Number at that name and preserves an
unrelated extra field. Both cases pass without additional implementation
changes. This is evidence for known replacement boundaries, not proof of all
symbolic tail equations or cyclic constraints.

Late constraint ordering (active, uncommitted): instrumenting the new three-way
generic optional-Result test showed error inclusions growing from 16 to 20 in
contract checking, after the error solver had already run. Removed that
instrumentation after reproducing the ordering gap. Constraint-producing
overlay contract checks now participate in a joint fixed point with overlay
and error-row solving; final generic rigidity/escape validation happens only
afterward. The loop tracks bindings, pending relation counts, and cached union
targets. The three-way exhaustive positive passes; a negative contract omitting
the third possible error is retained. This closes the observed late-union
ordering gap, not all unresolved-overlay/cycle entailment requirements.

Result union isolation/payload checks (active, uncommitted): independent calls
of the generic optional-Result forwarder retain distinct precise error rows,
each supporting its own exhaustive match. Negative tests reject Array[Number]
versus Array[String] success alternatives and required versus optional record
fields nested inside success arrays. All 240 solver integration tests pass.
These cases exercise instantiation and mutable-payload invariance without
changing the implementation. They do not prove late-added constraint handling
or all cache interactions complete.

Result fallback join implementation (active, uncommitted): `join_values` now
forms an exact error-row union when both alternatives are row-valued Results,
while unifying their success payloads invariantly. It reuses a memoized union
target for the same pruned source-row pair, so repeated partial-overlay checks
do not create unbounded fresh targets. Ordinary non-row Result parameters keep
the existing equality path. Both original fallback regressions now pass, plus
independent generic error-row union and a negative one-source-only contract.
Strict Clippy passes. Added imported CLI cases for absent/present overrides.
Further audit must cover cached unions across instantiations, mutable success
payloads, and constraints introduced during late generic-contract checks; this
is not yet full cross-solver acceptance.

Optional Result fallback investigation (active, uncommitted): added a positive
regression merging required `Result[Number, [:first]]` with optional
`Result[Number, [:second]]`, then propagating the selected value. It initially
failed with `[:second]` versus `[:first]` during payload joining; the paired
negative return contract excluding the fallback error rejected. `join_values`
then unconditionally unified its inputs, preventing a fallback union. The
Result join implementation above fixes this reproduced failure while preserving
success-payload invariance and memoizing union targets. The earlier negative
alone was not evidence of correct propagation because its definition already
failed at the payload join.

Overlay/error-row interaction (active, uncommitted): reproduced a function
propagating a merged Result field accepting a return row `[:first]` even though
the rightmost field supplies `[:second]`. Error-row scheme preparation ran
before overlay-connected variable selection and eliminated the hidden input
error tail as an empty existential. It now protects variables referenced by
residual overlay operands/results before performing that elimination. The
negative regression rejects, the precise selected-row positive passes, and all
233 solver integration tests pass. Other cross-solver interactions and final
constraint acceptance still require audit.

Captured factories (active, uncommitted): added source-level regressions for a
factory returning a lambda that merges its captured left input. Disjoint field
reads pass, incompatible overwrite results reject, and a captured shared Array
cannot be re-generalized to String after Number mutation. An imported CLI
factory executes and mutation through the returned merged record reaches the
original captured array, preserving shallow-spread aliasing. All 231 solver
integration tests and the standalone CLI fixture suite pass. No additional
implementation change was needed for these cases; broader cyclic/unresolved
constraint acceptance remains open.

Shared graph expansion (active, uncommitted): a direct solver regression
reproduced 12 shared binary overlay nodes unfolding into 8,192 leaf operands
during universal contract checking. Expansion now visits the rightmost
occurrence of each producer once, preserving intervening-write order while
avoiding repeated DAG unfolding. Active-path cycles retain the prior opaque
handling; this change does not claim cycle validity. The regression asserts
two retained leaves and the exact ordering around an intervening write, without
wall-clock assertions. All 228 source-level solver integration tests pass.
Producer lookup and repeated constraint comparisons still have polynomial
costs; this is a bounded-expansion fix, not a global complexity claim.

Recursive payload audit (active, uncommitted): a recursive call feeding
`{ nested: merged }` back into the merge's left input rejects specifically with
`InfiniteType` when the right operand is closed and has only `marker: Bool`.
The positive counterpart with a required right-hand `nested: Number` passes:
the overwrite cuts the payload cycle. An initially inferred right operand is
not by itself an infinite-type counterexample, since it can acquire exactly
such an override. Tests retain the definite negative and guaranteed-override
positive, alongside existing recursive forwarding. All 228 solver integration
tests pass. Unknown-tail cycle entailment and bounded expansion remain open;
do not replace that analysis with blanket rejection of recursive overlays.

Repeated-input constraints (active, uncommitted): reproduced an unused inferred
function promising Number and String reads from two merges of the same input
types. Separate residual constraints allowed the contradiction to escape.
Overlay solving now identifies results of equal ordered operand type lists,
retaining both source constraints. Reversing the inputs remains distinct and a
positive regression accepts different right-biased payload types. Solver
iteration also tracks new substitution bindings, since payload equalities can
unlock other constraints without adding a field. Recursive forwarding of
independent inputs passes. All 226 solver integration tests pass. These cases
do not establish general cyclic satisfiability or bounded graph-expansion cost;
those remain open.

Partial field exposure (active, uncommitted): reproduced optional reads failing
when one operand has a universal tail. Eagerly reducing the whole merge with
one open operand was tested and rejected: it adds an unintended constraint on
an unrelated earlier `marker: Bool` field that the open operand may overwrite
with any type. Instead, unresolved overlays now expose only fields proved by
right-to-left operand inspection. A rightmost required field ends the search;
optional fields retain all known fallbacks; an unknown possible overwrite or
fallback prevents exposure. The residual ordered relation remains for future
solving/contract checks. Shape additions drive another fixed-point pass.
All 223 solver integration tests pass, including optional presence, required
fallbacks despite unrelated overwrite types, and rejection of an unproved
optional fallback hidden in a universal row. Cycle/termination and complete
constraint interaction checks remain open.

Call-site shape timing (active, uncommitted): a new regression reproduced
`merge(left, right).value` rejecting a closed optional-Number operand as
Number versus Option[Number]. Overlay solving ran only after field access had
already inferred a required payload. Calls now discharge closed overlays after
argument unification, before reads/destructuring infer the result shape. All
220 solver tests pass, including guaranteed generic rightmost payloads and a
wrong known payload with an unresolved left input. Actual CLI execution checks
imported merges distinguish an absent optional field from a present None.
Reviewed the stored-consumer diagnostic change: the same String/Number mismatch
now arises during the ordinary declared-return check and labels the consumer
function, rather than the later overlay check's call region. The earlier source
fidelity check still rules out stale producer locations. This does not yet solve
overlays with genuinely unresolved operand shapes or generic optional-read ABI
questions; closed-shape availability is only one timing boundary.

Stored-interface audit (active, uncommitted): replaced the synthetic overlay
transport fixture with an actual inferred `pub fn merge(left, right)` scheme.
The test checks that inference publishes the parameter/result relationship,
preserves it through serialization and arena copying after arena destruction,
and fingerprints operand order. A driver consumer test separately serializes
a dependency's generic merge, destroys its producer/hydration/copy arenas, then
checks independent disjoint/overwrite calls using only the owned interface.
A Number consumer of a String overwrite rejects without publishing an artifact
or interface. Its reviewed colorless diagnostic snapshot labels the consumer's
`merge` call, not a stale producer source region. Broader generic/optional and
composed imported contracts remain part of the remaining audit.

Composition audit (active, uncommitted): positive tests now cover nested generic
merges, higher-order forwarding, and a final required override after an
intermediate merge. A negative concrete composed result contract also rejects
correctly. However,
`independent_open_spreads_reject_hidden_contracts_through_composition` reproduces
an unsound acceptance: merge `{ r | value: Number }` with `{ s | other: Bool }`,
then merge the intermediate with `{ marker: true }`, and return `.value` as
Number. Universal `s` can hide a String overwrite. The direct overlay contract
checks do not establish the promise through intermediate relationships. Keep
this regression unignored; do not classify the passing simpler composition
tests as sufficient. The contract checker now expands intermediate overlay
results back to their ordered operands rather than treating an inferred result
shape as independent evidence. The negative regression now rejects, while the
final-required-override counterpart and top-level alias instantiations pass.
All 217 solver integration tests pass. Expansion follows exact pruned result
identity and guards revisits along each expansion path; this is not yet a proof
of cyclic constraint validity, partial-tail entailment, or complete propagation.
Local block aliases are intentionally monomorphic per `docs/language.md`; the
independent-instantiation alias test therefore uses a top-level alias.

- Make both unignored reproductions pass without constraining the inputs equal.
- Test empty/disjoint/overlapping inputs, both source orders, and three spreads.
- Retain optional fallback and final required overwrite regressions.
- Check different calls, stored aliases, higher-order forwarding, and recursion.
- Add negative result/payload contracts and source-aware diagnostics.
- Serialize a generic merge interface, destroy its arena, and check positive
  and negative consumers using the stored interface.
- Execute imported merges through the real CLI and check property values,
  presence, mutation aliasing, and source-order evaluation.
- Run full validation and add changesets for affected publishable crates.

Do not declare this item complete based only on the two initial examples.
