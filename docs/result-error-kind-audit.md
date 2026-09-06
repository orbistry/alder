# Result error-kind contract audit

Status: annotation, alias, and enum declaration checks implemented;
broader error-kind audit remains open.

## Direct group annotation normalization checkpoint

Derived-field normalization now uses the same `Infer::from_ast` conversion as
ordinary annotations instead of a second incomplete converter. Removing nominal
group dictionaries exposed the old path in an existing regression for an enum
wrapping `Result[Number, Failure]`. That regression and all 399 solver integration
tests now pass. Generic nominal wrappers still own their own implementations;
only their error-group payloads are structural.

The stored-Hash regression serializes a generic Result/error-row fingerprint
and equality function, drops the producer, and reloads its interface. Independent
Number/String consumers and a reordered named group pass; a function payload
is rejected with a reviewed colorless Hash diagnostic at the consumer call.
All 158 driver tests pass after adding this boundary check. The CLI generic
wrapper case also executes Hash/Eq, Show, and JSON with direct and Result-wrapped
named groups and custom payload dictionaries.

Subsequent migration: nominal error-group implementation synthesis and its
declaration-order Ord snapshot have been removed. Groups now use conditional
structural Eq/Show/Hash/Json; Hash includes the actual tag and payloads, not
group identity or the row's other possible tags. The old failure described
below is historical, not an outstanding snapshot awaiting acceptance. The
replacement codegen snapshots and standalone CLI fixtures pass, including
custom payload Hash and equality, reordered groups, widened rows, and recursive
containers. Open-row evidence and whole-goal release verification remain open.

The new full-solver reproduction rejected `fn relay(value: First) Second { value }`
for two groups with identical tags/payloads in different orders. Error groups
were expanded only in Result's error slot; ordinary `from_ast` conversion left
their names nominal. Direct references now use the same guarded structural
expansion, including aliases and references nested in arrays/record fields.
Incompatible payloads still fail instead of being coerced.

The CLI traits fixture extracts an error from Result, passes it through a
function with direct group annotations, and exercises structural Show/Json with
a custom payload codec. Exact output and the direct error JSON round trip pass.
A serialized producer interface preserves direct-group relay/renderer contracts
for an independently named consumer group; the wrong-payload consumer is rejected
at its call argument, with a reviewed colorless diagnostic snapshot.

Before that migration, normalization exposed a nominal derive inconsistency.
The workspace unit/integration run stopped at the codegen snapshot
`error_group_ord_preserves_declaration_order`: its Ord superclass changed to
structural Eq rather than the nominal group's generated Eq dictionary.
That proposed snapshot was not accepted because it retained nominal ordering
inconsistent with structural group identity. The test and per-group synthesis
have now been removed, and consumers use the structural capabilities above.

Focused validation after correcting test-fixture syntax: all 396 solver
integration tests and all 157 driver unit tests pass, as do the standalone CLI
fixtures, strict workspace Clippy, formatting, and diff checks. Those counts
describe the earlier normalization checkpoint; doctests and packaging were not
rerun for it.

## Structural Show checkpoint

A full-solver reproduction rejected Show for equivalent named groups and their
literal closed row unless a nominal derive happened to supply it. Closed error
rows now resolve Show structurally before instance lookup, recursively requiring
Show for every payload. Missing payload capabilities remain errors, not a reason
to fall back to an unrelated nominal group dictionary. Evidence retains sorted
tag names and per-payload dictionaries; direct Oxc AST emission builds the
descriptor consumed by the existing kernel renderer. No runtime or generated
source-string ABI was added.

The regression covers two reordered group declarations without derives and an
equivalent literal row. The negative checks a function payload without Show.
CLI traits coverage verifies custom payload Show behavior, unit tags, and nested
Option payload representation. The reviewed emitted-code snapshot contains the
actual Alder source, canonical tag order, and explicit payload dictionaries.
This is one selected capability, not blanket derivation: automatic Ord was not
added. Nominal group derives, Json/Hash behavior, and direct annotations were
addressed in subsequent checkpoints above. Open-row capability composition and
the broader recursive/stored-interface audit remain unfinished.

Validation: `cargo test --lib --tests -- --quiet` completes successfully across
the workspace (392 inference tests, 148 driver tests, 55 codegen tests, 52 kernel
tests, and all 12 CLI tests among the passing suites). Workspace Clippy with all
targets/features and denied warnings passes. Both new snapshots were reviewed:
the negative diagnostic names the missing function-payload Show capability and
shows its obligation chain. Formatting/diff checks pass and no pending snapshots
remain. Doctests and packaging were not rerun for this checkpoint. While updating
the codegen documentation, corrected its stale Option pseudo-implementation to
use the existing WeakSet box identity rather than a forgeable tag comparison;
the runtime implementation was not changed.

### Generic and stored Show evidence

A new dependency-interface test exports a formatter for a closed error row with
generic payload `a` and a `where a: Show` bound. After serialization, dropping
the producer, and reload, independent Number/String consumers work, including a
locally named group with reordered tags. A function payload is still rejected;
the reviewed colorless snapshot labels the consumer's formatter call. This
checks preservation of the payload bound rather than merely an expanded
concrete group type.

The CLI traits fixture also calls a generic formatter from a separate module
with Number, String, and a custom-Show enum payload. Exact output assertions
confirm the generic dictionary is forwarded to the structural error formatter.
All 149 driver unit tests, the standalone CLI e2e test, targeted driver Clippy,
formatting/diff checks pass; no pending snapshots remain. This follow-up needed
only regression coverage, not an implementation change. The earlier workspace
unit/integration run remains the latest broad run. Recursive payloads, open-row
composition, direct group normalization, and the remaining capability migration
are still open; these tests do not close those separate requirements.

### Recursive nominal payload execution

The CLI fixture now contains `FailureTree[a]`, a derived-Show nominal enum whose
recursive branch is a Result with `:cause(FailureTree[a])` in its error row.
Wrapping that tree in another structural error exercises the recursive enum
dictionary through both Result and error-row evidence. Exact output passes for
nested Option payloads and a second instantiation using a custom-Show enum,
including the cross-module generic formatter. Independently constructed trees
containing `Some(None)` also compare equal. This passed without implementation
changes: finite nominal recursion is distinct from prohibited recursive
structural group expansion.

The standalone CLI e2e suite, formatting, and diff checks pass. This is bounded
finite-value execution evidence, not a stack-safety or cyclic-object formatting
guarantee. Recursive stored interfaces, open-row capabilities, and the remaining
nominal derive/structural capability migration still need their own work.

## Reproductions

At `570ee29`, the full trait-solving entry point (`solve_input`, not the legacy
inference-only helper) accepts:

```alder
fn identity(value: Result[Number, String]) { value }
```

It rejects each of these independent programs:

```alder
fn success() Result[Number, String] { Ok(42) }
```

```alder
fn failure() Result[Number, String] { Err("bad") }
```

```alder
fn read(value: Result[Number, String]) Number {
    match value { Ok(n) => n, Err(_) => 0 }
}
```

The first error reports `[_]` versus `String`; the latter two report `a` versus
`String`. These were executed through a temporary solver audit test, subsequently
removed rather than committing assertions that bless the inconsistent behavior.
The earlier real CLI extern fixture independently exposed the pattern failure.

## Root causes and conflicting evidence

- `Infer::from_ast` delegates the second Result argument to
  `convert_ast_error_type`. A bare variable there is assigned `ErrorRow` kind,
  including the built-in Ok/Err constructor's generic error parameter.
- An ordinary concrete named type in that same position falls back to ordinary
  type conversion unless it is a named error group. Thus String annotations
  survive while constructor instantiation and patterns cannot unify with them.
- `unify_return` uses error-row inclusion for Result returns, independently of
  whether the concrete error argument is actually a row.
- Existing higher-kinded tests intentionally exercise `Result[Number, String]`;
  they are not proof that constructing, matching, or propagating it works.
- `docs/language.md` and the M4 plan describe tagged rows and named error groups,
  with separately kinded row variables, but do not explicitly settle support for
  arbitrary ordinary error types.

## Decision and required follow-through

The user approved error rows/groups only, not ordinary error types. This needs
consistent annotation validation, constructors, patterns, return checking, `?`,
aliases, higher-kinded applications, externs, stdlib signatures, and cross-module
interfaces. Diagnose invalid annotations at their error argument and replace
obsolete non-row fixtures with supported row cases. Add positive and negative
source-aware diagnostics and CLI regressions. No solver semantics were changed
by recording this decision. See `plans/hardening-language-decisions.md` for the
related structural capability and Option propagation decisions.

## Resumed implementation checkpoint

Permanent full-solver regressions now reject unused Result annotations whose
error argument is String, Bool, Array, tuple, or record, while accepting named
groups, structural rows, and generic error tails. The ordinary-type regression
failed before the fix. Converted error arguments now retain a deferred kind
check at their source region, checked after inference so an unused annotation
cannot silently publish an ordinary error type.

Obsolete higher-kinded fixtures now use real error rows. The two-hole ordering
test uses a two-parameter enum because it tests ordinary value arguments of both
parameter types, not error-row values. Four changed source-aware snapshots were
reviewed: preserved higher-kinded contracts, transparent alias expansion, and
incompatible fixed error rows remain covered.

Validation: solver integration 295/296 pass; the sole failure remains the
pre-existing forward tuple-projection regression. Strict workspace Clippy,
formatting, and diff checks pass; no pending snapshot files were found.
This is not completion of the error-kind audit: validate constructor
identity (not just the spelling Result), partial/higher-kinded applications,
unused declarations and aliases, row-tail kind reuse, and stored interfaces.
Add dedicated rendered diagnostics and actual CLI coverage before closing it.

## Unused declarations and diagnostic checkpoint

The resumed audit reproduced acceptance of both an unused invalid alias and an
unused enum containing Result[Number, String]. Inference previously skipped
these declaration types entirely. Alias bodies and tuple/record enum payloads
now pass through annotation conversion even when never instantiated. Permanent
tests cover these failures, alias substitution, and supported rows/groups/tails.

Invalid converted error arguments now use InvalidResultErrorType instead of a
generic mismatch against `[_]`. Two reviewed colorless miette snapshots label
the actual String argument in a function and an exported unused alias, explain
the row/group requirement, and suggest a tagged row. Normal CLI color handling
is unchanged. Elm's annotation-hint approach was consulted for explaining the
contract rather than exposing solver internals.

At this checkpoint remaining declaration coverage included bodyless trait
methods, associated bindings, error-group payloads, and type-bearing deferred
constructs. The
constructor identity, partial/higher-kinded, row-tail, and cross-module audits
above remain required; these declaration checks are not proof of complete kind
checking.

Validation for this checkpoint: all 128 driver tests and strict workspace
Clippy pass; solver integration is 298/299, with only the known forward tuple
projection failure. Formatting and diff checks pass.

## Bodyless contract checkpoint

Further permanent regressions reproduced invalid Result error types in an
unused bodyless trait signature, an error-group payload, and an associated-type
binding. These declaration types now pass through conversion and deferred kind
validation without depending on a value-level use. The trait test covers both
parameter and return positions. A reviewed colorless driver snapshot verifies
that the method signature retains the original error-argument source region.

All 129 driver tests pass. Solver integration is 301/302, with only the existing
forward tuple-projection failure. The remaining audit is not closed: constructor
identity, partial/higher-kinded applications, conflicting row-tail kinds,
cross-module contracts, recursive error-group expansion, and deferred
type-bearing constructs still need scrutiny. In particular, validating more
declarations must not introduce unbounded recursive group expansion.

## Recursive structural expansion checkpoint

The exact test for `error Recursive { :nested(Result[Number, Recursive]) }`
aborted with a Rust stack overflow before the fix (run in an isolated test
process with core dumps disabled). Error-group conversion now tracks the active
expansion path by canonical name. Re-entering a group records a source-located
RecursiveErrorGroup error instead of expanding indefinitely; the error must be
reported before inferred contracts can be published. Groups leave the active
set on completion, so sharing a group across sibling payloads is not a cycle.

Regressions cover direct cycles, cycles through Array payloads, mutual cycles,
repeated acyclic groups, and recursive nominal enum payloads. A reviewed
colorless miette snapshot labels the cyclic reference and recommends an enum
for recursive payload structure. This follows the existing prohibition on
recursive structural aliases, not a restriction on nominal recursive enums.
It does not resolve all remaining structural error-group identity/capability
work or establish stack safety for arbitrarily deep acyclic source types.

Validation: all 130 driver tests, strict workspace Clippy, formatting, and diff
checks pass. Solver integration is 303/304; the only failure remains the known
forward tuple-projection regression.

## Partial constructor checkpoint

A source-level user declaration named Result is rejected as a duplicate of the
built-in during canonicalization; it did not reproduce a legal name-collision
program. Inference's Result-specific checks now nevertheless use its complete
built-in canonical identity rather than its name alone.

The actual bypass reproduced in an impl head: `Result[_, String]` was accepted
because partial constructor fixed slots used ordinary conversion. Fixed Result
error slots now use error-row conversion and validation. A positive dispatch
test then exposed a second defect: trait template matching rejected every
structural error row, including a row identical to the implementation's fixed
slot. It now matches tags and payloads structurally, requires exact closed rows,
and binds residual rows for open tails. Tests cover valid dispatch, reordered
tags, open tails, and rejection of wrong payloads, wrong tags, and extra tags in
a closed-row instance. This does not authorize custom instances on individual
named error groups; the tests implement a trait for a partial Result constructor.

Named-group normalization in trait matching, higher-kinded application-created
Result types, and row substitution/coherence remain audit work. These fixes do
not establish that every higher-kinded error-row path is correct.

## Structural row coherence checkpoint

The duplicate-instance check already rejected reordered closed rows. It failed
to reject overlap between `Result[_, [:left | e]]` and
`Result[_, [:left | :right]]`: the coherence head representation discarded error
row tails and required equal tag counts. The permanent regression accepted both
implementations before the fix, despite lookup being able to select either.

Coherence heads now preserve error-row tail variables, their shared identity,
and occurs-check traversal. Overlap unification compares common payloads and
unifies residual rows into open tails, allocating a fresh shared tail when both
sides are open. Regressions cover reordered duplicates, open/closed overlap,
independent open/open overlap, and valid disjoint payloads/closed tags. A reviewed
colorless driver snapshot labels both conflicting impl declarations.

The analogous record-head representation still omits tails; audit that separate
path rather than assuming this error-row fix addresses record coherence. Named
error-group normalization and full substitution/coherence agreement remain open.

Validation for the coherence checkpoint: both solver library tests, all 131
driver tests, strict workspace Clippy, formatting, and diff checks pass. Solver
integration is 313/314, with only the existing forward tuple-projection failure.
No pending snapshot files were found.

## Shared-tail adversarial checks

Two additional full-solver regressions verify coherence across multiple trait
arguments: a shared tail cannot satisfy different closed residual tag sets,
while matching residual sets make the implementations overlap. Both pass
without further implementation changes. Derived Eq for a generic enum field
`Result[a, [:failed]]` also passes and now has a permanent positive regression.

Inspection noted that `substitute_type` falls back to Any for several structured
forms, including an explicit error-row template. This is not yet a reproduced
defect: source where-constraints permit only a type-variable subject, and the
derived-Eq probe did not expose a failure. Do not claim the successful derived
test proves every prerequisite substitution path, or broaden constraint syntax
merely to exercise the helper. Trace generated/interface predicates if auditing
this fallback further.

Checkpoint validation: all 130 driver tests and strict workspace Clippy pass;
solver integration is 308/309, with only the pre-existing tuple projection
failure. Formatting and diff checks pass.
## Named error-group implementation boundary

The approved structural-group design prohibits per-name custom implementations.
A new regression reproduced successful compilation of `impl Marker[Failure]`
for `error Failure { :failed }`. Coherence now rejects source-origin
implementations with a direct error-group trait argument, following transparent
aliases and consulting both local and imported group identities. The driver
routes the diagnostic to the implementation's module and source region.

The direct and alias regressions pass; a nominal enum wrapping a Result whose
error slot names the group still permits a custom implementation. This does
not reject genuine wrappers or blanket implementations merely because they
can contain errors. Compiler-generated derive/automatic instances are not
source-origin custom implementations and are unchanged by this checkpoint.

Remaining structural work is not waived: canonical capabilities must be
conditional on payloads and independent of originating group names/order;
nominal fallback and the current derive inventory need reconciliation with the
approved policy. Container heads naming groups in Result error slots still
need structural normalization, not per-name dictionary behavior. Cross-module
rejection coverage, diagnostic review, and full validation remain required.

Checkpoint evidence: the colorless diagnostic snapshot was reviewed for the
actual source, implementation label, and wrapper guidance. All 360 solver
integration tests, 134 driver tests, and 12 CLI tests pass. Strict workspace
Clippy, formatting, and diff checks pass; no pending snapshots were found.
Added the error-group-custom-implementations Sampo changeset. Cross-module
rejection and the broader structural-capability/packaging audit remain open.

### Imported implementation diagnostics

Cross-module tests now reject both an imported group and an imported transparent
alias, in both source-discovery orders. A serialized dependency interface test
also rejects an alias after its producer has been dropped. The consumer owns
the trait, so these tests exercise the group restriction rather than orphan
checking.

The source-span assertions reproduced a separate reporter defect: implementation
origins retain source-item ordinals, but canonical item arrays omit imports and,
in the header pass, ordinary value declarations. Indexing that array by the
source ordinal produced an empty fallback label. The reporter now finds the
implementation by its full identity. Generated implementation items retain
their originating declaration regions and use the same lookup.

The imported tests assert the exact implementation source slice, not merely
compilation failure. The stored-interface colorless snapshot was reviewed: it
labels line 4 of the consumer despite both an import and a preceding function.
This closes the imported-rejection coverage gap above, not the remaining
structural-capability, container-normalization, or packaging work.

### Structural Result instance heads

Two more regressions reproduced disagreement between coherence and inference:
equivalent groups in `impl Marker[Result[Number, Group]]` were considered
disjoint, while a call using an equivalent row could not select either named
head. Coherence now expands named groups (including transparent aliases) in
Result error slots and fixed error slots of partial Result constructors. It
compares tags and payloads structurally, including their nested type shapes.
This does not turn true nominal wrappers into aliases or change compiler-owned
direct group capability instances, whose migration remains outstanding.

Instance matching now consults the group database when a named template meets
an inferred error row, using the same closed-row matching helper as literal
rows. The database is threaded through recursive matching and associated-type
instance selection. Permanent tests cover reordered equivalent groups,
alias/literal partial constructors, disjoint payload types, and successful
selection across names. CLI traits coverage exercises the selected dictionary
on both Ok and Err and calls it from another module.

The first attempted overlap probe used `|` between group declarations and failed
parsing; the corrected comma-separated probe reproduced successful solving
before the fix. The selection probe independently reproduced MissingInstance
before its fix. Neither parsing failure nor general suite success is used as
evidence of the original defects. Broader capability inventory, recursive
payload/interface coverage, and final acceptance gates remain open.

CLI probe follow-up: a two-tag fixture exposed another unresolved failure:
`error Second { :missing, :failed(Number) }` followed by
`let failure: Result[Number, Second] = Err(:failed(7))` was rejected with
expected `[:failed(Number) | :missing]`, found `[:failed(Number)]` in the
traits project. Investigate constructor row widening and the surrounding
project's tag environment; this is not resolved by head normalization. The
dictionary execution fixture now uses one-tag equivalent groups to isolate
selection; the solver overlap/selection regressions retain reordered two-tag
groups. Multi-tag constructor execution is still required, not waived.
The first imported annotation also used unsupported dotted type syntax; it was
corrected to a named type import rather than changing the language grammar.

### Contextual singleton Err construction

The multi-tag failure above reproduces without the CLI project's surrounding
tag environment. A fresh `Err(:failed(7))` gets a singleton closed error row;
the contextual expression checker previously unified it exactly with the named
two-tag annotation. Fresh Err calls now use the existing directional Result
return check: payload types unify, and the constructed error row must be
included in the expected row. Ordinary already-typed values and invariant
container aliases retain their existing checks; this is not blanket row
subtyping or an implicit conversion.

Regressions cover a local annotated binding, fresh array elements, function
arguments, both Err and Result.err spelling, and rejection of unknown tags or
wrong payloads. The CLI fixture again uses two reordered multi-tag groups and
executes both local and imported dictionary calls successfully. This resolves
the specifically recorded singleton-construction failure; arbitrary expression
forms, constructor aliases, and broader structural capability work still need
their corresponding audit rather than inheriting a completeness claim.

### Context through block tails

A subsequent probe found that wrapping the fresh Err initializer in a block
lost its expected type and reproduced the same singleton-versus-multi-tag
mismatch. Block inference now accepts an optional expected tail type; ordinary
inference still uses the same implementation without one. Contextual checking
passes the expectation to a falling-through tail, while statements keep the
enclosing function's separate return boundary. Blocks that exit do not need
fake initializer values, and reachable unit fallthrough remains checked.

The analogous two-branch probe already passed before this change and is retained
as coverage, not reported as a second defect. Permanent tests check both forms,
early return through the enclosing boundary, and incompatible unit fallthrough.
The CLI traits fixture includes a contextual block with a side effect and checks
its dictionary result plus exactly-once execution. General constructor aliases,
other expression forms, structural capability inventory, and final acceptance
gates remain under audit.

### Built-in Result identity

`is_result_err_expr` recognized a function named `err` using only the final
module-path segment `Result`. A driver reproduction in application source
`Result.ald` compiled `fn err(value: a) a { value }` followed by a call with
`:failed`, bypassing the rule that tags are values only inside Err. Recognition
now requires the Builtin package, exact `["Result"]` module path, and function
name. Constructor recognition already required the Builtin package.

The driver regression covers a same-module call. An attempted imported-module
probe used `import ~/Result`, which the grammar rejects; it was removed rather
than counted as identity-check evidence. No import grammar change is intended.
The existing solver contextual-array regression still accepts actual Result.err
alongside Err, so tightening identity does not remove the intended built-in
entry point. The same helper also controls contextual fresh-Err treatment and
therefore no longer grants that treatment to unrelated user functions.
