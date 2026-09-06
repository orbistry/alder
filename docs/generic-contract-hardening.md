# Generic contract acceptance evidence

## Current contract-audit reconciliation

The explicit-signature/default/implementation/HKT/associated-equality/bounds/
recursive-group/interface review in hardening requirement 1 is complete for the
integrated implementation. This is an evidence-backed audit, not a proof of
general compiler soundness or final committed-tree release verification.

`infer_function` keeps inference positions flexible, registers declared
universals and independently substituted trait-method universals, and derives
implementation dictionary order from the trait contract. Implementation-only
bounds remain obligations; declared associated equations are installed before
signature/body checking. Default bodies now scope every trait-head variable.
Local and lambda annotations reuse the enclosing callable scope, which is
restored after each body along with givens and projection equations.

The final module loop settles record initializers, Try constraints, tuple
projections/shapes, overlays, and error-row constraints before
`check_generic_contracts`. That check rejects concrete specialization, merged
independent representatives, shared-environment escape, and undeclared tuple or
error-row restrictions. Overlay universal proofs and retained-row behavior have
their completed source/acceptance map in `record-row-acceptance.md` and the
record-overlay plan. The earlier open-overlay notes below are historical.

After this boundary, `resolve_obligations` consumes an `InferenceResult` and
constructs evidence through `resolve_predicate`; it does not receive the mutable
`Infer` substitution state. Published annotations and stored-interface tests
therefore describe the contract checked before evidence construction. Broader
dictionary lowering and evaluation-order audits remain separate delivery items.

The permanent matrix below is supplemented by thirteen local-annotation cases,
including recursive peers, trait-head scope, and unmentioned higher-kinded
Functor parameters. Both the valid inherited-Functor case and rejection of an
attempt to specialize that constructor to Array pass. Stored method and recursive
local-annotation producers are serialized, discarded, and reloaded for consumers
with independent Number/String calls. Source-aware diagnostic and compiled
cross-module evidence are retained; no test is a substitute for the source-path
review or the final integration/package gates.

## Local annotation scope correction

Local let annotations now follow the documented callable annotation rule:
an enclosing type-variable name is reused; otherwise it is fresh for that
annotation. Local bindings remain monomorphic and fresh names do not leak into
sibling declarations. The prior canonicalizer used an empty permitted-name set,
rejecting valid `let copy: a = value` inside `fn copy(value: a) a`.

Allowing those names alone reproduced a second bug: the local solver built its
annotation with a fresh map and accepted `let unused: a = 42` inside an otherwise
universal function. The solver now clones the enclosing annotation scope, as it
already does for lambda annotations, so the final generic-contract check catches
that specialization. No new generalization rule or dictionary ABI is introduced.

Seven solver tests cover valid scalar/higher-kinded contracts, rejected
specialization, sibling freshness, monomorphic shared arrays, nested lambda and
async scopes, and trait default/override/bound evidence. The driver snapshot
`renders_local_annotation_specializing_an_enclosing_generic_without_color`
contains the actual source and reports Number specializing the promised `a`.
The compiled Option-law fixture now uses a body-local `Result[a, ...]` annotation
and passes for independently instantiated nested Number and unit payloads.

The local-scope finding is resolved; this does not close the remaining wider
generic/evidence integration review or final package gates.

## Default-method scope follow-up

Default-method follow-up: the annotation scope previously contained only names
encountered in method signatures. A trait parameter absent from those signatures
was represented by a separate fresh variable when creating the self predicate,
and was neither scoped over the body nor included in the universal contract.
In `trait Marker[a]`, a default `fn value() Number` containing
`let unused: a = 42` reproduced acceptance of a specialization. Default
inference now seeds every trait-head parameter before checking signatures,
building self/superclass evidence, and registering the generic contract.

The negative regression fails before the fix and passes afterward. A positive
test checks an unmentioned `a: Show` bound inside a nested annotated lambda.
Recursive local annotations also retain independent peer contracts, reject peer
specialization, and survive producer serialization/deserialization into a fresh
consumer that calls the exported functions at Number and String. The focused
local-annotation suite now has eleven cases, all passing; the stored regression
passes without retaining the producer's arena.

## Earlier contract audit checkpoints

The following records `42c5423` plus the integrated hardening worktree, not an isolated
validation of that commit or a claim of general compiler soundness. The remaining
joint-constraint audit is tracked in the hardening
plan and the optional-arguments/record-overlay plans.

## Contract invariant and phase boundary

Current phase-order audit at `42c5423`: the final module loop runs overlay
solving, universal-overlay checks, tuple-shape solving, and error-row inclusion
solving before `check_generic_contracts`. The subsequent error-match and tag-
placement checks inspect resolved types/coverage without introducing equality
constraints. Obligation normalization uses projection equations or unique
instance-template matching; template variables are bound in a separate map,
not by unifying a checked goal variable. Type application reconstructs types.
Projection normalization can elaborate associated type syntax, so future changes
to that path must also preserve this boundary; this review is not a blanket
claim that every post-check operation is immutable.

The focused generic-contract selection now runs 14 integration tests, all
passing, and the stored trait-overlay independent-row regression passes too.
This confirms the inspected phase boundary and its existing regression matrix,
not full symbolic/cyclic overlay entailment. Those obligations remain open in
`plans/record-overlay-hardening.md`; no acceptance scope was reduced.

`GenericContract` in `alder-solve/src/inference.rs` distinguishes explicitly
promised universal variables from ordinary inference variables. Alder checks
the promise after solving rather than introducing a separate rigid Ty variant.
Each named universal must resolve to an unbound variable, independent of other
universals in the same contract and of the enclosing monomorphic environment.
Specialization, identification of independent variables, and escape are errors.
Tuple shape and directional error-row requirements are checked too.

Contextual Option solving must respect universals before generalization, not
only reject violations at the final contract check. The joint depth solver now
fixes a universal's assumed outer Option depth to zero: the definition treats
that type as opaque even though callers may instantiate it with an Option.
This permits representation-aware Some insertion without specializing the
promised parameter. Ordinary payload equality and the final independence/escape
checks still apply. See the universal-lifting checkpoint in
`plans/optional-arguments-hardening.md` for the reproduced defect and coverage.

`infer_function` registers both implementation annotations and the substituted
trait method's independent variables. Partial, unannotated positions retain
ordinary inference. Implementation-only bounds are obligations, not extra
dictionary parameters; the trait declaration determines dictionary order and
available projection assumptions. Default methods receive the trait's self
dictionary and declared bounds.

`infer_module` checks contracts after recursive groups and deferred constraints,
before exporting annotations. The subsequent evidence resolver consumes
normalized predicates and matches instance templates against them; it does not
receive the inference substitution table to specialize a checked universal.
An unresolved shared public export is rejected before publishing its scheme.

Reference: local Elm `compiler/src/Type/Unify.hs`, `unifyRigid`, rejects concrete
structure and distinct rigid variables while permitting flexible variables to
adopt the rigid contract. Alder enforces these obligations at the final solving
boundary, with additional mutation escape and row/tuple requirements. Any new
constraint-solving pass must preserve that boundary.

## Permanent coverage

| Requirement | Current regression evidence |
| --- | --- |
| Body/signature specialization | `generic_contract_rejects_a_specialized_method_body`, `_method_signature`, `_function`, `_default` |
| Independent universals and shared-state escape | `generic_contract_keeps_independent_parameters_distinct`, `generic_contract_rejects_escape_into_shared_mutable_state` |
| Recursive groups | `generic_contract_checks_specialization_through_recursive_peers`, `generic_contract_accepts_mutually_recursive_universal_functions` |
| Higher kinds and partial annotations | `generic_contract_rejects_higher_kinded_specialization`, `partial_annotations_share_variables_with_inferred_positions` |
| Bounds and dictionary ABI | `implementation_cannot_add_a_method_bound`, `implementation_cannot_add_an_unused_method_bound`, `implementation_bound_order_uses_the_trait_dictionary_abi` |
| Associated equalities | `implementation_cannot_add_a_projection_assumption`, `implementation_inherits_a_method_projection_equality`, `associated_equality_normalizes_a_generic_method_result` |
| Source-aware failures | Driver snapshots `renders_generic_method_specialization_without_color`, `renders_generic_variable_escape_without_color`, `renders_lambda_specializing_an_enclosing_generic_without_color` contain actual Alder source and labeled diagnostics |
| Owned interfaces | `stored_method_contract_preserves_independent_universals` serializes a producer, drops it, then deserializes into a fresh consumer; overridden and default methods are independently called at Number and String, alongside an ordinary generic identity |
| Executable cross-module calls | `tests/e2e/traits` imports `function_instances`; four assertions execute overridden/default `Select` methods at distinct payload types |

At the initial stored-interface checkpoint, the new test and executable
assertions passed. All twelve tests then selected by
`cargo test -p alder-solve generic_contract -- --quiet` passed.
Formatting and strict all-target/all-feature Clippy passed. The subsequent full
workspace unit/integration run passed, including 159 driver tests, 414 solver
integration tests, and the actual CLI fixtures. The separate workspace doctest
run also exited zero, with the same three pre-existing ignored documentation
examples (driver, parser, runtime). No snapshots changed or remained pending.

The original eleven-counterexample rerun is recorded in
`hardening-current-verification.md`. This coverage closes the missing direct
stored-method and cross-module execution evidence, not the entire optional
lifting, row-overlay, or recursive constraint integration audit.

## Explicit async boundary follow-up

`generic_contract_rejects_specialization_across_explicit_async_boundaries`
checks four invalid contracts: an overridden async trait method, an async
default with an early return, a plain universal function returning an async
block, and an async universal function returning another task. Each fails with
`GenericSpecialization`, not merely an unrelated parse or annotation error.
`generic_contract_preserves_universals_across_explicit_async_boundaries` accepts
independent Number/String method calls, a Bool default-method call, generic
capture in a lazy block, and exactly two Task layers for nested async.

The explicit_async CLI fixture now executes the same positive contracts in
`function_instances.ald`, called from its main module. No production change
was needed. Source review confirmed that implementation checking registers the
trait method's independent variables as well as the implementation annotation,
and that final validation checks specialization, representative uniqueness,
outer-environment escape, and tuple/error-row restrictions after deferred solving.
This closes the direct generic/explicit-async coverage gap; it does not establish
the still-open joint overlay/Option constraint entailment requirements.

Validation at this async-boundary checkpoint: all 14 solver unit tests,
454 solver integration tests, 14 CLI tests,
associated doctests, strict all-target/all-feature Clippy, and formatting pass.
No snapshots changed. Subsequent integrated validation covers 463 solver
integration tests and 17 CLI tests; fresh package verification includes these
additional test sources. See the `42c5423` checkpoint in
`release-packaging-hardening.md`. Clean committed-tree validation remains open.
