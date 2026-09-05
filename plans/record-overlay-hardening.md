# Independent record overlay implementation

Status: transport, deferred inference, partial-field exposure, and several
contract checks are implemented and tested. Full constraint entailment and
cycle acceptance remain unfinished. This is part of requirement
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
record, independently of when input tails become known. A later required field
replaces earlier payloads; an optional field preserves the earlier fallback
when absent. Fields not overwritten remain present. Known absence, possible
absence, and required presence are distinct. The recent optional/open-overlap
regressions remain required, including final overrides that discard otherwise
incompatible intermediate alternatives.

## Implementation direction to validate

Transport checkpoint (uncommitted): `Annotation.record_overlays` stores ordered
operand types, a result type, and a source region. Arena copying and owned
dehydration/hydration preserve the relation. Owned interface format is now 4.
The transport regression now obtains metadata from an inferred generic merge,
serializes it, destroys the hydration arena, copies it to a new arena, and
verifies exact round-trip equality. Reversing operand order
changes the semantic fingerprint. Workspace checking and strict Clippy pass.
Inference now emits overlays for multiple open operands, selects their connected
variables during generalization (alongside error-row relations), preserves them
in free-variable accounting, instantiates them with the scheme's replacement
map, and publishes/hydrates them through annotations. A fixed-point pass solves
overlays once all operand shapes are closed, using the existing optional/final-
overwrite rules. The original 207 solver tests passed before adding the new
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
6. Optional-field codegen depends on solved read-site presence. Do not finalize
   those sites before overlay solving; avoid a generic function changing its
   runtime representation according to a caller-only specialization.
7. Termination and occurs checks must cover overlay cycles. Diagnostics from
   imported constraints must label the current consumer's source, not stale
   producer regions.

The existing `Scheme.error_row_inclusions`, connected-component selection in
`generalize_global`, `instantiate_scheme`, annotation hydration, and owned
interface conversion provide integration examples, not interchangeable
semantics. Their directed set-inclusion solver cannot simply be reused for
ordered property replacement.

## Acceptance work

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
