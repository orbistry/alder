# Diagnostic UX acceptance review

This is the current requirement-by-requirement review, not a completion claim.
The historical baseline and implementation checkpoints remain in
[diagnostic-ux.md](diagnostic-ux.md). General pattern policy and final handoff
remain outstanding. Tests below are source-driven unless
explicitly described otherwise.

## Active path and reference boundary

`alder-solve/src/lib.rs` declares `inference`, `option_levels` and `traits` and
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

## 5. Pattern policy: decision still required

The detailed family matrix is in the pattern-policy checkpoint in
`diagnostic-ux.md`. Ordinary enums/Option, tuples, arrays/rest, records and
refutable bindings do not have Elm's general recursive coverage/usefulness pass.
Result has outer constructor/error-tag coverage and open-row catch-all checks.
Guards and nested pins do not count as unconditional coverage; alternatives and
fallbacks are covered by source tests, including an effectful pin.

Language documentation does not settle general partial/refutable matching or
redundancy policy, including restricted payload patterns inside Result arms.
Existing acceptance has been preserved except the explicitly requested nested-pin
correction. A user decision is needed before imposing broader rejection or a new
blanket warning. This is unresolved policy, not verified full pattern parity.

## 6. User experience and delivery

Seven real CLI/editor subprocess tests cover source-order error delivery,
dependency cascade suppression, warnings with effects retained, unsaved source,
stale versions, dependency invalidation, save/watched-file rechecks, UTF-16 ranges,
document closing and clearing diagnostics. The new statement test checks multiple
errors from one function in Check/Build/Test and clears them after an unsaved fix.
Colorless snapshots and NO_COLOR assertions do not disable normal CLI color.

Positive controls, cross-module/stored-interface cases and adversarial
combinations remain in the ordinary workspace tests. Arena-owned AST/interface
boundaries and direct Oxc/Rolldown generation were not replaced by recovery code.
No runtime source maps, M5–M10 or provider checking are part of this work.

## Completion gates

- Implementation and source evidence above have been inspected, including limits.
- Formatting, strict all-target/all-feature Clippy, full workspace tests,
  snapshot-reference checks and all seven CLI/editor tests passed for this tree
  (253 driver tests; no pending/unreferenced snapshots; two existing ignored
  doctests).
- All 17 crates passed fresh extracted-archive verification using
  `/tmp/alder-final-diagnostic-package.GhOMC0`. Behavioral checks on that binary
  confirm the nested-pin rejection and contract-safe hint; a positive fallback
  executes an effectful pin exactly once and returns the fallback result. Failed
  projects retain only source/configuration files. Reusing the earlier package
  directory retained stale same-version dependencies despite Cargo success, so
  that cached run is explicitly excluded from the evidence.
- Sampo changesets accompany behavior changes; work remains on `diagnostic-ux`
  without merge or push.
- Pattern policy must be resolved before marking its matrix row complete.
- Final handoff must identify current commits, validated gates and deliberate
  limitations, and leave a clean committed branch. This review does not mark the
  whole goal complete while those remaining gates are open.
