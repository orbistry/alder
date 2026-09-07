# Optional arguments and contextual Option lifting

Status: implemented and accepted. Authoritative semantics are in
`plans/hardening-language-decisions.md`; final evidence is in
`docs/compiler-hardening-final-report.md`. The incremental notes below, including
initially failing probes and pending migration statements, are historical.

## Approved record-equivalence migration

The user's explicit confirmation resolves record presence: shorthand and explicit
Option field types are equivalent, and omission supplies None. Added
`explicit_option_record_fields_accept_omission_like_shorthand` before changing
production code. It currently fails at the shorthand-to-explicit function call
with Number versus Option[Number]. This is an expected unmet new requirement,
not an accepted regression or completed migration. Remove separate semantic
presence metadata and migrate construction, spread, access, assignment, patterns,
interfaces, and dictionaries together. Preserve explicit Some(None) nesting.

Canonicalization step: record shorthand now calls the same Option-construction
helper as parameter shorthand (renamed `optional_annotation`). The focused
canonical test failed before this change and now passes; source-local generic
alias tests check Option[Number] after substitution. The end-to-end solver probe
now gets past shorthand-to-explicit type equality and fails at omitted-field
construction with MissingField. Omission/default metadata, semantic presence
removal, spreads, and runtime consumers remain required; this is intentionally
an incomplete migration, not a green full-workspace checkpoint.

Fresh-construction step: contextual record literals with known closed shapes now fill
missing expected Option fields and record their names by construction region in
`omitted_record_fields`. Direct AST codegen emits those fields with None's null
representation. Defaults are not added by general record unification, so existing
mutable aliases are not implicitly converted. The original equivalence solver
probe now passes. A reviewed codegen snapshot distinguishes an omitted outer
None from explicit Some(None), and the new `record_options` CLI fixture passes
equality, nested Option, bare-payload lifting, and spreading a defaulted record
over an earlier Some value. The latter observes None, not the earlier value.
The fixture also reproduces and verifies defaults for a spread-only construction
from a closed record: the new record receives None without changing the source.

This covers contextually typed closed literals, not all omission sites. Open-row
spread construction, constructor records, late expected types, old presence
metadata/remnant branches, JSON and dictionary behavior, and migration of old
fixtures remain required. Do not treat the focused passes as full-suite success.

Runtime-consumer checkpoint: Fiber traversal options now read `concurrency` as
an ordinary Option field. The former own-property test forwarded explicit None
as a concurrency number and threw RangeError; both the kernel adapter test and
compiled `record_options` fixture reproduced that failure before the fix. The
adapter reads the field once per task run and uses None for the sequential
default. All 55 kernel tests, the explicit-async CLI fixture, and record-options
CLI fixture pass. Record assignment regressions now require actual Option
values (Some/None), with no initialization coercion applied to assignment.
The two record type snapshots were reviewed and changed from presence notation
to ordinary Option types; their generated pending files were removed.

Solver fixture migration: reviewed all 15 remaining failures individually.
Spreads overwrite an earlier value even when the later Option field is None;
they no longer provide payload fallbacks. Unannotated branch/loop construction
tests now explicitly build Some values rather than relying on presence joins.
The nested contextual spread fixture now uses `value?: Number` for its intended
single Option layer. Error-row union tests retain their original generic/error
and mutable-payload obligations through explicit Option matches and Result `?`;
negative tests likewise exercise those obligations rather than failing early on
mixing Option and Result propagation.

This exposed a real `?` defect: `resolve_try_type` unified an already-known
Result with a fresh open error row. A universal source row could become an
empty row extension, then fail generic checking with GenericSpecialization.
Reuse known Result parts, reserving fresh shape inference for unknown inputs.
Three generic-fallback cases reproduced the defect before the fix. All 437
solver integration tests now pass, including missing-error and mutable-payload
rejections. The CLI record-options fixture additionally executes heterogeneous
spread overwrites and both explicit fallback paths (including typed errors).
Semantic presence machinery removal and the other migration gates above remain
required; this is not full-workspace acceptance.

Read-lowering cleanup: removed `optional_accesses`, its inference collection,
the OptionalField pattern step, and the kernel `$optionalField` helper. Ordinary
access and enum/record patterns now project the stored Option directly. Removed
the superseded kernel test for absent-versus-present-None behavior; compiled
record-options coverage now verifies the approved equivalence, nested Option
patterns, and unit payloads instead. The removed helper/test remain recoverable
from Git history/worktree diff.

The cleanup exposed enum record payloads still canonicalizing shorthand into
presence flags. They now canonicalize to Required + Option like record types
(the transitional flag itself still needs removal). Constructor field checking
reuses contextual record inference, including defaults and minimum-Some lifting.
All 84 canonicalizer tests and 14 + 437 solver tests pass, as does the expanded
CLI fixture. A reviewed source-aware codegen snapshot shows explicit null
defaults, nested Some(None), and direct pattern reads without wrapping. Remaining
presence state is in internal row/overlay operations, AST/interfaces, dictionary
descriptors and runtime dictionary consumers; those are not declared complete.

Dictionary runtime cleanup: structural/derived Eq, derived Ord, derived Hash,
and derived Show now visit every declared field and delegate to its dictionary,
without own-property skipping or presence ordering. Kernel tests use actual
Option fields/dictionaries instead of physically missing payload keys. Compiled
record-options coverage checks omitted/explicit None equality, equal hashes,
Show of all declared fields, and nested Option distinctions.

An attempted compiled Ord derive exposed a remaining capability gap: builtin
Ord currently exists only for Number/String/BigInt, not Option (nor Unit).
Consequently derived Ord for `value?: Number` can no longer rely on the old
presence-ordering shortcut. Resolve/document conditional Option ordering as part
of the migration; the current CLI fixture checks Eq/Hash/Show, not Ord. JSON
presence descriptors and all other listed migration gates remain open.

Conditional ordering implementation: added audited builtin `Ord[()]` and
`Ord[Option[a]] where a: Ord` headers in both stdlib copies. Unit compares Equal;
Option uses None < Some and delegates two Some values to the payload dictionary
after representation-aware unboxing. Container evidence builds its Eq superclass
from the payload Ord superclass, sharing the lazy descriptor mechanism with Hash.
This does not provide Ord for error rows or arbitrary other containers. The
previous failed derived-Option CLI probe now compiles and executes. Solver
coverage rejects an Option function payload without Ord, and current canonicalizer
and solver checks pass (84 canonicalizer, 14 solver unit, 438 integration tests).

JSON migration: derived variant descriptors no longer contain an optional-field
list, and the AST backend no longer consults FieldPresence. Builtin Option JSON
dictionaries carry a `$option: true` codec marker. Missing record fields decoded
with that codec are materialized as null/None; other missing fields retain the
path-specific error. This is codec identity, not source field-presence metadata,
so generic fields instantiated with Option and transparent aliases work too.
Encoding visits every declared field through its dictionary. A compiled missing-
field decode failed before this change and passes after it. All 54 kernel tests
and the expanded CLI fixture pass, including generic/alias Option defaults,
required Number rejection, and nested Some(None)/Some(()) round trips. Existing
codegen snapshots still require deliberate refresh/review for descriptor removal
and the new Option codec marker; full-workspace acceptance is not claimed.

Snapshot/fixture migration checkpoint: the codegen run had 15 failures, all
resolved by deleting only obsolete `optional` descriptor properties. All 66
codegen tests/doctests pass. The JSON derive snapshot now uses solved emission
instead of undefined payload evidence; reviewed output includes the String
codec, Option codec marker, and lazy payload dictionaries.

The workspace run then stopped at CLI fixtures with superseded presence
expectations. Migrated traits, records, optional-arguments, and control-flow
fixtures to explicit Some(None), ordinary Option assignments, right-biased
spreads (None overwrites), explicit Result fallback matching, and JSON null
fields. Checked the final failing assertion against an actual CLI-built bundle
at `/tmp/alder-record-bundle.B9aGUE/main.mjs`. All 14 CLI tests now pass, including
standalone projects, artifact persistence, and six fresh bundle-order runs.
The full workspace run must still be repeated beyond that previously failing
CLI gate; internal presence removal and final packaging remain required.

Full-workspace checkpoint after fixture migration: the next failure was the
owned-interface row-tail roundtrip test expecting the obsolete optional flag.
It now asserts a normal builtin Option[String] field after serialization and
rehydration, as well as distinct input tails and preserved output relationships.
`INSTA_UPDATE=no cargo test --quiet` passes across the workspace and doctests
(two existing ignored doctests); formatting check and strict all-target/all-
feature Clippy also pass. Counts include 173 driver, 438 solver integration,
66 codegen, 54 kernel, and 14 CLI tests. This supersedes the prior red-workspace
checkpoint, not the remaining implementation requirements.

Removal boundary inspected: FieldPresence remains in AST/owned fields and in
solver record/overlay shapes. In particular, merge_record_operands still uses
Optional internally to probe a possibly absent field in an unresolved row.
Removing serialization flags alone would erase that internal distinction from
published constraints. Replace that overlay probe representation deliberately,
then remove the presence-only final validation pass and all AST/interface flags
together. Do not merely delete the final validation guard while unification
still permits missing Optional fields. Current green tests do not close this
approved cleanup requirement or packaging/commit/final-audit gates.

Overlay representation step: retain an explicit RecordOverlay whenever any
operand has an unresolved tail, not just when two or more operands do. The
existing deferred solver already resolves closed operands and exposes fields
whose final values do not depend on unknown tails. Closed merging is now plain
right-biased map extension; removed the single-open-row path that synthesized
Optional probe fields and joined speculative payload alternatives. This makes
the unknown overwrite relationship explicit in stored constraints instead of
encoding it as field optionality.

Solver (14 unit/438 integration), driver (173 existing), and CLI (14) tests pass
with that change. Added a driver regression verifying a single open spread has
an empty-field open input and a two-operand overlay, preserved through bincode,
rehydration, and arena copy; it passes separately (driver total now 174). The
remaining presence enum/flags and dead optional branches still need removal,
but their speculative probe producer is gone. No packaging or final-audit
completion is implied by this checkpoint.

Unification cleanup: missing declared fields now fail ordinary record
unification regardless of the transitional presence flag. Removed the deferred
`value_checks` collection and recursive presence-only validation pass; ordinary
type equations and generic-contract checks still enforce payload compatibility.
Existing solver (14 unit/438 integration), driver (174), and CLI (14) tests pass.
A new two-case regression also passes: an already-bound empty record cannot gain
an Option default at a call, and a record with a Number field cannot implicitly
become a record with an Option field. Fresh construction remains the only place
for record defaults/field lifting. Solver integration total is now 439.
AST/owned presence flags and remaining obsolete branch logic still await removal;
do not confuse removal of this redundant validation pass with completed cleanup.

Serialized-interface cleanup: removed `OwnedRecordField.optional` and bumped
the interface format from 5 to 6, without a legacy reader. Hydration currently
uses the transitional Required AST flag; stored types carry ordinary Option
fields only. Existing serialization/rehydration and independent-row tests pass.
Overlay field exposure and universal-result checking now select the rightmost
known field directly, stopping at unknown intervening tails. Removed their
obsolete optional-presence fallback lists and payload joins: a stored None
overwrites an earlier field just like any other value.

Validation after these changes: 14 CLI tests, 174 driver tests, 14 solver unit
tests, 439 solver integration tests, and associated doctests pass. Formatting,
strict workspace Clippy, and whitespace-aware diff checks pass. Added the driver
to the record-equivalence changeset. Canonical AST/internal solver presence
representation and its remaining matching/join/rendering branches still need
removal; open-row/late-context construction defaults and final packaging/audit
gates remain open. This is not a completed hardening goal.

Canonical/internal representation cleanup: removed FieldPresence and the
canonical field property. Solver record maps now store field types directly;
matching, normalization, row unification, variable traversal, interface copying,
hydration, and rendering no longer transport a redundant presence tuple.
Record joins retain ordinary unified fields without presence upgrades. Updated
canonical design documentation to describe the active solver rather than the
superseded presence-based subsumption proposal, and added alder-ast to the
record-equivalence changeset. No FieldPresence/property references remain in
the crates. All workspace unit/integration tests pass with no snapshot changes;
formatting and strict Clippy pass. The full `cargo test --quiet` process finished
successfully, including doctests (two existing ignored examples).
Open-row/late-context defaults and final packaging/audit
remain required; removing this representation does not close those gates.

Generic-field omission checkpoint: reproduced `choose({}, Some(42))` failing
with MissingField for `choose(record: { value: a }, fallback: a) a`. Fresh
construction previously supplied defaults only after the field was already
known to be Option. Omission now contributes an ordinary None constraint:
an unknown expected field type unifies with Option of a fresh payload. The
later argument resolves that payload; existing aliases are still not converted.
Positive tests cover a plain literal, closed spread, and reversed argument
order. Negative tests reject specializing a universally quantified return field
and returning a concrete Number from the defaulted Option contract. The compiled
record-options fixture checks that both literal forms actually contain None.
This resolves late *field-type* inference, not an entirely unknown expected
record shape or an unresolved spread tail. Those cases remain under review.
Validation: 14 CLI, 174 driver, 14 solver unit, and 441 solver integration tests
plus their doctests pass after correcting a test's record-return annotation to
use a type alias (the initial spelling did not parse). Strict workspace Clippy,
formatting, and whitespace-aware diff checks pass. No snapshot changes needed.

Late-shape construction checkpoint: reproduced `apply({}, read)` rejecting the
later callback's `{ value: Option[Number] }` parameter because the first literal
had already closed its expected type. Fresh contextual closed literals now
retain a pending initializer check and an open shape containing their explicit
fields. Once SCC constraints and Option lifts are solved, defaults are inserted
and the literal is checked/closed before generalization. No pending initializer
is published as a polymorphic open row. The same treatment applies when an
earlier branch has already established the shared open shape; otherwise the
second branch prematurely closed it (reproduced by the expanded test).

All 443 solver integration tests and 14 solver unit tests pass. Added negative
coverage for required fields, existing aliases, and records returned by an
already-generalized function. The compiled fixture exercises late callback
context for plain/closed-spread literals and branches. This does not yet default
an actual unresolved spread tail; only closed construction inputs use this
pending-initializer path. Full workspace validation follows this checkpoint.
The runtime fixture additionally executes both branch choices and observes
initializer-before-callback side effects exactly once (`trace == [1, 2]`).
Reviewed the one changed driver diagnostic snapshot: stored generic overlays
still reject String where Number is required, now labeling the offending string
initializer instead of the whole function. Accepted only that snapshot and
removed its generated pending copy. Strict Clippy and formatting pass.
The subsequent full `cargo test --quiet` run finished successfully, including
doctests (two existing ignored examples). No pending snapshots remain, and the
whitespace-aware diff check passes. Packaging and final audit are not yet run
against this checkpoint.

Open-input default checkpoint: reproduced a generic `copy(record)` returning
`{ value: Option[Number] }` via `{ ..record }` rejecting `copy({})`. Defaults now
form a first overlay operand, followed by the actual construction input. This
matches direct AST emission (null defaults before source fields/spreads), retains
rightmost overwrites, and does not require the input to supply the default.
Closed inputs still merge directly. No source alias is mutated or converted.

Added successful and rejected call cases plus a serialize/hydrate/arena-copy
driver test: absence succeeds, supplied Some succeeds, incompatible String
payloads fail, bound Number records cannot gain implicit field lifting, and
required Number fields cannot be invented. A fresh Number initializer remains
eligible for contextual Some lifting; corrected the initial negative fixture to
use an existing alias rather than rejecting this approved behavior.

The combined open-spread/later-callback reproduction also initially failed.
Pending initializer checks now include open inputs, with a separate contextual
shape until defaults and the checked overlay connect input to output. Closed
inputs still close before generalization; open inputs publish their actual
overlay relation, not an unconstrained synthetic row. All 446 solver integration
tests and 14 unit tests pass. The cross-module CLI fixture executes omission,
None, Some, nested Some(None), late callback context, and source non-mutation.
All 175 driver tests, 14 CLI tests, and associated doctests pass. Strict workspace
Clippy, formatting, and whitespace-aware diff checks pass; no pending snapshots
remain. The last full-workspace test run predates this open-input checkpoint.
These changes close the reproduced open-input omissions, not the final
construction/constraint-complexity audit or packaging gates.

## Universal-lifting integration review

The joint-constraint review reproduced a valid generic relay being inferred with
an Option-shaped input: `fn relay(value: a) a` called a polymorphic optional
consumer, then returned `value`. A Number caller failed with Option-versus-Number
before final universal validation. Merely rejecting the specialized contract
would still reject a valid program that can insert Some around its opaque input.

Depth solving now gives explicit universal representatives a zero assumed outer
Option depth, alongside its existing shape restrictions for sparse tuples.
This keeps direct-match preferences subordinate to the declared contract.
Instantiation may still choose Option as the payload of a universal; emitted
code wraps the whole value, preserving Some(None) rather than passing bare None.
Payload equalities are unchanged, and incompatible concrete payloads or attempts
to identify independent universals remain rejected by the ordinary contract
checks.

Four inference regressions cover ordinary generic functions, trait promises
without implementation annotations, default methods, lambdas/record contexts,
and incompatible payloads. A source-aware codegen snapshot contains exactly one
kernel Some wrapper. A serialized-interface regression checks independent
Number/String/Option instantiations after dropping the producer. Actual CLI
execution imports the relay and verifies that the optional consumer sees a
present value even when the original payload is None.

The first two additional fixtures incorrectly used semicolons and an out-of-scope
local annotation variable; those fixtures were corrected before evaluating their
inference behavior. All 430 solver integration tests and nine solver unit tests
pass, as do the new stored-interface test and standalone CLI execution. This fix
remains with the uncommitted joint solver/codegen implementation; it cannot form
an independent commit without its contextual lifting machinery. Full validation
subsequently passed for the working tree: all workspace tests/doctests, strict
all-target/all-feature Clippy, and formatting, including 168 driver tests and
64 codegen tests. Two existing doctests remain ignored; no snapshots are
pending. Final packaging and the remaining late-constraint/generalization audit
are still required.

## Connected-depth complexity and diagnostic precedence

The solver used a dense all-pairs matrix even for connected constraints whose
direct matches were all compatible. A bounded 256-variable chain regression
passed but took 0.69 seconds in the debug test before the change. Such a
component needed quadratic storage and cubic closure despite having a linear
number of relationships.

The connected solver now first checks whether all sites can have zero wrapping
slack. It traverses forward/reverse weighted edges, keeps node zero anchored,
and translates unanchored components to their smallest nonnegative depths.
When these equalities coexist, zero is the global minimum at every site and no
dense matrix is needed. Conflicts, negative anchored depths, or arithmetic
overflow fall back to the existing solver; they are not automatically rejected.
This fast path is linear in the component's nodes and edges. Component discovery
still uses ordered maps, and components needing positive slack retain the dense
solver's quadratic storage and cubic work. This is not a general linear-time
claim for contextual inference.

Tests include the bounded end-to-end chain, a 10,000-variable direct-solver
chain, and all 19,683 three-edge graphs over three nodes with unit/zero offsets,
compared against the original dense solver. The large test calls the sparse
routine directly so a regression cannot invoke an enormous dense allocation.
Existing exhaustive-assignment and ambiguity tests remain in place.

The stronger comparison reproduced a separate defect: component traversal could
return Ambiguous before discovering that a later component was Inconsistent.
Solving now retains ambiguity while examining the remaining components and gives
inconsistency precedence. A focused regression checks both component orders.
All eleven depth-solver tests pass after this correction. After the row-kind
follow-up below, the full workspace test/doctest run also passes, including 431
solver integration tests, 13 solver unit tests, and actual CLI fixtures. Strict
Clippy, formatting, and diff checks pass; no snapshots changed or remain pending.
Two existing doctests remain ignored. These changes remain part of the
uncommitted joint solver checkpoint; final packaging is not yet verified.

A related kind-integration regression then reproduced an inferred error-row
parameter being rejected when passed to an optional consumer. Unlike explicit
universals and sparse tuples, inferred row-kind variables had no zero-depth
restriction. Their later kind check rejected the selected Option structure even
though Some insertion would preserve the row. Depth solving now constrains both
record-row and error-row variables to have no assumed outer Option structure.
The error-row regression passes with the Result construction before or after
the optional call, and an imported relay executes through the actual CLI. The
fixture initially used the reserved name `error`, then a bare tag at the call
site; those were corrected to `failure` and extraction from `Err`, preserving
the existing tag-placement rules. This is not permission to construct arbitrary
tag values or use ordinary types as Result errors.

### Failure-site provenance

Two rendered regressions reproduced `solve_option_lifts` assigning failures to
`constraints[0]`: both highlighted a valid `consume(42)` before an unrelated
ambiguous call group or a nested Option mismatch. The latter also rendered the
unrelated argument's types instead of the incompatible nested Option types.

The depth solver now returns a failure kind and an original edge index. Fixed
impossible edges retain their own index; connected failures select their
earliest participating edge. Source edges precede synthetic kind/shape edges,
so that index maps to a real source constraint. Inference uses its region and
types. This identifies the failing component, not a minimal unsatisfiable subset
of its constraints.

Reviewed colorless snapshots now highlight `first` in the conflicting call group
and `nested` in the mismatched call, respectively. Unit coverage checks fixed
edges, unrelated valid components, and synthetic constraints added after source
edges. All twelve depth-solver tests and the selected driver regressions pass.
Full workspace tests/doctests subsequently pass, including 170 driver tests,
431 solver integration tests, and 14 solver unit tests. Strict Clippy,
formatting, and diff checks pass; no snapshots remain pending. Two existing
doctests remain ignored. The changes remain in the joint solver checkpoint,
with final packaging and broader integration review still open.

The parser/source-AST/canonicalization shorthand and formatter coverage are
committed independently as `470e3ef`, including grammar, diagnostics, reviewed
snapshots, and a changeset. Call omission and contextual lifting remain in the
larger solver/codegen worktree. The latest complete workspace validation passes
1,310 parser tests, 82 canonicalizer tests, 166 driver tests, 426 solver tests,
and 15 formatter tests, plus doctests and strict Clippy. Fresh package verification
also passes for this source state; see `docs/release-packaging-hardening.md`.

## Invariants

- `name?: T` is declaration shorthand for `name: Option[T]`. Source ASTs retain
  the marker; canonical types use the builtin Option, not a new function slot
  kind or a user-shadowable synthetic type name.
- Trailing Option parameters may be omitted, supplying None. Function values,
  aliases, stored interfaces, and inferred calls must retain that behavior using
  their ordinary function types. Only the trailing consecutive Option suffix is
  omittable; earlier Option parameters followed by non-Option parameters remain
  required. The user confirmed this clarification during implementation.
- Supplied call arguments and record-field initializers prefer a direct match;
  otherwise add the minimum number of Some wrappers necessary. In particular,
  42 can initialize Option[Option[Number]]. None directly matches outer absence;
  Some(None) denotes present inner absence. No recursive implicit unwrapping.
- This is contextual elaboration, not a change to general unification, equality,
  trait selection, assignment, or final-return checking. Evaluate source values
  once, preserving order, and use the centralized Option runtime representation.
- Generic inference must not silently select a wrapping depth based on visit
  order. Resolve contextual constraints before choosing a lowering; report any
  remaining genuine ambiguity rather than generating an arbitrary representation.

## Current evidence

The two new parser snapshot tests rejected `?` with Params::End before changes.
The parser now retains a source-only optional flag for named annotated parameters.
It rejects a missing annotation and does not extend optional markers to patterns.
Canonicalization adds exactly one builtin Option layer for named functions,
components, lambdas, and trait signatures. The shorthand inference regression
passes with explicit Some/None arguments and ordinary function-type annotations,
including a nested Option and a lambda. That initial shorthand checkpoint did
not verify omitted or lifted arguments; subsequent implementation evidence is
recorded below. Parser snapshots were reviewed for
source text, marker, payload type, and regions.

Validation for this checkpoint: all 1,310 parser tests, 80 canonicalizer tests,
142 driver tests, 376 solver integration tests and two solver unit tests pass.
The 14 pre-existing formatter tests pass; the separately added optional-parameter
formatter regression also passes, checking semantic structure and idempotence.
The selected crate test commands completed including their doctests (two existing
ignored doctests across parser and driver). Workspace clippy with all targets,
all features, and warnings denied also passes after the formatter regression.
Formatting and diff checks pass. These checks do not replace the goal's final
workspace, runtime/CLI, or release-packaging gates.

## Remaining implementation and acceptance

### Omission checkpoint

The new `trailing_option_arguments_can_be_omitted` regression initially failed
with exact-arity fn(Number) versus fn(Number, Option[Number], Option[String]).
Known callable types now permit omission when every missing parameter is an
ordinary builtin Option after pruning. Solver metadata records the count by
call UseId; direct AST emission appends that many explicit null/None values
after source arguments, without confusing dictionary prefixes or pipe inputs.
The source-aware codegen snapshot was reviewed: both direct and piped calls
contain explicit null, not JavaScript's implicit undefined.

The traits CLI fixture now executes direct and imported calls, typed function
values, lambdas, nested Option absence, and pipes. It verifies omitted and
explicit None agree while Some(None) remains distinct. Its first run exposed a
fixture name ambiguity with the existing NamedSome enum in main; qualifying
option.some fixed the fixture, and the real standalone CLI test passed. Negative
arity fixtures initially placed two declarations on one line; they were changed
to indoc multiline source so they exercise type checking rather than parsing.
After those corrections, all 378 solver integration tests, two solver unit
tests, and 52 codegen tests pass, including doctests. Workspace clippy with all
targets/features and warnings denied passes. These results are for omission,
not evidence that recursive wrapping or the full hardening goal is complete.

This is not recursive lifting. In particular, do not implement lifting by
eagerly binding each unknown argument to the expected Option type and adding
wrappers only after that fails. Consider:

```alder
fn single(value?: Number) {}
fn double(value?: Option[Number]) {}
fn consume(value) {
    single(value)
    double(value)
}
```

The two calls share one inferred argument type. Reversing their order must not
choose a different type or change acceptance. An eager direct-match-first pass
can bind value to Option[Option[Number]] at double, making single fail, whereas
the opposite order admits Option[Number] with one wrapper at double. Pending
lifting constraints need joint resolution before generalization, including
their free variables and mutation restrictions; merely delaying and then
processing the constraints in source order does not solve this issue. Keep the
new call rule scoped to the declared goal rather than leaking Option subtyping
into general function unification or trait selection.

### Recursive call-lifting checkpoint

The concrete-value and shared-expected-payload regressions first failed with
Number versus Option mismatches. The shared-input fixture initially used the
reserved name `use`; it was renamed `consume` before evaluating type inference.
All three new inference regressions now pass, including reversed source orders.

Known Option parameter checks now collect contextual constraints rather than
eagerly binding each source value. At the SCC boundary, split each outer Option
spine into its fixed depth and payload. Payloads use ordinary invariant type
equations; unresolved spine variables get nonnegative depth variables. Each call
contributes a difference constraint; its slack is the emitted Some count.
`option_levels.rs` computes tight bounds and requests the per-site minimum slack
simultaneously. If minima conflict, diagnose ambiguity, not a source-order or
arbitrary total-wrapper-count tie-break. Independent depth components are solved
separately. No trait-based conversion, payload mutation, or JS string generation
is introduced. Sparse tuple operands have a zero-depth restriction so unresolved
tuple metadata is not mistaken for an unconstrained Option spine.

Codegen uses solved (call UseId, source argument index) wrapping depths, with pipe
inputs at index zero. The reviewed snapshot shows two nested $optionSome calls
around one materialized piped value. The actual CLI fixture passes direct,
imported, and piped nested calls, explicit outer/inner absence, and a side-effect
counter proving single source-expression evaluation. All 382 solver integration
tests passed at this checkpoint, as did the initial five depth-graph unit tests
and two existing solver unit tests. Further validation is recorded as it finishes.

The bounded exhaustive depth test also passes: all two-edge problems on two free
nodes with offsets -1, 0, and 1 agree with enumerated feasible assignments and
per-site minima. Final selected-crate validation passes 382 solver integration,
eight solver unit, 53 codegen, and 143 driver tests, including the reviewed
source-aware colorless ambiguity snapshot and doctests (one existing driver
doctest ignored). Workspace clippy passes with all targets/features and warnings
denied. These finite graph and integration tests do not establish the remaining
row, inference-boundary, or packaging requirements.
The expanded CLI run also passes shared-input calls in both orders and a generic
wrapper instantiated with Number and Option[Number]. The first generic absence
assertion lacked a determining Eq payload type; annotating its None fixture as
Option[Number] fixed that test without weakening trait inference.

### Record initializer checkpoint

`record_initializers_recursively_lift_option_values` initially rejected Number
against Option[Option[Number]]. Contextually checked fresh, spread-free record
fields now contribute to the same joint lifting constraints as call arguments.
The resolved field-name region carries a Some depth into direct record AST
emission. It is applied before any materialization required by later property
setup, preserving field order and once-only source evaluation. Existing record
values are not converted: a negative alias test keeps ordinary mutable record
invariance.

Direct record return types now supply field context to function tails and early
returns. The return regression first needed a named record alias because a bare
brace return type was parsed as a body; the corrected fixture then reproduced a
Number/Option mismatch before the context fix. This does not add implicit
wrapping of whole return values. Branches, matches, nested constructor contexts,
record spreads/overlays, and records inside an Option call still need auditing.

The field codegen snapshot was reviewed for exactly two centralized Some calls.
The CLI fixture checks bare and already-wrapped field inputs, outer versus inner
absence, and ordered side effects. Solver/codegen validation passes 385 solver
integration tests, eight solver unit tests, and 54 codegen tests including their
doctests. The ambiguity wording now refers to values rather than only calls,
since fields can participate in the same constraints.
Final checkpoint checks pass: all 143 driver tests and its doctests (one existing
ignored doctest), the expanded real CLI fixture including both return paths,
workspace all-target/all-feature clippy with warnings denied, formatting, and
diff checks. No pending snapshot files remain.

Resolved by explicit user confirmation: `{ value?: Number }` and
`{ value: Option[Number] }` must be equivalent, both accepting omission and
treating explicit None as outer absence. Current record field-presence metadata
still distinguishes them and must be removed. The preceding payload-lifting
checkpoint does not implement this newer decision. See
`plans/hardening-language-decisions.md` for the approved contract.

### Context and bare-pipe boundary checkpoint

`option_wrapping_preserves_context_for_fresh_record_payloads` reproduced a nested
Number/Option mismatch in `take({ value: 42 })` for an Option-wrapped record whose
field is Option[Number]. `infer_lift_input` now supplies the known underlying
record/array context to fresh literals before solving their outer Option layers.
This also handles fresh records in arrays and nested Option-valued record fields.
An alias regression confirms that this does not convert existing mutable record
payloads. The expanded CLI fixture executes all three positive forms.

`bare_pipe_destinations_use_optional_call_rules` separately reproduced that
`42 |> nested` bypassed lifting and omission although `42 |> nested()` used the
checked call path. Bare destinations now call `infer_call` and the common direct
AST call emitter with the pipe UseId; explicit destinations retain their own
call UseId. Await/Try destination wrappers preserve that identity recursively.
The CLI fixture passes bare lifting, omission, and a trait-method `show` call.
The reviewed placeholder snapshot adds a callee temporary before invocation;
the lambda and its argument positions/evaluation order remain unchanged.
Validation passes 388 solver integration tests, eight solver unit tests, 54
codegen tests, and 143 driver tests. Both actual CLI integration commands pass:
the standalone projects and the separate explicit-async laziness/capture/nested
task fixture (the latter is not covered by the standalone command). Workspace
all-target/all-feature clippy with warnings denied, formatting, and diff checks
pass; the intentional placeholder snapshot is reviewed and no pending snapshots
remain. These checks are not the goal's final full-workspace/packaging audit.

Still incomplete: contextual fresh records on the *left* of pipes are inferred
before their destination type is known; this checkpoint fixes bare call rules,
not that context path. Also audit explicit Some constructors around fresh record
payloads, blocks/branches/matches, spreads/overlays, and constructor-record fields.
Do not claim all contextual forms work from these direct-literal tests.

### Component complexity and lambda-return checkpoint

Code inspection found that component discovery partitioned variables but then
filtered the entire edge list for every component. For N independent Option
arguments this performed N-by-N candidate edge scans despite each depth problem
being tiny. Discovery now indexes incident edges, assigns each edge to its
component once, and never joins independent components through the fixed zero
anchor. The existing exhaustive small-graph test still passes, as does a new
10,000-independent-depth regression. This removes the repeated global scans; it
does not claim linear solving for a single large connected component (its bound
closure remains cubic).

An explicit lambda return annotation was another missing context path:
`() Payload -> { { value: 42 } }` rejected the initializer when Payload's field
was Option[Option[Number]]. This was reproduced as a type mismatch before the
fix. Lambda inference now forwards known record result context to fresh field
initializers while retaining its own return, loop, annotation, and reachability
scopes. Whole-value Option return wrapping remains disallowed. The CLI fixture
now exercises this annotated lambda form too.
Selected-crate validation passes 389 solver integration tests, nine solver unit
tests, 54 codegen tests, and 143 driver tests, including doctests (one existing
driver doctest ignored). The expanded standalone CLI fixture and workspace
all-target/all-feature clippy with warnings denied pass. Formatting and diff
checks pass, with no pending snapshots. The full `cargo test -- --quiet` command
subsequently completed successfully, including all workspace unit/integration
tests and doctests (three existing ignored doctests: driver, parser, runtime).
This is a current checkpoint, not the final goal audit: contextual forms, record
presence, structural error capabilities, public Fiber APIs, packaging, coherent
commits, and other recorded hardening requirements still need completion.

### Remaining acceptance checklist

Spread-context reproduction: `spread_record_initializers_preserve_written_field_context`
fails with Number versus Option[Number] for `{ ..base, value: 42 }`, where base
only supplies a String label. The same test also covers the reversed source
order and optional function arguments. `infer_checked_expr` currently disables
its contextual record path whenever any spread is present, and the fallback
infers all explicit field payloads without context.

Preserve overlay semantics when fixing this: newly written fields can receive
initializer coercions, but inherited nested values must not be rewritten through
aliases. The negative `spread_record_context_does_not_convert_inherited_payloads`
passes. `contextual_spreads_discard_overwritten_field_expectations` checks a Bool
initializer overwritten by a required Option[Number] spread; the earlier value
must not be forced to satisfy the final field type. Optional later fields can
leave earlier values visible, and unresolved row tails must retain their ordered
relationships. Do not fix just the final-field spelling, reverse inference order
without checking constraint/reachability effects, or constrain all intermediate
operands to the final record annotation. The new positive regression remains
unignored; the implementation follow-up below now corrects this path.

Spread implementation: contextual written fields get independent target type
variables and ordinary deferred Option-lift constraints. The existing ordered
merge relates surviving targets to the final record annotation; overwritten
targets are not constrained by that annotation. Spread expressions themselves
are inferred normally, with no inherited-payload coercions. Fresh nested records,
arrays, and canonical Some construction preserve context through initially
unknown target variables; existing aliases do not enter those fresh paths.
All 413 solver tests present at the first post-fix run pass. Additional targeted
tests pass for nested fresh payloads, explicit Some, arrays, and optional later
fields retaining a wrapped fallback. CLI side-effect and nested-value assertions
and broad workspace validation are running. This does not establish every
unresolved/generic overlay or lifting/generalization interaction.

The expanded CLI and full workspace unit/integration suite subsequently pass
(414 solver integration tests), as does strict workspace Clippy. Runtime tests
include absent later properties, a present Some payload, a present None payload,
and exactly-once overwritten-field/spread effects. Formatting/diff checks pass,
with no snapshot changes. This checkpoint does not close the remaining generic,
late-constraint, record-presence, packaging, or final-goal acceptance items.

Explicit-constructor follow-up: permanent reproductions rejected both
`let value: Option[Payload] = Some({ value: 42 })` and
`take(Some({ value: 42 }))` when Payload's field was Option[Number]. Checked
Some calls now constrain the constructor result before checking its argument,
so ordinary field initialization receives the payload context. Only canonical
builtin Some/option.some receive this contextual treatment, not functions that
happen to share a spelling. Mutable payload aliases still require direct matching.
The positive reproduction passes; 405 solver tests passed with the separate
known-failing branch regression explicitly excluded. CLI execution passes for
both constructor spellings and annotations. An additional alias-negative test
passes alongside the positive constructor test; lint is running. This is not a
green full-suite claim.

The separate `optional_record_arguments_preserve_branch_context` reproduction
initially failed: `take(if flag { { value: 42 } } else { { value: 7 } })` lost the
expected field type before the branch results are joined. Fix contextual flow
through branch/block tails without globally allowing implicit Option returns,
rewriting existing record aliases, or forcing all branches to the same outer
Option depth before direct-match-first lifting is resolved. Match and spread
forms remain part of the same acceptance audit.

Branch/block follow-up now distinguishes exact contextual checking from
inferring an input whose outer Option wrapping is still pending. Both contexts
flow through a shared if/match checker and block-tail machinery; only the latter
permits the enclosing call/field to add wrappers. Branch result joins remain
ordinary type joins. Explicit Some layers remain explicit; existing aliases are
not field-wise converted. A refactor initially bypassed exact checking of the
aggregate result for a missing else, caught by the existing reachable-unit
fallthrough regression; the aggregate check was restored. A new match fixture
also needed parentheses to distinguish its record literal from a block.
After those corrections, all 410 solver tests pass, including no implicit
function-return wrapping and alias rejection. CLI tests now execute both branch
choices, match arms, explicit Some arms, field initialization, and exactly-once
branch effects. Broad workspace lint/test validation is running. Spreads/overlays,
late constraints, and the remaining full-goal gates are still under audit.

Reference check: local Elm `Type/Constrain/Expression.hs` `constrainIf` and
`constrainCase` propagate annotation expectations to branch expressions and
otherwise relate the joined branch type to the outer expectation. Alder retains
that distinction, additionally separating pending Option input lifting from
exact checking; Elm has no corresponding contextual wrapping semantics.

Completed validation for this checkpoint: all workspace unit/integration tests
and strict all-target/all-feature Clippy pass. The CLI assertions exercise both
branch choices and exactly-once effects, match results, explicit Some layers,
and record-field context. No snapshot changes or pending snapshots. Doctests
and packaging remain final-goal gates, not established by this run.

The pipe-left gap is now a failing permanent solver reproduction:
`pipe_inputs_preserve_context_for_fresh_record_payloads` checks bare destinations,
explicit calls, and arrays of fresh records. The first form fails with Number
versus Option[Number] in the field initializer, while the existing direct-call
control passes. `infer_binop` infers the left expression before calling
`infer_pipe_destination`; `CallInput.leading` then carries only a type/region,
so `infer_lift_input` cannot contextualize that initializer. Preserve the source
expression through the checked call path, without inferring it twice. The
refactor must retain left-before-destination runtime evaluation and reachability
for returns/divergence in the left expression, destination, and later arguments.
Do not repair this by converting existing mutable record aliases.

The checked-call path now carries the leading expression rather than an already
inferred type. It learns the destination's parameter context, then checks the
leading initializer once through the same path as ordinary arguments. Destination
inference is guarded by input fallthrough, while the input retains its original
reachability. The original reproduction and all 400 tests present at the first
post-fix solver run pass. Additional regressions check alias rejection, invalid
reachable returns, and unreachable loop breaks; CLI checks field wrapping and
input-before-callee side effects. All 403 solver tests at that checkpoint and
the expanded CLI fixture pass. The first dead-return test incorrectly assumed
unreachable return statements are exempt from type checking; existing Alder
behavior checks them. The corrected reachability regression verifies that dead
break values do not contribute to a loop's inferred result. Await/Try-wrapped
input context also passes, followed by the full workspace unit/integration run
(404 solver integration tests). No snapshot changes were needed. Strict Clippy
and formatting pass. Explicit Some payloads, branches/spreads, late constraints,
and the remaining checklist below are separate from this pipe-input fix.

- Call inference currently pads the known omittable suffix and constructs an
  exact-arity Ty::Fn in `infer_call`. Replace the contextual checking step with
  sound lifting constraints for every contextual form; the basic known-Option
  call path above is implemented, not the entire feature.
  Cover callables whose type is initially unknown and aliases of builtin Option.
- Record initialization must use the same direct-match-first lifting policy;
  audit the existing optional-field payload/whole-value distinction rather than
  assuming current record behavior already implements the new agreement.
- Audit late payload equalities, projections, generic rigidity, sparse tuple
  shapes, record overlays, array invariance, recursive boundaries, and mutation
  restrictions against the new joint solver. Its payload-equation rebuild loop
  and component construction need adversarial complexity/termination coverage.
  The graph solver does not itself prove these compiler integration invariants.
- Audit the resolved call wrapping/omission metadata across call placeholders,
  dictionaries, extern adapters, and other existing solved metadata. Extend the
  same representation-aware direct AST approach to record initialization.
- Determine the omittable suffix after type resolution, not just by the presence
  of source `?`: explicitly written Option and transparent aliases have identical
  semantics. Earlier Option parameters remain required when followed by a
  non-Option parameter. Audit generic instantiations and higher-order arity.
- Add source-aware diagnostics, formatter semantic/idempotence coverage, trait
  contracts/defaults/impls, externs, imported owned interfaces, recursive/SCC
  inference, and actual CLI runtime tests with nested absence and side effects.
- Test direct match versus wrapping with polymorphic None/Some and generic
  callbacks, wrong payloads, too many arguments, missing required arguments,
  explicit inner None, Option[Unit], and nested record initializers. Check that
  no coercion leaks into general expression or trait matching.
- Implement the four public Fiber traversal adapters once these semantics work;
  the kernel implementation alone does not satisfy that API requirement.
- Run full workspace validation, review snapshots, packaging, and coherent
  commits as required by the parent goal. No release or merge is authorized.
