# Structural instance selection

Status: active; related record findings are in record-coherence-hardening.md.

Confirmed function-head defect: match_type rejected every Type::Fn, although
coherence and method checking accepted function implementation heads. A local
Invoke[fn(Number) Number] implementation failed at its call with MissingInstance.
The positive source regression failed before the fix and passes afterwards.

Function matching now requires equal arity, recursively matches each parameter
and the result, and uses the same binding map throughout. Repeated template
variables must denote the same type; fn(a) a cannot match fn(Number) String.
This selects dictionary evidence only; it introduces neither function subtyping
nor a separate runtime dispatch mechanism. No generated-JS protocol changes.

Added positive generic selection at Number and String, and negative parameter,
result, arity, and repeated-variable cases. The imported function_instances CLI
module exercises a method invoking a callback and a generic default method
returning the original callback. Validate this through the actual CLI suite.

Remaining audit: applied variable heads, associated projections and normalization,
substitution of structured prerequisite types (currently several fall back to
Any), generic callbacks carrying their own dictionaries, nested function/row
heads, and stored interfaces. These are not proven correct by the current tests.
Do not mark the broader trait hardening complete from this checkpoint.

Validation: all 346 solver integration tests, 12 CLI tests, 133 driver tests,
and 51 codegen tests pass. The CLI suite includes the imported function instance
module and executes all three new assertions. Strict workspace Clippy,
formatting, and diff checks pass; no pending snapshots were found. Added a
function-instance-selection Sampo changeset. Full workspace/release packaging
and the original hardening acceptance audit remain outstanding.

## Async callback evidence

The explicit_async CLI fixture imports an Invoke implementation for
fn(Number) Task[Number], whose async method awaits its callback. Creating the
method's task produces no callback events. Awaiting the same task twice yields
42 twice and adds exactly one event per execution, including an actual
Task.sleep(0) suspension in the callback. This checks imported dictionary
selection, explicit async method lowering, lazy construction, and reusability
together. No new runtime or compiler change was necessary.

A solver regression rejects both fn(Number) Number and
fn(Number) Task[Task[Number]] against this one-Task implementation. Matching
neither wraps plain return types nor flattens nested tasks. All 347 solver
integration tests and 12 CLI tests pass after these additions. This extends
acceptance coverage without completing the outstanding selection audit.

## Applied constructor variables

Confirmed another missing-instance defect: `impl Marker[Array[f[a]]]` was
accepted, but neither Array[Option[Number]] nor Array[Array[String]] selected
it. The initial positive regression failed with two MissingInstance errors.
The matcher rejected every applied Type::Var.

Equal-arity applications now match structurally: bind f to the actual head and
match its arguments using the same binding map. Repeated f occurrences must
agree; a function from Option[Number] to Array[Number] cannot match fn(f[a]) f[a].
Constructor bindings are available to prerequisite resolution, including a
Functor[f] bound. This introduces no instance-driven type unification.

The basic positive regression passes. Added a prerequisite regression and
imported CLI calls to a generic default method on nested Option and Array
values. Unequal-arity partial recovery remains required follow-up: review
docs/traits-internals.md's leftmost-hole abstraction convention and the active
unify_higher_kinded_pattern implementation together before implementing it.
The active implementation also handles fully concrete pattern arguments;
the older prose is not complete evidence of its current accepted domain.
Equal-arity matching is an implementation checkpoint, not the final scope.

The prerequisite regression exposed a representation mismatch in the initial
fix: binding f to a bare nominal head made Functor[Option] and Functor[Array]
fail against canonical all-hole instance heads. Constructor bindings now use
the same Partial representation with ordered holes as canonical schemes.
The failure was reproduced by both the solver probe and actual CLI tests;
the prerequisite fixture is retained rather than weakened by dropping its bound.

After that correction, all 350 solver integration tests and 12 CLI tests pass,
including imported constructor prerequisites. Strict workspace Clippy,
formatting, and diff checks pass. The applied-instance-heads Sampo changeset
records this slice. Partial recovery, remaining structural substitutions, and
full hardening/release gates remain open.

## Leftmost partial recovery and coherence

Two new regressions failed before this checkpoint: Array[f[a]] did not select
for Array[Result[Number, [:failed]]], and coherence accepted that generic head
alongside the concrete Result head. Selection and coherence therefore needed
to change together rather than enabling broader matching alone.

Matching named applications now abstracts the leftmost pattern arguments into
ordered holes and retains remaining arguments as fixed slots. The selected
constructor is Result[_, [:failed]], which can resolve its Functor prerequisite.
This follows the existing leftmost-section convention, not arbitrary type lambdas.

Coherence now represents constructor sections explicitly instead of treating
holes as fresh value-type variables. It compares hole positions and fixed
slots, applies fully supplied sections during head pruning, and abstracts a
shorter variable-headed application against a longer named application. Occurs
checks traverse fixed slots. Reusing a section preserves its fixed arguments.
Record rows, error rows, and constructor sections remain distinct terms.

The initial complete solver run passed all 352 tests. Added two further
coherence probes for reusing a recovered Result section with the same versus
different fixed error rows, plus actual CLI execution through the imported
Functor-constrained nested-container implementation on Results.

Still required: explicit non-leftmost sections in mixed instance arguments,
matching-order independence for previously bound constructor variables,
structured prerequisite substitution, interface/adversarial acceptance, and
full hardening/release gates. The prior “partial recovery remains” notes are
historical; this checkpoint implements leftmost recovery, not every related case.

Validation after all new probes: 354 solver integration tests, 12 CLI tests,
133 driver tests, and 51 codegen tests pass. Strict workspace Clippy,
formatting, and diff checks pass; no pending snapshots were found. Updated
the applied-instance-heads changeset. No commit or release action was taken.

## Explicit section order in coherence

A valid declaration-only regression exposed another overlap miss:
Marker[f[a], f] and Marker[Pair[Number, String], Pair[Number, _]] were accepted
together. Coherence defaulted f to a leftmost section while checking argument
one, then rejected the supplied rightmost section in argument two. The two
heads do overlap when f is the explicitly supplied section.

Coherence now collects explicit section equalities before application recovery,
including aligned nested structural positions. The ordinary unification pass
still checks complete shape compatibility and fixed-slot constraints. The
regression passes after the fix; the initial full solver run passes 355 tests.
Added reversed-argument and incompatible-fixed-slot probes for final validation.

Initial attempted call-site fixtures used partial constructors inside ordinary
value annotations and then a bare higher-kinded alias in that position; these
failed canonicalization (InvalidHole and BadArity respectively), not instance
selection. They are not counted as matcher defects or accepted syntax, and no
grammar change was made. The permanent regression uses supported explicit
partial constructors directly in trait implementation arguments.

Remaining: broader prebound-section matching at call/projection sites, constructor
aliases across structural positions, optional-record coherence, and all original
acceptance/release requirements. This fixes the demonstrated coherence order
case; it does not prove general higher-order unification.

Final checkpoint validation: full `cargo test -- --quiet` exits 0, including
357 solver integration tests, 12 CLI tests, 133 driver tests, 51 codegen tests,
50 kernel tests, and all other workspace suites. The three previously ignored
doctests remain ignored. Strict workspace Clippy, formatting, and diff checks
pass; no pending snapshots were found. This supersedes earlier partial-suite
evidence for the current worktree, but does not satisfy outstanding packaging,
implementation, documentation, or commit gates by itself.
