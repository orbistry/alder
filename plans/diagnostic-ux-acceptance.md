# Diagnostic UX acceptance review

This is the final requirement-by-requirement acceptance review.
The historical baseline and implementation checkpoints remain in
[diagnostic-ux.md](diagnostic-ux.md). General pattern policy is resolved and
implemented in `1a851af`; all completion gates below passed. Tests below are source-driven unless
explicitly described otherwise.

## Active path and reference boundary

`alder-solve/src/lib.rs` declares `inference`, `option_levels`, `pattern_matrix` and `traits` and
exports `inference::solve`. The driver calls that solver after canonicalization
and constraint generation, renders returned errors, and creates the solved
interface/codegen output only after success. The inactive Elm-port files do not
provide this behavior. Canonicalization calls the real local/module/import
warning collectors, rather than constructing an empty warning array.

Elm's `Type/Solve.hs`, `Type/Unify.hs`, `Type/Error.hs` and
`Reporting/Error/Type.hs` provide the recovery and contextual-type comparison
reference. Alder uses discarded fresh inference retries rather than Elm's
in-place Error descriptors. `Reporting/Warning.hs` is the warning comparison;
`Nitpick/PatternMatches.hs` and `Reporting/Error/Pattern.hs` supply the general
coverage/redundancy comparison. Alder's traits, mutation, async, Option lifting,
error rows and effectful pins require Alder-specific invariants.

## 1. Recovery and accumulation

| Requirement | Inspected implementation and executable evidence | Assessment |
| --- | --- | --- |
| Multiple independent errors in one module | `infer_recovering`; `independent_type_errors_accumulate_without_publishing` | Verified across declarations in Check/Build/Test |
| Independent errors within a callable | `recover_body_errors`, `BodyRecovery`; `independent_statement_errors_accumulate_without_dependent_cascades` | Verified at statement-sequence granularity, with pins/captures/shadowing controls |
| Partial unification, recursion, generic contracts | Entire Infer state discarded per retry; `recovery_discards_partial_unification_before_rechecking_shared_state`, `recovery_isolates_recursive_peers_and_deferred_generic_contracts`, method-contract regressions | Verified isolation; no catch-and-continue over corrupted substitutions |
| Deferred structures, mutation and async | Deferred/async recovery, sparse-tuple, overlay, method shared-state, and statement-recovery fixtures | Verified on adversarial source combinations; constraints/evidence from failed attempts do not survive |
| Trait evidence | Clean remainder resolved only after fresh inference; `core_recovery_also_reports_independent_trait_obligations`, invalid sibling-impl publication tests | Independent valid remainder checked; partially omitted bodies never supply evidence |
| Invalid declaration metadata | `declaration_annotation_errors`; six-error declaration fixture with indirect alias and importer | Verified accumulation before body inference and no dependent cascade |
| No invalid publication | Solver failure returns before solved interface/codegen; driver clears artifacts and interface files on any failed module before package-index creation | Cross-module, re-export, sibling-impl and CLI failed-build tests verify the boundary |

Recovery deliberately uses atomic declarations, methods and outer body statements.
It does not promise Elm-identical error counts inside a single nested expression.
An outside failure ends a body probe; ordinary declaration recovery then handles
it without using the incomplete body's contract. Trait resolution is not run on
partially omitted bodies because removing constraints can create misleading
ambiguity. Invalid metadata prevents body inference; recursive structural groups
retain a single-cycle diagnostic boundary. These are conservative diagnostic
granularity limits, not permission to publish partially checked programs or relax
type contracts. Retry cost grows with failing units times module inference work.

## 2. Structured context and comparisons

`alder-constrain::Error` owns structured comparison types and an optional
`Expectation` with a same-module origin. Core comparisons, generic restrictions,
infinite equations, missing-return types and associated equalities remain
structured until the driver localizes and renders them. Trait diagnostics retain
all arguments and obligation-chain types. Regions are not reinterpreted as
foreign-module source positions.

| Context/type requirement | Source evidence inspected |
| --- | --- |
| Annotation, argument ordinal/callee, condition, branch, array element, return | `mismatch_explains_expectations_at_the_source`, `call_arity_reports_counts_and_callee`, deferred optional-argument regression |
| Pattern, assignment, await, propagation and innermost cause | `nested_expectations_keep_the_innermost_cause`, async statement fixture |
| Origin labels and boundary scoping | Explicit return, lambda/async return-origin, missing-return annotation, associated-equality and transparent-alias regressions |
| Nested records and missing/extra fields | Nested record and record comparison fixtures; stored overlay regressions |
| Rows, functions, tuples, generic variables, Task/Result/Option | `mismatch_preserves_nested_types_and_open_rows`, distinct-variable and compound-comparison fixtures |
| Imported/re-exported identities and aliases | Nominal-name, re-export, stored nominal-identity and specialized-core-error fixtures |
| Complete trait goals without solver IDs | Distinct inferred/declared variables, all imported arguments and record-chain regressions |

Aliases are shown as readable expanded shapes with source annotation origins,
not reconstructed source-synonym syntax. Dense generic names preserve distinct
variables and structural relationships rather than exposing allocation IDs.
This satisfies readable comparison; it is not a source-code pretty-printer or a
copyable inferred-annotation feature.

## 3. Actionable explanations

The active reporter provides field candidates only from actual fields and only
for a uniquely nearest spelling; missing/extra fields are compared structurally.
Source tests inspect these hints, arity/callee explanations, branch context,
infinite equations and generic restrictions. Infinite-type advice does not claim
that an annotation can make a structural cycle finite. Generic-contract advice
explains caller-chosen universals and shared captures rather than recommending a
stronger implementation contract. Function equality advice does not recommend
implementing Eq for functions. Instance advice is conditional on ownership/head
rules; an annotation is not claimed to resolve overlapping concrete instances.

The final core-hint inspection reproduced one misleading recommendation in a
trait default body: an impossible error pattern suggested adding its tag to the
declared Result type. `impossible_error_pattern_advice_preserves_the_declared_contract`
now requires contract-preserving guidance, with a corrected-tag positive control.
The reporter directs attention to the spelling and permitted row instead.

## 4. Generated warnings and inferred-annotation assessment

Canonicalization generates unused imports, locals/parameters and private module
bindings from resolved references. Inspected fixtures cover exports, recursive
roots, initializer effects, types/traits/constructors, named/module/wildcard
imports, alternatives, rest/alias patterns, shadowing, captures, pins and writes.
Warnings are sorted by source. `_` is a discard and introduces no warned binding;
`SPEC.md` explicitly says `_name` is not an identifier, so no invented Rust-style
underscore-prefix convention is applied.

The reporter says to preserve needed initializers and to consider module
initialization/trait instances before deleting imports. The real CLI effect test
executes both import and local effects; editor tests receive warnings and clear
them after edits. These are pipeline-generated warnings, not renderer-only proof.

Elm's copyable inferred-annotation warning is assessed separately: Alder permits
inferred signatures, and public shared-state restrictions already receive actual
type errors where necessary. A blanket warning would create noise and impose a
new policy. It has not been adopted. A future opt-in annotation/code action would
need faithful syntax for generic bounds, rows and associated equalities, rather
than copying diagnostic display text. No such feature is claimed implemented.

## 5. Pattern policy: resolved and implemented

The user approved uniform exhaustiveness and redundancy checking using Elm's
Maranget-based approach, including irrefutable bindings and parameters.
`docs/language.md` records the rule. `Infer::check_patterns` now calls the shared
constructor-specializing matrix after type solving; the narrow Result collector
has been removed.

| Family / boundary | Implementation and source evidence |
| --- | --- |
| Enum/Option/Result payloads | Finite constructor families from local and imported metadata; missing nested Bool/literal payloads, restricted `Ok(0)`, recursive Tree and empty error-row controls |
| Tuples and records | Aligned product columns check combinations, not only individual fields; ordinary and constructor-record payload snapshots |
| Arrays/rest | Analysis-only nil/cons encoding checks exact lengths and unbounded prefixes; missing one-element and nested Bool payload witnesses |
| Alternatives and redundancy | Source-order usefulness with a reduced set of covering locations; duplicate alternatives, numeric signed zero and equivalent large decimal/hex BigInts |
| Parameters/destructuring/iteration | Irrefutability checks, including async/lambda/top-level rejection through the CLI; single-constructor/product/rest controls |
| Guards/pins and mutation | No unconditional coverage; mutable field/tuple/array refinements are forgotten across potentially effectful failures. Runtime fixture checks later arms becoming reachable, alternative guard retries, pin capture/evaluation and async cleanup |
| Markup match | Same inference adapter, with a positive exhaustive control; this does not implement markup code generation or later milestones |
| Recovery and delivery | Three independent pattern errors with no artifacts/interfaces; imported and serialized-interface enum tests; CLI Check/Build/Test and unsaved editor correction/clearing |

Witnesses are bounded to four representative missing patterns, not an exhaustive
enumeration. `_` denotes remaining possibilities for open/infinite spaces.
Guards/pins are not evaluated or proven pure, so effects involving mutable
structures can require a fallback even when a human could prove the guard pure.
This conservative analysis is intentional; accepted code retains its effects.

## 6. User experience and delivery

Eight real CLI/editor subprocess tests cover source-order error delivery,
dependency cascade suppression, warnings with effects retained, unsaved source,
stale versions, dependency invalidation, save/watched-file rechecks, UTF-16 ranges,
document closing and clearing diagnostics. The new statement test checks multiple
errors from one function in Check/Build/Test and clears them after an unsaved fix.
The pattern test adds missing/redundant source diagnostics and unsaved clearing.
Colorless snapshots and NO_COLOR assertions do not disable normal CLI color.

Positive controls, cross-module/stored-interface cases and adversarial
combinations remain in the ordinary workspace tests. Arena-owned AST/interface
boundaries and direct Oxc/Rolldown generation were not replaced by recovery code.
No runtime source maps, M5–M10 or provider checking are part of this work.

## Completion gates

- Implementation and source evidence above have been inspected, including limits.
- Formatting, strict all-target/all-feature Clippy, full workspace tests,
  snapshot-reference checks and all eight CLI/editor tests passed for this tree
  (256 driver tests, 491 inference tests; no pending/unreferenced snapshots; two existing ignored
  doctests).
- All 17 crates passed fresh extracted-archive verification using
  `/tmp/alder-pattern-package.yQxOg9`. Outside the checkout, that packaged binary
  reports three independent pattern errors in Check and Build (both exit 1)
  and leaves only source/configuration files. A valid mutable-record/failed-guard
  match executes the newly reachable arm, prints `42` and exits 0. Fresh targets
  avoid the stale same-version dependency artifacts identified in the historical
  package checkpoint.
- Sampo changesets accompany behavior changes; work remains on `diagnostic-ux`
  without merge or push.
- Pattern policy is resolved and the family matrix above is implemented and tested.
- Implementation commit: `1a851af` (uniform pattern coverage and redundancy),
  following the previously accepted recovery/context/warning work through
  `b28fb8c`. This final acceptance update changes documentation only.

## Final handoff

Restored behavior includes safe independent-error recovery, structured contextual
type comparisons and source origins, evidence-based explanations, actual unused
binding/import warnings, uniform pattern diagnostics, deterministic CLI/editor
delivery and stale-diagnostic clearing. Failed modules do not publish interfaces,
package indexes or executable artifacts.

Deliberate limits remain those documented above: recovery uses atomic nested
statements and does not solve trait obligations from partially omitted bodies;
recursive structural failures retain a cycle boundary; expanded aliases do not
reconstruct source synonyms; no blanket missing-annotation warning is enabled;
pattern witnesses are representative and effect analysis is conservative. These
are explicit language/design boundaries, not unimplemented completion tasks.

Work is committed on `diagnostic-ux`. No merge, push, provider checking, source
maps or later milestone implementation was performed.
