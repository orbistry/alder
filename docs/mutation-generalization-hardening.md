# Mutation and generalization acceptance evidence

## Current acceptance reconciliation

Requirement 2's value-restriction audit is complete for the integrated hardening
tree. This is an implementation review with finite regression evidence, not a
proof of general type soundness or final release approval. Historical open-audit
notes below describe earlier checkpoints.

The active path preserves shared variables through the entire boundary:

- Canonical assignment tracking uses resolved global identities, including
  write-only dependencies. SCC inference protects both the surrounding
  environment and restricted peers before any member is generalized.
- Free-variable traversal includes function arguments/results, constructor
  arguments and fixed higher-kinded slots, record/error tails and payloads,
  predicates, and associated projection equations. Scheme traversal also includes
  sparse tuple shapes and ordered overlay/error-row relationships.
- Generalization retains the connected deferred-constraint component before
  excluding protected variables. Restricted values quantify nothing. A closure
  factory is polymorphic; a stored result of calling it is not.
- Instantiation copies all quantified variables with one map across the value
  type and constraints, leaving unquantified identities shared. Unknown shapes
  stay constrained, rather than becoming independent unconstrained results.
- Error-row simplification protects visible/shared variables, universal
  contracts, overlay operands/results, tuple elements, and error payloads. It
  eliminates only hidden existential source tails. Final joint solving precedes
  universal specialization/escape checks and public-export validation.
- Export uses one type-variable name map for all relationships. Owned storage,
  hydration, and arena copying preserve them; import recreates their identities
  with one variable map. An unresolved monomorphic export is rejected.

Three additional solver tests combine tuple assignment, an independently open
record spread, and `?` propagation of an error carrying an array. They reject
String use of captured Number state, accept independently allocated Number and
String state, and verify that an uncalled export actually retains all three
constraint families. The driver test
`stored_joint_constraints_preserve_shared_error_payloads` then serializes and
drops that producer, checks independent result/untouched-slot types in a fresh
consumer, and rejects an incompatible captured payload. Failure publishes
neither interfaces nor artifacts. Its colorless diagnostic snapshot contains the
consumer source and the Number/String mismatch at the return contract.

The actual CLI records fixture imports `joint_constraints.ald`, propagates the
captured array through `?`, mutates it through the returned error, and verifies
the module's shared binding and the caller's tuple observe the same mutation.
It also checks an independent String-valued final overwrite and an untouched
tuple slot. The standalone end-to-end suite passes; no compiler changes were
needed for these probes.

These checks supplement the array/map/set/record/cell/task, recursive capture,
assignment, alias, re-export, and stored-interface cases mapped below. Mutation
syntax cleanup is reconciled in `plans/mutation-syntax-hardening.md`; the wider
dictionary/codegen audit and final clean-tree/package gates remain separate.

Validation for this reconciliation: full workspace tests/doctests pass (including
193 driver, 69 kernel, 17 CLI, 465 general inference, 13 local-annotation, and nine
mutation-generalization tests). Strict all-target/all-feature Clippy, formatting,
and whitespace checks pass. The new snapshot is reviewed and no pending snapshot
files remain. This verifies the integrated worktree, not an isolated checkout of
the focused test/documentation commit.

## Recursive capture follow-up

The current integrated tree adds six focused regressions in
`crates/alder-solve/tests/mutation_generalization.rs`. A closure returned by a
factory cannot re-generalize its hidden array through a variable alias; the
async-block version retains the same restriction. Mutually recursive functions
cannot re-generalize a captured array. A record containing both an array and a
lambda referring back to its reader tests a restricted allocation in the same
recursive group as a function. Each negative reaches solving and requires a
type mismatch, rather than merely accepting any parse or canonicalization error.
Positive cases preserve independent closure allocations and independent argument
types in recursive functions that also return a concrete shared array.

Source review reconfirmed that `infer_module` collects restricted SCC members'
free variables before generalizing any member, and `generalizable_item` does not
treat a call returning a closure as a fresh polymorphic function declaration.
`generalize_global` closes over deferred tuple/overlay/error-row relationships
before subtracting protected environment variables; `scheme_free_vars` traverses
those relationships in already-published schemes. `instantiate_scheme` uses one
replacement map and replaces only quantified variables. These are the active
owned-type mechanisms, not the inactive Elm-port solver's rank machinery.
The local Elm `Type/Solve.hs` reference similarly separates environment sharing
from freshly copied generalizable variables, but its immutable-language
generalization policy is not sufficient for Alder's mutable captures.

All six cases pass without production changes. Full workspace tests/doctests,
strict all-target/all-feature Clippy, formatting, and whitespace checks pass;
there are no pending snapshot files. They extend the evidence below;
they do not by themselves close the remaining joint deferred-constraint audit
or the final committed-tree/package gates.

## Earlier acceptance checkpoint

This describes `42c5423` plus the integrated hardening worktree, not an isolated
validation of that commit or a proof of general compiler soundness. There is no
mutation permission keyword or borrow system;
assignable bindings and shared JavaScript objects retain their mutation behavior.

## Selected restriction

The solver does not equate an unassigned binding with immutable contents.
`generalizable_item` permits function declarations and a conservative set of
non-expansive let initializers: lambdas, variable/constructor references, and
primitive literals. Collection allocations, calls, and task values are not
generalized merely because their binding is never reassigned. A function that
allocates a fresh collection on each call can still be polymorphic.

Resolved assignment metadata prevents generalizing replaceable global bindings.
Canonical dependency analysis includes assignment targets, so write-only edges
participate in recursive groups. Before generalizing a group, the solver
subtracts free variables from the enclosing environment and from restricted
members of the same group. A sibling function therefore cannot quantify away
the element type of a shared collection or replaceable function it captures.

`scheme_free_vars` includes predicate arguments, associated projection equations,
error-row inclusions, record overlays, and sparse tuple shapes, not only the
visible function type. `generalize_global` retains connected constraints and
subtracts environment variables before choosing quantified variables.
Instantiation freshens only the quantified variables and carries the constraints
along. An unresolved monomorphic public export is rejected instead of publishing
an apparently universal contract.

This restriction is deliberately conservative: local let bindings remain
monomorphic, and an arbitrary call producing a safe-looking value does not gain
polymorphism by inspection of its result. It preserves shared-reference semantics
rather than inserting copies or imposing mutation permissions.

## Coverage

Assignment-specific acceptance reconciliation at `42c5423` plus the integrated
tree: canonical `record_assignment` records resolved global binding identities,
not textual names. The SCC inference pass includes assigned and expansive
members' free variables before generalization. `generalize_global` subtracts
those protected variables after gathering connected constraints; instantiation
freshens only explicitly quantified variables. These source checks match the
negative captured/reassigned tests and positive independent-function cases.

Four canonical assignment tests, three reassigned-function tests, 28 solver
tests selected by `shared`, and the stored assignment and stored re-export
tests all pass in a fresh focused run. The 28-test selection includes unrelated
shared-row checks; it is not a claim of 28 distinct mutation regressions.
The two stored tests were inspected to verify that they serialize/drop/reload
producer interfaces, accept safe polymorphic uses, and reject incompatible
captured-state consumers without publishing interfaces or artifacts.

This closes the assignment-specific audit item, not the separately tracked
joint deferred-constraint/generalization audit or final committed-tree gates.

| Boundary | Permanent evidence |
| --- | --- |
| Shared allocations | Solver `shared_array_cannot_be_instantiated_at_incompatible_types`, `shared_map_cannot_be_instantiated_at_incompatible_types`, `shared_set_keeps_its_element_type` |
| Aliases and nested records | `shared_alias_cannot_regeneralize_state`, `shared_record_keeps_nested_state_monomorphic`, `function_cannot_generalize_captured_shared_state` |
| Replaceable function values | `reassigned_function_cannot_be_instantiated_at_incompatible_types`, `captured_reassigned_function_cannot_escape_through_generalization`, `reassigned_function_preserves_valid_monomorphic_calls` |
| Reusable tasks and cells | `shared_task_cannot_regeneralize_captured_state`, `ref_reusable_allocation_task_keeps_shared_array_payload_monomorphic`, `ref_captured_function_cell_cannot_be_specialized_through_an_alias` |
| Constraint-only relationships | `sparse_tuple_constraints_do_not_generalize_captured_arrays`, `associative_record_overlays_do_not_generalize_captured_shared_arrays`, `overlay_factory_cannot_regeneralize_captured_mutable_payloads` |
| Safe polymorphism and exports | `array_factories_remain_polymorphic`, `exported_shared_state_requires_a_determined_type`, `exported_closure_cannot_hide_an_unresolved_shared_type`, `exported_shared_state_can_have_a_concrete_type` |
| Stored interfaces | Driver `stored_assignment_contracts_preserve_safe_and_restricted_functions`, `stored_async_contracts_preserve_layers_and_captured_state`, `stored_sparse_tuple_capture_keeps_shared_payload_monomorphic` |

The new sparse-tuple stored-interface test compiles a producer that assigns a
captured Number array into the first tuple slot, serializes it, drops the original
interface, and checks fresh consumers after deserialization. Untouched slots
remain independently usable as String and Bool. Pushing String into the captured
array is rejected, and the invalid consumer publishes no interface or artifact.
This passes without a production change.

Re-export acceptance: `stored_reexport_aliases_preserve_shared_state_and_safe_polymorphism`
checks a named-renaming facade followed by a wildcard facade. Its producer
exports a concrete shared Number array, an independent identity lambda, and a
variable alias of a function that calls a replaceable captured operation. After
serializing the final facade and dropping all producer interfaces, a consumer
can use identity at Number and String and call the restricted function at
Number. Separate consumers cannot push String into the shared array or call
the restricted function with String; both fail with a type mismatch and publish
no interface or artifact. This passes without a production change. Renaming
and owned storage therefore do not re-generalize these captured-state cases.

The associated implementation review confirmed that SCC generalization solves
pending initializers/lifts and row/tuple constraints before collecting outer
free variables, includes restricted members of the SCC in that set, and only
freshens quantified variables on instantiation. The final public-export check
rejects remaining free scheme variables after generic contract checking.
This supports the tested restriction; joint constraint entailment remains a
separate open audit, not a consequence of this finite regression set.

Validation after the re-export test: all 182 driver tests, 14 solver unit tests,
452 solver integration tests, associated doctests, strict all-target/all-feature
Clippy, and formatting passed. No production changes or new snapshots were
needed for this follow-up.

Remaining acceptance work includes the joint deferred-constraint and generic
evidence review, the syntax/documentation cleanup audit, and final committed-tree
validation. This evidence does not waive those requirements.
