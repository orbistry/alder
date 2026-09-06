# Record-row acceptance map

Current implementation and regression evidence, not a proof of general
soundness or final release approval. Incremental changes and related constraint
work are tracked in `plans/optional-arguments-hardening.md` and
`plans/record-overlay-hardening.md`.

## Representation and unification

The active solver's `Ty::Record` contains a name-sorted map of ordinary field
types and an optional tail term. `VariableKind::RecordRow` and the `RecordRow`
wrapper distinguish residual record fragments from value types and error rows.
`bind` checks kinds and occurs before storing a substitution. `occurs`,
`free_vars`, and `replace_vars` visit both field types and tails.

`unify_records` checks common fields and rejects residual fields against closed
rows. Two independent open tails share a fresh residual tail; an already-shared
tail cannot absorb contradictory residual fields. `prune` exposes solved row
fragments, and `from_ast`/`to_ast` preserve named tail relationships across
generalization and instantiation. Coherence and instance selection use the same
ordinary field-type semantics, not contextual lifting or presence subtyping.

Relevant source tests in `crates/alder-solve/tests/inference.rs`:

- `record_rows_accumulate_fields_independently_of_access_order` checks both
  x/y access orders and a concrete sum call.
- `record_rows_instantiate_independently_and_retain_extra_fields` preserves
  different extra fields in independent calls.
- `shared_record_tails_reject_incompatible_extra_fields` rejects conflicting
  residual payloads.
- `record_rows_reject_direct_and_mutual_payload_cycles` checks InfiniteType for
  direct and two-record assignment cycles.
- `record_rows_preserve_input_output_relationships_through_spread` and
  `record_rows_do_not_allow_fabricating_a_promised_tail` cover preserved versus
  invented tails.

## Construction and runtime

Optional shorthand is ordinary Option. Canonical, inference, copied, and stored
record fields have no separate presence flag. Fresh contextual construction
supplies None and minimum-Some field lifting; existing aliases and assignments
use ordinary types. Late context is checked before generalization, with closed
inputs closing their result and open spreads retaining checked overlay relations.
Defaults precede source fields/spreads in both the type-level overlay and direct
AST emission, so supplied values—including None—overwrite earlier defaults.

The `records` CLI fixture executes inferred sums, correlated tails, aliases,
assignment, patterns, and composed spreads across modules. `record_options`
executes shorthand equivalence, late callback/branch context, source alias
non-mutation, and cross-module open-spread defaults. It distinguishes outer None
from Some(None), tests actual derived operations/JSON, and observes exactly-once
left-to-right initializer/callback effects.

## Stored boundaries

Driver `independent_record_tails_survive_serialization_and_rehydration` checks
distinct input tails and their corresponding outputs. The stored generic and
associative overlay consumer tests check independent uses and incompatible
overwrites after serialization and arena copying. The new
`stored_open_spread_defaults_accept_absence_and_check_overwrites` verifies that
default operands survive the same boundary while wrong payloads still fail.
Interface format 6 stores ordinary field types without a legacy optional flag.

## Overlay audit and remaining delivery

The trait-scheme boundary now has direct negative and positive checks:
`trait_overlay_contracts_reject_unknown_right_hand_overwrites` rejects an
unsupported Number promise in both a trait default and an override with
GenericSpecialization. `trait_overlay_contracts_accept_a_guaranteed_final_overwrite`
accepts a final explicit Number overwrite for both method forms and independent
call-site row instantiations. No compiler change was needed for these cases.
`stored_trait_overlay_methods_preserve_independent_row_arguments` preserves
those accepted contracts through producer serialization/destruction and fresh
consumer checking. The traits CLI fixture calls default and overridden methods
across modules and checks that spreading/overwriting leaves input records
unchanged. The packaged CLI executes those assertions successfully.

The overlay plan now records the source review of all three joint criteria:
retention through schemes/interfaces, universal field promises, and cyclic
expansion versus payload occurs checks. The review uses the implementation and
named source/stored/runtime regressions, not a claim that a finite suite proves
general soundness or complete constraint satisfiability. Broader generic/evidence
integration, final original-counterexample reruns, packaged CLI checks,
documentation/changeset reconciliation, and clean commits remain goal-wide gates.
