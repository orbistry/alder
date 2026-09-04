# Record-row hardening design

Status: implementation in progress. The core tail representation and inference
paths are implemented in the worktree; optional compatibility and the complete
acceptance audit are not finished.

## Current failures

The pre-hardening solver represented `Ty::Record` as a field map plus an open Boolean.
Accessing an unknown field on a type variable binds it to a singleton record;
subsequent access does not extend the open record. Annotation conversion drops
the identity of `RowExtension::Open`, while publication invents the name `r`.
Spread copies only currently known fields and always returns a closed record.
Record unification compares known fields but never reconciles residual rows.

Consequently the documented `rename` example cannot preserve its caller's
extra fields. More seriously, a function annotated to preserve an arbitrary
tail can discard it without a declaration error. Optional and required fields
are also treated as interchangeable in either direction, allowing an omitted
field to flow into a required-field reader.

## Current implementation checkpoint

`Ty::Record` now stores an optional tail type, with `None` denoting a closed
record. Record-row variables use `VariableKind::RecordRow`; bound fragments
are wrapped in `Ty::RecordRow` so they cannot be confused with ordinary record
values or error rows. Empty fragment wrappers normalize to their tail variable,
preserving universal contracts rather than spuriously specializing a row to an
empty extension node.

The active paths reconcile residual fields, extend open tails during access,
and traverse tails during pruning, occurs/free-variable checks, instantiation,
normalization, and publication. Single-spread records retain their source tail.
Annotation and owned-interface conversion retain tail variable names instead of
inventing one shared `r`. The original CLI records probe now executes successfully.

Value compatibility now records deferred directional presence checks at calls,
annotated bindings, assignments, and returns. These reject optional-to-required
flow after inference resolves variables. Function parameters are checked in
the opposite direction; container arguments and nested mutable payloads are
checked in both directions to prevent alias-based weakening. Annotated let
bindings retain their declared shape. Fresh array and non-spread record literals
are checked recursively against their expected type, including call arguments,
allowing construction without weakening an already-shared container. Bare pipe
destinations and lambda returns also participate in compatibility checking.
If/match record results retain optional presence from either branch. Loop
frames likewise accumulate joined reachable break types and return the final
joined type, not the type fixed by the first break. Optional presence from any
reachable exit is retained; nested loops keep independent result frames.

Solved output records optional read sites by source region. Codegen emits an
Oxc call to `$optionalField(record, name)` only at those sites; required reads
remain ordinary member access. The helper checks own-property presence and
wraps a present payload with the centralized Option constructor, distinguishing
an absent field from present null/None or unit payloads. The record expression
and property value are each evaluated once. CLI and kernel tests cover these
boundaries. Assignment to the final field expects its declared raw payload T,
not the Option[T] produced by a read. Intermediate optional members still have
the read type, preventing assignment through a possibly absent parent. Solver
regressions cover both rejected Option-as-payload writes and absent-parent
traversal; the CLI fixture exercises writes of nested Option payloads and writes
through a required parent. Compound assignments additionally require the read
type to match the stored payload; they cannot use an optional field as if it
were present.

Record patterns now use the same field-read inference as access expressions,
rather than unifying an invented all-required record with the scrutinee. This
preserves Option-valued optional bindings and rejects extracting a required
payload from an absent field. Constructor record patterns retain the declared
presence of each instantiated payload field. Solved optional access regions
also identify pattern field names; both pattern tests and bindings lower those
steps through `$optionalField`. Solver and CLI regressions cover destructuring,
match bindings, pinned field comparisons, and absent versus present nested
Option payloads, including enum records. Effectful pattern evaluation and
broader pattern compatibility remain part of the related codegen audit.

Still open: full optional compatibility coverage, multiple open spreads (currently unified,
which may overconstrain valid combinations), shadowed labels/lacks constraints,
cross-kind annotation use, trait matching of record shapes, and cross-module
execution/serialization tests. These are acceptance work, not waived limitations.

Cross-module checkpoint: executable `records` tests now cover imported row
updates, independent instantiations, two distinct tails, inferred field
requirements, and optional results. Driver tests exercise owned-interface
binary serialization and rehydration after source-arena destruction, retaining
tail identity and optional presence, plus negative imported compatibility cases.
This does not complete the interface audit: record type aliases were found to
remain nominal named types rather than expanding in active canonicalization.
The alias reproduction and required follow-up are recorded in the hardening plan.

## Required representation and operations

- Keep ordinary record values distinct from row fragments. Give record tails
  actual variable identities with a record-row kind, separate from ordinary
  types and error rows. Reuse the existing inference-variable machinery only
  where its kind checks, generalization, and contract checks remain valid.
- Flatten substituted row fragments when examining a record. Unify common
  fields, then reconcile residual fields through the opposite tails. For two
  distinct open tails, introduce a shared fresh tail. Shared-tail equations
  cannot silently discard residual fields. Closed tails reject incompatible
  required fields, and cyclic tail equations fail an occurs check.
- Field access adds a required field through an open tail, while preserving
  known optional-field reads as `Option[T]`. Repeated accesses share the same
  tail constraints independent of access order.
- Preserve tail variables through occurs/free-variable traversal, substitution,
  scheme instantiation, generic contract checking, trait evidence matching,
  and canonical/owned interface conversion. Independently instantiated rows
  must not become linked merely because publication spells both tails `r`.
- Spread must constrain an unknown operand to a record and retain its tail.
  Field replacement, duplicate-label handling, optional fields, and mutation
  require explicit checks; copying known fields into a closed map is not row
  polymorphism.

Elm's `Type/Unify.hs` `unifyRecord` and `gatherFields` provide the reference
for residual-row reconciliation and flattening. Alder's optional fields are
an additional constraint; Elm's algorithm alone does not establish their
soundness.

## Optional fields and compatibility

Do not retain symmetric optional/required compatibility as type equality.
Construction may omit an optional field, as documented, but a value whose
field can be absent cannot satisfy a required-field reader. Audit call,
annotation, return, pattern, and assignment checks to distinguish value
compatibility from equality without permitting mutable aliases to invalidate
an existing required-field promise. Do not silently change JavaScript-style
aliasing or remove support for omitted optional fields.

## Acceptance evidence still required

The initial regressions cover both access orders, the documented row-preserving
spread example, declaration-level loss of a promised row, and optional-to-required
flow. All four now pass. Independent instantiation, shared-tail incompatibility,
optional local/global construction, fresh array construction, and rejection of
shared-array field weakening also have passing tests. Add cyclic
rows, field replacement, aliases, patterns, assignment, and cross-module owned
interface tests. Execute accepted examples through the CLI; negative programs
must fail during compilation. This design is not a completed acceptance audit.
