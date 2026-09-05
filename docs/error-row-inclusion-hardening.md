# Error-row inclusion hardening

Status: core inclusion and exact-union regressions implemented; wider boundary
audit remains open. Equality unification and directional inclusion are separate
operations and need separate invariants.

Representation checkpoint: canonical `Annotation` now has
`error_row_inclusions`, each with source/target row types, a source region, and
an `exact_target` bit identifying a least-union target.
Owned schemes serialize the same metadata; interface format is version 3.
Arena interface copying and owned hydration preserve both endpoints. Solver
schemes retain connected inclusion constraints, instantiate them with the same
variable substitution as their function type, and publish them in annotations.
A driver test now obtains a real inferred inclusion and verifies binary
round-trip plus copying after arena destruction. A solver test verifies the
input-to-output relationship in both the defining function and a calling helper.
Imported annotation instantiation also loads endpoints with the function's
variable map. A driver regression accepts an imported two-source union under a
covering closed result and rejects a closed result omitting one source's tag.

Universal-contract checking follows directed inclusion edges through inferred
intermediate tails and rejects a path between independent universals in the
same contract. This fixes the direct-return and `?` counterexamples below,
without equating tails. The solver replays known lower bounds after inference
until substitutions stabilize, then resolves flexible source tails constrained
by closed upper bounds and checks again. It does not equate independent source
tails or choose one upper bound's extra tags. Multiple overlapping upper bounds
are covered by a regression. Lambdas now use the same directional return check
as named functions; their former equality check incorrectly identified a shared
source with the first lambda's wider return type.

Scheme-local simplification removes reflexive constraints and existential tails
that occur only in hidden source positions. It protects visible type variables,
predicate/projection variables, outer shared state, universals, payload variables,
and intermediate target tails. This removes the extra parameter in the existing
pipe/await snapshot without changing that snapshot or global substitutions.

The result row first created for `?` in an unannotated return is an exact union
target. Existing declared rows, including `Result[a]` shorthand, are not marked
exact. After lower-bound propagation stabilizes, an exact flexible tail closes
only when all its contributing sources are closed (or refer to the same target).
Independent open sources and universal tails prevent that closure. The exact
marker survives scheme instantiation, publication, binary storage, hydration,
and arena copying. Imported and local concrete unions now support exhaustive
matching without an added result annotation.

Unknown returned values and bare error variables contribute directional source
inclusions rather than being equated with the output. Equating them had wrongly
required an unannotated input to already contain the errors from preceding `?`
operations. A permanent positive regression demonstrates the independent input.

Final module solving also simplifies hidden existential source tails for local
lambdas, which do not undergo global generalization. It protects variables in
module binding types, predicates, projection equations, and trait obligations,
in addition to the universal/target/payload protections above.

This is not a claim of complete row soundness. More general cyclic unions,
existential elimination, recursive groups, higher-order interactions, aliases,
and contract entailment across all supported boundaries still require the wider
audit. Imported constraint diagnostic regions currently originate in the
definition; accurate consumer-site attribution needs follow-up.

Empty inclusion lists are omitted from Annotation's Debug output so unrelated
existing AST snapshots remain stable; nonempty lists are visible for review.

## Reproductions

Before the hardening changes, the active solver accepted both these universally
invalid definitions (both now reject):

```alder
fn forget(value: Result[Number, [:known | e]]) Result[Number, [:known | f]] {
    value
}
```

```alder
fn forget(value: Result[Number, [:known | e]]) Result[Number, [:known | f]] {
    let number = value?
    Ok(number)
}
```

`e` and `f` are independent universally quantified tails. Forwarding `e` does
not establish that its tags belong to arbitrary `f`.

Conversely, the new positive regression combining two independent sources
with tails `e` and `f` was rejected after concrete instantiation: the inferred
output retains an unrelated open tail instead of the union of source errors.
The test uses independently checked closed-row producer functions, avoiding a
separate call-argument row-equality mismatch from passing bare Err constructors.
It now passes with the covering closed return annotation, and the missing-tag
counterpart rejects.

Shared-tail widening from `[:known | e]` to `[:known | :extra | e]` already
passes and must stay valid. The original negative inferred-forwarding test was
weak evidence alone: it formerly rejected an unrelated open output. It is now
paired with positive and negative concrete-union cases.

## Root cause

The original `Infer::include_error_rows` compared common known payloads, then discarded the
source tail. An open target with no residual known source tags succeeds
unconditionally. Otherwise the target acquires known residual tags and a fresh
unrelated tail. Neither branch records the relationship between the source's
unknown errors and the target. Universal-contract checking cannot reject a
relationship that was never represented.

The earlier empty-residual equality fix is valid but does not solve this:
it avoids falsely specializing a universal variable when two rows are equal.
Inclusion must additionally preserve source-to-target dependencies.

## Required solution and acceptance work

- Retain inclusion constraints, or an equivalent explicit union representation,
  for unresolved tails. Known source tags must belong to the target with
  compatible payloads; every source-tail possibility must also be covered.
- Preserve valid widening. Do not fix this by simply equating whole rows or
  all source tails: two independent polymorphic inputs must remain independent.
- Check explicit universal contracts against their declared relationships;
  independently quantified `e` and `f` do not imply `e` is included in `f`.
- An inferred result must preserve the union of input errors. Resolve concrete
  instantiations precisely enough to accept a closed result covering that union
  and reject a closed result omitting either source's tags.
- Carry unresolved relationships through generalization, instantiation, SCCs,
  owned interfaces, serialization, and imported calls. A local deferred check
  that disappears when a scheme is published is not sufficient.
- Apply the same rules to direct returns, early returns, `?`, async forwarding,
  and aliases. Preserve tag-payload and occurs checks and source locations.
- Cover declaration-only rejection, valid shared-tail widening, multiple-source
  unions, concrete narrowing, and cross-module execution/negative diagnostics.

Focused tests are in `alder-solve/tests/inference.rs` under the
`error_row_inclusion` prefix. The annotation-free exhaustive union and local
lambda cases first failed with `NonExhaustiveErrorMatch { missing: [], open:
true }` and now pass. The explicit-open-contract negative still rejects.
Driver tests cover imported exhaustive matching and preservation of the exact
marker through stored interfaces. The CLI errors fixture executes an imported
union on its left failure, right failure, and successful sum paths without a
wildcard error arm.

Checkpoint validation: full workspace tests pass (186 solver integration tests,
92 driver tests, plus existing runtime and CLI fixtures); strict all-target,
all-feature Clippy passes. The updated CLI fixture passes its execution test.
No expected snapshots changed or pending snapshots remain.
