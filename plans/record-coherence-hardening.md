# Record implementation coherence

Status: record overlap and selection use ordinary field types and shared row
tails. Optional shorthand is exactly Option, with no presence metadata. The
joint alias/HKT/imported-evidence and complexity audit remains open.

## Current Option equivalence

`record_coherence_treats_shorthand_and_explicit_option_as_identical` rejects
separate instances for `{ value?: Number }` and `{ value: Option[Number] }`.
`record_coherence_distinguishes_option_from_its_payload` accepts distinct Number
and Option[Number] instances and checks both call sites. The explicit optional
selection test checks both parameter spellings against the same instance.
Construction lifting is not part of dictionary matching; existing aliases
retain their ordinary field types. Historical checkpoints below explain the
row fixes but do not establish a second optional-presence semantics.

Validation: both new coherence regressions and the extended selection test pass.
The full solver suite passes 449 integration and 14 unit tests after adding
direct/mutual record-cycle checks as well. Full workspace tests/doctests passed
immediately before these test-only additions (446 solver integration tests),
including the latest open-spread production changes. No snapshots changed.
The current source/evidence map is `docs/record-row-acceptance.md`; no blanket
soundness or release-completion claim follows from these finite checks.

## Original overlap defect

Confirmed defect: the coherence-only HeadType::Record retained fields but
discarded row extensions. An implementation for `{ r | x: Number }` and one for
`{ x: Number, y: String }` were both accepted, although the latter record shape
satisfies both. The regression failed before the fix with successful solving.
Reordering identical closed fields was already rejected; that probe did not
establish a separate bug.

Head records now retain optional tail terms, using the implementation's existing
type-variable identity map. Their occurs checks visit the tail. Coherence row
unification matches common fields by name, unifies payloads, and constrains each
open tail to the other side's residual fields. Two independent open tails share
a fresh residual tail. Closed rows cannot absorb extra required fields, and a
shared tail cannot solve contradictory residual requirements. Record and error
row constructors remain distinct.

Reviewed Elm Type/Unify.hs unifyRecord/unifySharedFields/gatherFields as the row
reference. Alder's coherence pass is separate from value inference and must
agree with ordinary Option field matching. Field presence is intentionally
absent from both HeadType and the canonical/inference types. Expanded tails
still need the broader normalization and joint-constraint audit.

Six focused regressions pass: reordered duplicates, open/closed overlap,
independent open overlap, compatible shared tails, incompatible shared tails,
and disjoint required payload types. The complete solver run now passes all
337 integration tests. All 133 driver and 12 CLI tests pass, as do strict
workspace Clippy, formatting, and diff checks. The new source-aware colorless
diagnostic snapshot was reviewed: it includes the actual Alder source, labels
both overlapping declarations, and preserves the existing no-specialization
guidance. No pending snapshots were found. A Sampo changeset records the fix.

Remaining: tails already bound through another trait argument, alias/HKT
matching, imported implementation
sets, diagnostic review, and full hardening/release gates. Do not mark the
broader record or trait audit complete from these tests.

## Record instance selection

The already-expanded shared-tail overlap probe passes without another coherence
change. A new call-site probe exposed a separate defect: match_type returned
false for every Type::Record, so the accepted open-record implementation could
not be selected by a call. The positive regression failed with MissingInstance.

Instance matching checks record fields by name and ordinary type, and binds an
open tail to the remaining fields and actual tail.
Closed instances require no residual fields or tail. Repeated tail names must
match the same residual record. This follows the existing exact instance-type
matching model; contextual initializer coercion is not an instance-selection
subtyping rule.

The positive regression passes after the fix. Additional probes reject wrong
payloads, missing required fields, closed-instance extras, and inconsistent
shared residuals. A separate positive shared-residual probe prevents rejection
of everything from masquerading as success. A new imported record_instances
CLI module defines a ReadX implementation; actual execution checks records
both with and without extra fields. The expanded-tail test adds evidence, not
a claim of exhaustive normalization correctness.

Validation after record selection: all 341 solver integration tests, 133 driver
tests, and 12 CLI tests pass. The shared-residual positive and negative programs
are checked separately. Strict workspace Clippy, formatting, and diff checks
pass; no pending snapshots were found. Updated the existing record-row Sampo
changeset. The remaining acceptance gates stay open.

## Explicit optional record selection

Added a positive solver regression for an implementation whose field is
explicitly optional and a negative regression preventing required-field
dictionary code from accepting a possibly absent field. Both pass without
further compiler changes at that checkpoint. Under the final Option semantics,
the negative case rejects an Option field being read as Number, not a physically
absent property.

The imported CLI instance module now reads `value?: Option[Number]` through a
trait method. The migrated fixture checks omission as outer None and uses an
explicit Some(None) to distinguish inner absence. The fixture
uses Option.some because this larger trait project deliberately has another
Some constructor in scope; the initial ambiguity was a fixture naming issue,
not a new inference defect. The standalone CLI project suite passes.

The settled invariant is ordinary type equality: shorthand and explicit Option
are identical, and dictionary selection never inserts Some wrappers or defaults.

Checkpoint validation: all 343 solver integration tests, the standalone CLI
project suite, strict workspace Clippy, formatting, and diff checks pass.
No compiler implementation or runtime ABI changed in this checkpoint.
