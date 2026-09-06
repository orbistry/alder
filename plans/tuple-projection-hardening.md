# Tuple projection inference

Status: sparse projection production, fixed-arity constraints, scheme/interface
transport and consumer checks implemented; broader acceptance audit pending.

## Current recursive-group check

Three new solver regressions exercise mutually recursive functions projecting
indices zero and three. Both declaration orders accept a four-slot argument;
a two-slot caller is rejected. Whole-value returns preserve untouched slot
relationships, with separate calls independently using Bool/String and
String/Number middle elements. No production change was needed.

Two bounded cross-module calls in the records CLI fixture execute through both
recursive entry points and assert the returned tuples are unchanged. The fresh
packaged CLI executes this fixture successfully from `/tmp`. All 436 solver
integration tests and 14 unit tests pass. This verifies projection collection
and ordinary recursive scheme instantiation for these cases; it does not close
the separate generic trait-evidence or joint deferred-constraint audit.

## Current optional-lifting interaction check

Two solver tests cover tuple shape preservation when a generic optional consumer
appears before versus after projection collection. Positive calls independently
instantiate unobserved slots as Number and String. Negative cases reject an
extra tuple element, a wrong projected element type, and a changed unobserved
return element. These cases pass without a production change.

The traits CLI fixture additionally exports both statement-order variants and
calls each with Number and String second slots from another module. Its optional
consumer asserts that the supplied tuple arrives as Some, while callers assert
the returned tuple is unchanged. Execution with the freshly packaged CLI from
`/tmp` passes. All 433 solver integration tests, 14 solver unit tests, and solver
doctests pass; formatting also passes.

This closes these particular Option/tuple interaction probes, not the remaining
recursive-group, trait-evidence, and joint fixed-point acceptance review. Earlier
red-suite and dense-finalizer notes below are historical, superseded by the
sparse source implementation and subsequent validation.

## Confirmed failure

`fn sum(pair) Number { pair.0 + pair.1 }` rejects the second projection with
`actual: (a), expected: ()`. Reversing the accesses succeeds and publishes
`fn((Number, Number)) Number`. Two permanent source-aware inference tests now
reproduce this contrast; the forward-order success test intentionally remains
red until the implementation is fixed. The reverse-order snapshot was reviewed.

The `Expr::TupleAccess` inference arm immediately binds an unknown operand to a
closed tuple of length `index + 1`. This mistakes a minimum-index requirement
for an exact arity and even constructs a one-element internal tuple for `.0`,
although surface tuple syntax has no one-element tuple.

The same arm allocates fresh variables directly from the source index. A large
index can demand a huge allocation before a real tuple shape is known. This is
an inspection finding, not a stress test run: do not deliberately exhaust the
host's memory. The replacement should retain sparse index requirements until
shape resolution rather than imposing an arbitrary language-wide tuple limit.

## Approved language choice

Use fixed inferred tuple arity, collecting all projection requirements before
fixing arity; minimum inferred arity is two. Arity polymorphism is not approved.
Do not freeze a premature tuple size. See `plans/hardening-language-decisions.md`.

Elm's Type/Constrain/Expression.hs uses fixed tuple constructors for tuple
literals; it does not supply Alder's numeric projection inference semantics.

## Required coverage

- Ascending, descending, repeated and sparse projections; at least four slots.
- Aliases, local and top-level bindings, nested tuple projections and patterns.
- Exact annotations, too-short/non-tuple operands, and indexed assignment.
- Generic signatures, recursive groups, generalization and stored interfaces.
- Concrete tuple arity remains exact; preserve element relationships.
- Large indices are rejected or retained sparsely without source-sized allocation.
- Actual CLI execution and a source-aware out-of-bounds diagnostic.

Other hardening and concurrency requirements remain in scope independently.

## Known-shape diagnostic checkpoint

An actual CLI check of `fn read(pair: (Number, String)) { pair.2 }`
reproduced the misleading `expected (), found (Number, String)` diagnostic.
Known tuple reads and assignment places now emit `TupleIndexOutOfBounds` with
the index, tuple length, and index-token region. The miette report states the
tuple length and valid zero-based range. Two reviewed colorless driver snapshots
cover reads and writes. This does not change unknown-tuple inference or choose
an arity policy. All 125 driver tests and strict workspace Clippy pass; full
tests still fail only the existing forward-projection case (293/294 solver
integration tests pass). Formatting and diff checks pass.

## Projection collection checkpoint

Unknown tuple reads and writes now retain projection constraints instead of
immediately binding a closed tuple. Equal accesses unify their element variables
before selecting a shape, including accesses through nested aliases. Known
tuple shapes discharge constraints directly. At the SCC generalization boundary,
remaining unknown shapes use all collected indices and infer at least two
elements. Read and assignment paths share the same projection operation.

The original forward-order regression now passes with the same contract as the
reverse-order case. Reviewed source-aware snapshots cover nested aliases with a
four-element inner tuple, combined read/write requirements, and the minimum
two-element inferred shape. CLI records fixtures now include exported tuple
helpers and assertions for caller-visible mutation and untouched fields.

This is not the completed sparse-inference fix: finalization still builds a
dense vector of inferred elements, so a very large representable source index
can still demand excessive memory. Do not run an OOM probe or mark this section
complete. A sparse fixed-length representation (including schemes/interfaces)
or another justified bounded strategy is still needed; do not introduce an
arbitrary language-wide tuple-size cap. Also audit interactions with deferred
record overlays, recursive groups, late shape equalities, and generic contracts.

Validation: full `cargo test -- --quiet` succeeds, including 320 solver
integration tests, 131 driver tests, 50 kernel tests, and the workspace
doctest pass (three existing doctests remain ignored). Strict workspace Clippy
and formatting pass. The standalone CLI end-to-end test was rerun after adding
the cross-module tuple assertions and passes. Four new inference snapshots were
reviewed; no pending snapshot files were found. This evidence does not close the
remaining sparse-finalization and hardening acceptance work.

## Generic and overlay boundary checks

Additional full-solver regressions reject tuple reads and writes that would
specialize an explicitly generic argument. A whole-tuple return test verifies
that unprojected slots preserve their input/output type relationship, including
rejection when a String slot is claimed as Bool. An annotated Number-returning
function combining record overlays and four-slot tuple projections also passes.

The first overlay probe omitted its result annotation and failed on an
unsatisfied generic Num obligation. An extra overlay pass did not change that
result and was removed. The intended Number annotation resolves the obligation;
this was not evidence of a tuple/overlay scheduling defect. These checks do not
close the sparse finalization or deferred-constraint audit.

## Index parsing checkpoint

The parser regression `t.4294967296` was accepted and silently saturated to
4294967295. Checked digit accumulation now rejects overflow with a dedicated
syntax error rather than changing the index. Boundary snapshots preserve the
maximum u32 index exactly and reject its successor; the driver diagnostic is
rendered without ANSI color and labels the index start. This preserves the
existing AST integer width, not a new smaller tuple-size cap. Large representable
indices on unknown tuples still require sparse inference; this patch does not
resolve that allocation risk or the arity-policy question.

Committed independently as `14be079`, including parser and colorless driver
snapshots and the Sampo changeset. The latest workspace unit/integration run
passes all 1,310 parser tests and 166 driver tests, and strict workspace Clippy
passes. The sparse-inference work remains a separate pending checkpoint.

## Sparse finalization implementation route

Current-state tracing confirms that replacing only the dense allocation in
`solve_tuple_projections` is insufficient. `Ty::Tuple(Vec<Ty>)` reaches
`annotation`/`to_ast`, canonical `Type::Tuple`, arena interface copying, and
owned-interface serialization. Every unobserved slot currently receives its
own fresh variable. Replacing those slots with Any, one shared variable, or
dropping them would respectively weaken checking, incorrectly equate unrelated
positions, or lose input/output relationships.

Use the existing scheme-obligation architecture as the next implementation
route: retain the tuple operand's type variable and attach an exact-arity,
sparse projection constraint to it. This is an internal representation, not
source-level arity polymorphism or new tuple syntax. The exact length becomes
fixed at the same SCC boundary as today, after collecting all projections.
Unobserved slots need no individual variables until an actual concrete shape
or another projection constrains them. Reusing the operand variable preserves
whole-tuple input/output identity without enumerating the gaps.

Implementation checklist (not yet implemented):

- Separate collecting projection requirements from fixing their exact length.
  Store a width-safe length: max u32 index plus one does not fit u32, and the
  existing `index as usize + 1` also overflows on a 32-bit host.
- Preserve the operand variable plus sorted `(index, element type)` entries,
  source region, and fixed length. When operands unify, require equal fixed
  lengths and unify intersecting element requirements; do not extend an
  already-fixed length. Detect recursive element/operand equations without
  materializing a dense tuple.
- On unification with a concrete tuple, validate its exact length first and
  check only constrained elements. Never manufacture a dense tuple from a
  projection index. Tuple literals and explicit annotations may remain dense
  because their storage is proportional to actual source elements.
- Carry these constraints in Scheme and Annotation, following record overlay
  and error-row inclusion handling: free variables, value restriction,
  quantification, independent instantiation, arena copying, owned dehydration/
  hydration, and cache identity all require coverage. Instantiated diagnostic
  origins must point to the consumer rather than the producer.
- Ensure generic-contract and trait-evidence checks cannot mistake a constrained
  operand for an arbitrary universally quantified type. Propagate constraints
  through recursive groups and aliases before publishing an interface.
- Render a compact internal shape/constraint description for very large arities;
  diagnostic or snapshot rendering must not reintroduce index-sized output.
  Keep ordinary small-tuple displays readable without adding surface syntax.
- Add bounded allocation-count/constraint-count tests for max-index projection
  collection and stored-interface round trips, plus distinct unobserved-slot
  types, whole-tuple round trips, exact arity rejection, and independent calls.

This route avoids adding another structural Type variant to every consumer, but
does not waive any of the semantic obligations above. The dense finalizer is
still present and unsafe for large unknown indices. Do not run a maximal-index
full-solver probe until that path has actually been removed; a green workspace
suite cannot establish memory safety for this case.

## Sparse annotation storage checkpoint

Canonical annotations and owned schemes now have a `tuple_shapes` collection.
Each entry contains the operand type, u64 exact length, sparse u32-indexed
element types, and region. Arena interface copying and owned dehydration/
hydration preserve the entries. Interface format is now 5, with no legacy
reader; normal incompatible-cache handling applies. Fingerprints include the
new metadata through ordinary owned serialization.

This is prerequisite plumbing only: canonical declaration constructors and
current solver annotation emission still produce an empty collection. The
solver does not yet produce, instantiate, or enforce nonempty tuple-shape
constraints. Do not publish/merge this intermediate stage as a completed
sparse-inference implementation. The next step is Scheme/free-variable/
instantiation integration and replacing dense tuple finalization, followed by
the semantic and generic-contract checks above.

The storage regression constructs metadata directly (not a source-level huge
tuple), round-trips it through serialization and a discarded hydration arena,
checks a compact serialized size, and verifies that changing length changes the
fingerprint. It is deliberately not evidence that maximum-index source programs
are safe to compile yet.

Storage checkpoint validation: the sparse maximum-index metadata round trip
passes, as do all 139 driver tests (one existing ignored doctest), workspace
checking, strict workspace Clippy, formatting, and diff checks. The full
workspace test pass immediately preceding this schema change is recorded in
the main hardening plan and is not relabeled as post-change evidence.

## Imported shape constraint inference checkpoint

The solver now reads tuple_shapes from imported annotations, shares their type
variables with the function type, and records consumer regions. Scheme selection
includes the connected constraint variables, free-variable analysis includes
them for the value restriction, independent instantiation replaces them, and
annotation emission preserves them through inferred forwarders. Error-row
existential elimination also protects variables reachable through these shapes.

A driver test initially accepted a wrong String element despite stored metadata
requiring Number. `solve_tuple_shapes` now checks concrete tuple length and
constrained elements without constructing an expected dense tuple, rejects
non-tuples, combines matching unresolved operands, and detects direct recursive
element equations. Explicit universal variables cannot acquire tuple-shape
requirements. Consumer tests cover independent unconstrained first-element
types, wrong second-element type, wrong length, non-tuples, an inferred relay,
and rejection of an explicitly generic relay.

This still does not remove dense source projection finalization. Next integrate
projection lookup with existing fixed shapes and replace finalization with
constraint production. Validate transitive shape cycles, source generic
contracts, mutable captures, late shape equalities, and owned-interface
re-export/CLI behavior before claiming sparse inference is complete. The driver
probe deliberately creates stored metadata directly to isolate consumption;
it is not evidence that the source compiler emits sparse constraints yet.

## Sparse source production checkpoint

Source projection finalization now groups only the observed indices, computes
the exact length in u64 with a minimum of two, and retains the operand variable
with sparse shape constraints. The source-index-sized fresh-variable vector is
removed. Projections onto an already-fixed sparse shape use its element map and
reject out-of-range indices instead of enlarging its length. Known dense tuples
still check concrete elements directly.

After inspecting the changed path to establish that it no longer allocates by
index, a source-level maximum-u32-index test passed: one stored element, length
2^32, and only two quantified variables. The owned-interface storage test now
compiles this actual source rather than injecting metadata and checks compact
serialization, arena lifetime independence, and fingerprint sensitivity.

The first source switch preserved existing semantic tests but exposed five
snapshots whose helper omitted tuple constraints. The helper now prints compact
operand/length/element constraints; all five snapshots were reviewed for fixed
arity and preserved relationships, including identical ascending/descending
contracts. Stale generated `.snap.new` files from the omitted-constraint run
were removed after review.

A new mutual-element-cycle regression initially compiled under the sparse
representation. Iterative graph cycle detection now rejects transitive tuple
element equations, in addition to direct occurs checks, without recursively
expanding the shapes. This is a correction required by the representation
change, not evidence that arbitrary recursive types are now supported.

All 374 solver integration tests pass. Standalone CLI tests execute exported
tuple helpers, mutations and unaffected fields successfully. Remaining audit:
late shape equalities/fixed-point ordering, captured mutable values reachable
only through constraints, recursive groups and generic contracts, source-level
stored-interface forwarders, production diagnostic display, and final full
workspace/packaging acceptance. Earlier notes describing a still-dense finalizer
are historical checkpoints superseded by this section, not current behavior.

## Source-interface and capture checks

Actual source-produced constraints now have a stored-forwarder regression:
compile a tuple-preserving function, compile a separate dependency module
forwarding to it, serialize both interfaces, then check independent consumer
calls. Distinct previously unobserved slot types remain independent, a third
tuple slot is rejected under the fixed two-element contract, and the returned
tuple cannot claim a different unobserved slot type. These cases passed without
further solver changes. The initial producer fixture used a semicolon, which
the grammar rejects; it was corrected to an indoc multiline source before any
semantic claim was made.

A captured-array regression rejects incompatible Number/String uses when the
shared array flows through an inferred tuple element. A maximum-index function
compiled from source and serialized as an interface rejects a two-element
consumer tuple with a compact length diagnostic. Its new colorless snapshot
was reviewed: it labels the consumer's function reference, not the producer,
and does not expand the expected tuple into billions of slots.

These checks strengthen specific interface, mutation and rendering evidence;
late-equality ordering, broader generic/trait interactions and final full-scope
acceptance remain under audit. They do not establish exhaustive soundness.

Historical pre-fix validation: 1306 parser and 126 driver tests pass, as do strict workspace
Clippy, formatting, and diff checks. Full tests reach solver integration with
the same single forward-projection failure (293/294 pass). No pending snapshots.
