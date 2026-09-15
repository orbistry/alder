# Compiler hardening

Status: hardening accepted on `compiler-hardening`, based on `21994e0`.
Final requirement map, validation and limits: `docs/compiler-hardening-final-report.md`.

## Current integration status

The remaining cross-layer compiler/runtime changes and their regressions are
committed in `556a21c`, after the Coalesce checkpoint `c9bb0b6` and dictionary
audit `d63bd86`. All numbered and related-contract audits below are reconciled.
The final acceptance checklist below is reconciled: clean-branch packaging,
original probes, examples/integration fixtures, snapshot references, source
contracts and release metadata have been verified. The completion report records
the exact evidence and deliberate limits; no merge or publication is authorized.

The chronological notes below describe earlier worktree states; statements
about pending iterator decisions, missing defaults, uncommitted production
changes, and open contract audits are historical, not current blockers.

The integration's whitespace review found two colorless miette snapshots with
renderer-produced indentation on a blank help line, and one empty-JS snapshot
whose payload is a blank line. These are intentional exact output, not source
whitespace to trim or snapshot changes to accept silently. Non-snapshot source
passes whitespace checks.

## Historical implementation checkpoints

Source/deferred-boundary audit is reconciled in `docs/source-boundaries-hardening.md`.
Four new driver tests verify type-only table/schema publication, no callable macro
publication, rejection of runtime table/macro use, and exact Unicode diagnostic
byte spans across Check/Build/Test boundaries. A flaky timestamp-based cache test
fixture was replaced with exclusive, atomic-counter directories; a concurrent
64-directory regression passes. Full tests/doctests (199 driver, 72 kernel,
17 CLI, 69 codegen), strict Clippy, formatting and whitespace checks pass, with
no pending snapshots. Generic dictionary and evaluation-order audits remain,
alongside final original-probe/package/clean-commit acceptance gates.

Evaluation-order review found a pattern-capture type-safety defect: a later pin
could replace an earlier matched Some field with None before binding extraction.
The CLI assertion reproduced it. Per-pattern reached-path captures now preserve
the checked payload through later mutation and suspension, while retaining object
alias identity and source-position array-rest copies. Direct-AST snapshots and
compiled regressions pass; see `docs/pattern-capture-hardening.md`. Strict Clippy
and full workspace tests/doctests pass (69 codegen, 194 driver, 72 kernel,
17 CLI); no snapshots are pending. These edits remain uncommitted
with the pending imported-default and dictionary-initialization integration.

Array iteration now uses the approved separate cursor design: `array.iter`
returns `ArrayIterator[a]`, whose Iterator instance advances independently of
the array and stays exhausted after None. Native JS live-iterator semantics
preserve unread mutations and shared elements without copying. Kernel and
imported CLI regressions cover progress, independent/shared cursors, nested
Option/unit payloads, live mutation and permanent exhaustion; solver tests
preserve source-payload monomorphism and independent factory polymorphism.
Full tests/doctests (72 kernel, 194 driver, 17 CLI), strict Clippy, formatting,
and whitespace checks pass; all 17 packaged copies match and no snapshots are
pending. See `plans/array-iterator-hardening.md`. Broader dictionary/source
audits, pending integration commits, and final release packaging remain open.

Superclass initialization is now reproduced and fixed: the CLI failed on a
later derived Show dictionary with a temporal-dead-zone ReferenceError.
Dependency-ordered dictionary emission replaces Eq-only prioritization and
finishes dictionary construction before source value initializers. The new
CLI fixture covers derived/manual later superclasses, factory arguments, and
top-level method capture/calls. Full workspace tests/doctests and strict Clippy
pass, including the imported-default stored-interface test (194 driver tests).
Ten declaration-order snapshots were reviewed and updated; none are pending.
See `docs/dictionary-lowering-hardening.md`. These production edits remain
uncommitted; packaging, the iterator implementation, and broader audits remain.

The user approved separate iterator values for the Array progress defect.
Advancing an iterator leaves source elements intact; independent iterators own
independent progress. Implementation and regression coverage remain pending in
`plans/array-iterator-hardening.md`; this supersedes the decision request below.
The previous imported-default full-test and Clippy sessions are no longer
available to poll, so their terminal results are unverified and these gates must
be rerun before claiming integrated validation for that fix.

Dictionary audit found and fixed omitted defaults for implementations of imported
traits. Canonical inherited-method metadata now reaches direct Oxc lowering;
the helper import forwards self, method dictionaries, and source arguments.
Published trait helper symbols and foreign-implementation method lists are
corrected; interface format 7 invalidates stale metadata. The dedicated
explicit_async CLI test reproduced undefined.bind before the fix and passes
afterward, including singleton/factory defaults and captured async evidence.
See `docs/dictionary-lowering-hardening.md`. Review of a stored consumer artifact
also exposed potentially premature superclass initialization from a later
derived Show dictionary; runtime reproduction and dependency-order review remain.

Stdlib audit found a remaining Iterator defect: repeated `next` calls on the
same nonempty Array never advance or exhaust. A bounded kernel probe returns
`[7, 7, 7, 7]` from `[7, 8]`. The user was asked to choose a separate iterator
value (recommended) or destructive array consumption; see
`plans/array-iterator-hardening.md`. This keeps the stdlib audit open. Independent
inventory checked all 50 extern targets and all 17 packaged stdlib copies.
New all-builtin linking, scalar runtime, and compiled CLI checks pass. Full
workspace tests/doctests (four bundler, 70 kernel, 193 driver, 17 CLI), strict
Clippy, formatting, and whitespace checks pass. No pending snapshots. This
test/docs checkpoint makes no production changes; the iterator decision and
broader hardening work remain open.

Mutation-generalization reconciliation now completes requirement 2's audit.
Three mixed-constraint solver regressions exercise tuple assignment, ordered
overlays, and error-row propagation together; an uncalled export retains all
three constraint families. A serialized/dropped producer preserves independent
result types while rejecting a changed shared payload in a fresh consumer.
The new colorless snapshot was reviewed and labels the consumer's Number/String
return-contract mismatch. Source-path reasoning and the full coverage map are in
`docs/mutation-generalization-hardening.md`. The broader dictionary/codegen,
stdlib/source, and final integrated release gates are still open.
The actual CLI records fixture also verifies caller-visible mutation through the
propagated error and tuple, plus a String-valued final overwrite. Full workspace
tests/doctests pass (193 driver, 69 kernel, 17 CLI, 465 inference, 13 annotation,
nine mutation tests); strict Clippy, formatting, and whitespace checks pass.
No pending snapshots. No production change was required by this follow-up.

Mutation-generalization follow-up: six new focused solver regressions cover
factory-created closures and aliases, async captures, mutually recursive readers,
a restricted record allocation in the same SCC as its reader, and positive
independent allocations/argument polymorphism. All pass without production
changes. Source review reconfirmed whole-SCC restriction collection before
generalization and shared quantified-only instantiation; see
`docs/mutation-generalization-hardening.md`. This extends acceptance evidence;
the joint deferred-constraint audit and final clean-tree/package gates remain.
Full workspace tests/doctests pass, including 192 driver, 69 kernel, 17 CLI,
465 inference, 13 local-annotation, and six new mutation-generalization tests.
Strict Clippy, formatting, and whitespace checks pass; no snapshots are pending.

Generic-contract audit reconciliation is complete for requirement 1: final
constraint/contract/evidence phase boundaries were rechecked, and higher-kinded
default bodies preserve inherited Functor evidence while rejecting Array
specialization. The current acceptance map is `docs/generic-contract-hardening.md`.
Full tests/doctests (192 driver, 69 kernel, 17 CLI, 465 inference + 13 local
annotation cases), strict Clippy, formatting, and whitespace checks pass. No
pending snapshots. The focused tests/docs checkpoint is committed; the larger
worktree still needs mutation-generalization, dictionary/codegen, stdlib/source
audits and final integrated release verification.

Default-method scope follow-up: a trait parameter absent from method signatures
was not included in the body annotation scope or universal contract. A default
body's unused `let unused: a = 42` reproduced incorrect acceptance. Seeding every
trait-head variable before signature checking and self/superclass evidence fixes
the issue; the negative and a positive nested-bound regression pass. Recursive
peer annotations and a serialized-interface fresh consumer also pass. Eleven
focused local-annotation tests now cover these boundaries. The wider generic
audit and final clean-tree/package gates remain required.
Full tests/doctests completed successfully (192 driver, 69 kernel, 17 CLI,
465 general inference and 11 local-annotation integration tests). Strict Clippy,
formatting, and whitespace checks pass. The focused production/test/docs/changeset
checkpoint is committed; stored-interface coverage remains in the integrated
driver worktree. No merge or push was performed.

Local annotation scope is now fixed, following the existing lambda annotation
rule. Canonicalization permits annotation variables; solving reuses enclosing
variables and keeps fresh local names annotation-local and monomorphic. A
canonical-only intermediate reproduced acceptance of an unused `a = 42` local;
sharing the solver scope rejects it with GenericSpecialization. Seven solver
regressions and the compiled Option-law fixture pass. The new colorless driver
snapshot was reviewed and shows the promised `a` versus required Number.
See `docs/generic-contract-hardening.md`. Older pending-scope notes below are
historical; broader generic/evidence and final delivery gates remain open.
Validation: full workspace tests/doctests passed with 191 driver, 69 kernel,
17 CLI, and 472 solver integration cases. The seven new solver cases were then
moved unchanged into `tests/local_annotations.rs` and rerun successfully;
strict Clippy and formatting pass after that move. Snapshot and staged patch
were reviewed. This is integrated-worktree verification; final packaging must
include this production change as well as unbounded/cycle changes.

Option audit reconciliation: active constructor/pattern, dictionary, container,
JSON, and higher-kinded paths are mapped in `docs/option-operation-acceptance.md`.
A bounded four-layer kernel matrix passes equality/hash, round-trip, map
composition, apply/flatMap/traverse identity checks; the compiled generic
Hash/Json counterpart also passes. No production change was needed.

Generic-scope follow-up discovered by that fixture: `fn laws(values: Array[a])`
cannot use `a` in a body-local let annotation. `expression.rs` canonicalizes that
annotation with an empty type-variable set and reports unbound-type-variable.
The Option fixture now expresses its round-trip contract in a generic helper
signature, preserving the same checked type relationship. The generic-signature
audit must reconcile local annotation scope with the language contract; this
observation is not dismissed as an Option bug or silently counted as resolved.

Cyclic-operation checkpoint: derived Show/Hash/Ord/JSON now implement the
approved marker/error behavior with active-path tracking and exception-safe
cleanup. Four kernel regressions fail before the fix and pass afterward; the
compiled generic enum/Array/Option Show/Hash/JSON fixture also passes. Details
and the completed source-level Ord/container acceptance are recorded in
`plans/cyclic-value-operations.md`. No codegen ABI change was required.

Cycle acceptance follow-up: compiled Ord now covers user-defined dictionary
delegation, nested Options, and successful comparison after removing a cycle.
Mutual record/positional enum cycles through Array/Result cover Show/Hash/JSON;
raw structural Show/Hash cover mutation and shared children. All focused probes,
full workspace tests/doctests, formatting, and strict Clippy pass. The new
semantics/verification map is `docs/cyclic-values.md`. The broader Option/law,
generic/evidence, source-fidelity, and final integration/package gates remain.

Current implementation checkpoint: `fiber.unbounded` is now exposed through
the stdlib (and packaged copy), canonical typed-value lookup, kernel constant,
and bundle export. Its CLI regression reproduced unknown-name before the fix
and passes after it, testing gated concurrent starts, ordered results, and all
four public traversal APIs. Canonical coverage verifies a monomorphic Number;
kernel options coverage uses the new constant across all adapters. Formatting
and strict Clippy pass. Full `cargo test --quiet` completed with exit 0, including
85 canonicalizer, 190 driver, 63 kernel, 17 CLI, and 465 solver integration tests;
doctests pass with two ignored. No pending snapshots or whitespace errors.
Production changed, so the prior package checkpoint
below is historical and must be refreshed for final delivery.

User decision follow-up: both pending choices are approved. `fiber.unbounded`
is a concurrency-limit value used in `{ concurrency: fiber.unbounded }`.
Cyclic values retain cycle-aware structural Eq; Show emits a cycle marker;
derived Hash, Ord, and JSON encoding report explicit runtime errors on active
cycles, preserving existing acyclic behavior. See the concurrency and cyclic
value-operation plans. Implementation and validation remain required; older
checkpoint references to these choices being pending are historical.

Local documentation checkpoint `e2ac520` records the module/cache, re-export,
record-row, and inactive-cache-helper acceptance maps. Only four documentation
files were committed. Fresh full workspace tests/doctests, strict Clippy,
formatting, and whitespace checks pass, with no pending snapshots. The larger
hardening implementation and the remaining generic/evidence, Option/stdlib,
codegen/deferred-source, approved-policy implementation, and final clean-tree gates remain open.
No production sources changed, so the verified `6b4af4c` package contents remain
current for compiler/runtime code. Nothing was merged or pushed.

Fresh package verification completed in exec session `99378` with exit 0, using
the distinct target `/tmp/alder-package-current.4UcAo6` after both cache fixes.
All 17 crates verify and 1,511 extracted source files match. Cargo.lock SHA-256 is
`454f3c8f1a6956c18765d79d0ff62fcc5706776b1b0eafaba783491f2e1d9f0e`.
Fresh fixtures at `/tmp/alder-packaged-fixtures.HrnWiB` pass all four examples,
seven integration projects, both test-exit fixtures, and the original review
probes' expected results. Packaged check writes the new cache path, and the
extracted kernel passes the original fairness probe. Runner `92267` is terminal
with exit 0; no processes remain active. Exact evidence is in the release doc.
This is integrated dirty-tree validation, not final clean-commit approval.

Interface-only cache consistency is now a confirmed/fixed defect: two saved-file
regressions failed because the loader accepted an index missing a current impl
and an index retaining a deleted impl. All files had valid independently computed
fingerprints. The loader now compares sorted full implementation headers for
each module before returning dependencies. The positive loader fixture uses a
real checked trait/impl rather than only empty metadata. Both negative tests
pass after the fix. Local commit `6b4af4c` contains the loader check, fixtures,
documentation, and changeset only. Full integrated tests/doctests (190 driver,
465 solver integration, 63 kernel, 17 CLI), strict Clippy, formatting, and
whitespace checks pass. Package refresh remains required after this driver
change. See `dependency-index-consistency.md`. No merge or push occurred.

Cache-loading source review after `9b47b90`: source-backed dependency loading
does not read saved headers; interface-only loading validates each file's format,
compiler version, fingerprint, and requested owner identity. Old timestamp
rebuild helpers have no active callers and are identified in the implementation
map. All 188 driver tests/doctests and three CLI dependency regressions pass.
One concrete remaining check is index/interface generation consistency: the
loader currently does not compare the index's implementation headers with those
in the complete set of module interfaces it loads. Reproduce a stale-but-valid
index beside newer valid interfaces before claiming this is a confirmed defect
or closing the cache audit. Individual fingerprint validation alone is not
evidence that those files describe the same package generation.

Confirmed cache-path collision: four distinct application/builtin/member module
identities mapped to only two interface paths; named `members/member` also
collided with an application member's package index. The new regression failed
before the fix. Cache paths now explicitly separate identity variants, the
unused unqualified lookup API is removed, and the CLI's cache-path assertion is
migrated. No compatibility reader is retained. Committed locally as `9b47b90`,
with only the cache implementation/tests, CLI assertion, docs, and changeset.
Full integrated workspace tests/doctests (188 driver, 465 solver integration,
63 kernel, 17 CLI), strict Clippy, formatting, and whitespace checks pass.
The prior package archives predate this change and require a later refresh.
Sampo records driver/CLI patches in `package-kind-cache-namespaces.md`.

Overlay source audit concludes the three explicit criteria in its subplan:
retention, universal promise checking, and cyclic expansion/payload validation.
The review maps ordered producer normalization, contract-check phase ordering,
active-path/visited guards, retained equations, selected-field checking, and
structural occurs checks to existing source/stored/runtime regressions. Fresh
48-test overlay integration/1-unit and 31-test stored-driver runs pass. This is
a completed bounded implementation audit, not a general soundness theorem or
waiver of broader generic/evidence integration and final clean-tree gates.
The record-row acceptance map is updated; older pending-review entries below
are historical. No production changes or snapshots in this checkpoint.

Recursive universal-overlay follow-up adds a rejected unknown-overwrite promise
and an accepted per-iteration overwrite with a shared universal residual tail.
The first positive candidates violated monomorphic recursive parameter contracts;
the corrected fixture passes without a compiler change. The overlay plan records
that distinction explicitly. Full workspace tests/doctests pass (465 solver
integration, 187 driver, 63 kernel, 17 CLI), as do strict Clippy, formatting, and
whitespace checks. No pending snapshots. These tests remain with the ongoing
overlay integration; general universal/cyclic review and release gates are open.

Overlay retention audit now has a source-to-regression map in
`plans/record-overlay-hardening.md`. The reviewed path covers pending relation
retention, connected generalization, protected variables, shared instantiation
maps, owned storage, arena copying, and consumer diagnostic sites. Focused runs
pass (46 overlay integration/1 unit, 11 independent-spread, 31 stored driver,
and the separate metadata round-trip test). No source or snapshots changed.
This closes that transport/retention review, not the still-open universal and
cyclic equation criteria or whole-goal gates.

Acceptance-documentation checkpoint committed locally as `21b4510`: async,
generic-contract, and mutation/generalization maps now explicitly describe the
integrated worktree rather than isolated HEAD. Stale package-refresh statements
are corrected, and historical test counts are labeled. Fresh workspace tests
and doctests, strict all-target/all-feature Clippy, formatting, and whitespace
checks pass (187 driver, 463 solver integration, 63 kernel, 17 CLI). No pending
snapshots were found. Only those three documentation files were committed;
the remaining hardening worktree is preserved. Unbounded spelling and cyclic
operation policy remain pending clarification of the user's brief confirmation.
No merge or push occurred; whole-goal acceptance remains open.

Package refresh completed successfully in `/tmp/alder-package-current.NlhYrd`:
all 17 crates verify and 1,511 extracted source files match. Session `75064` is
terminal with exit 0. The packaged CLI runs all four examples, seven integration
projects (including mutual overlays), original probes, and both test-exit
fixtures from cache-free copies in `/tmp/alder-packaged-fixtures.9ZkB8z`.
Original fairness and four new Promise harnesses pass against the extracted
kernel. All processes finished; exact evidence is in the release-packaging doc.
The branch-wide audit and clean committed-tree gates remain open.

The release refresh above followed the Promise waiter fix and new boundary tests.
Its initial in-progress checkpoint is superseded by the terminal result above;
session `75064` must not be treated as an active build.
The overlay plan now separates its existing unresolved-relation, universal-
promise, and cyclic-termination criteria, retaining their full scope and existing
source/stored/runtime evidence rather than conflating them into a blanket claim.

Mutual-overlay stored/runtime follow-up passes: the producer is serialized and
dropped, fresh valid consumers independently instantiate Number/String, and an
invalid empty-overwrite consumer rejects without publishing artifacts or an
interface. The reviewed colorless snapshot labels the imported consumer call.
The dedicated `record_options` CLI test executes both mutual entry functions
at zero and several steps. Full workspace tests/doctests, strict Clippy,
formatting, and whitespace checks pass (187 driver, 463 solver integration,
63 kernel, 17 CLI). No pending snapshots remain. These tests are part of the
pending overlay integration, not a general cyclic-entailment completion claim.

Mutual-recursive overlay follow-up adds paired solver regressions: an empty
overwrite rejects incompatible payload growth, while a required overwrite
permits independent Number/String calls across an even/odd SCC. Both pass;
the positive fixture's initial Bool/String seed mismatch was corrected without
changing the compiler. Full workspace tests/doctests, strict Clippy, formatting,
and whitespace checks pass (463 solver integration tests). These tests remain
with the pending overlay integration; no production change was needed. Their
stored/runtime counterparts and general symbolic/cyclic entailment remain open.

Joint-constraint phase-order follow-up: inspected the final solving loop,
generic-contract validation, subsequent error-match/tag checks, projection
normalization, and type application. No new defect was confirmed in this slice.
Fourteen generic-contract integration tests and the stored trait-overlay
independent-row test pass. Details and the associated-type elaboration caveat
are in `docs/generic-contract-hardening.md`. Symbolic/cyclic overlay entailment
remains required and unclosed. Pending unbounded-concurrency spelling and cyclic
value-operation policy were presented together for explicit user input; no
answer is assumed from automatic goal continuations.

Async lifecycle reconciliation closes the three specified related-audit items
below with source-to-test mappings in `docs/async-hardening-acceptance.md`.
Two missing focused checks for throwing rejection mappers and throwing then
getters pass without a production fix and are committed as `42c5423`. The full
integrated workspace test/doctest run, strict Clippy, formatting, whitespace,
and staged Rust parsing checks all pass (63 kernel tests). No production code
changed in this checkpoint. The named unbounded-concurrency API decision,
other compiler audits, dirty-tree integration, and final packaging remain open.

Promise waiter fix committed locally as `92beacb`, containing only the internal
guard, new late-mapper regression, strengthened/repositioned reentrant regression,
and changeset. Full integrated workspace tests/doctests finish successfully
(61 kernel, 17 CLI, 186 driver, 68 codegen, 461 solver integration); strict
Clippy, formatting, whitespace, and staged Rust parsing checks pass. The staged
patch was reviewed in full. This is integrated-tree validation, not an isolated
export of the partial commit. Package refresh and the broader acceptance audit
remain required. Nothing was pushed or merged.

Confirmed Promise cancellation follow-up: late rejection still ran the internal
mapper after interruption completed, because its side effects preceded the
existing guarded resume. A new regression failed before the fix. Suspension now
exposes active-waiter state to Promise registration; rejection skips mapping
after invalidation but remains observed. All 61 kernel tests pass after the
initial fix; the reentrant registration test is additionally expanded to check
mapper suppression. See the async acceptance map and
`cancelled-promise-mapper.md` changeset. Full validation and refreshed kernel
packaging remain required; the archives below predate this change.

Fresh package verification after `209141d` passes for all 17 publishable crates
in `/tmp/alder-package-current.9dUFHT`. All 1,510 extracted source files match
the integrated tree; the lockfile digest is unchanged. Fresh packaged CLI runs
from `/tmp` pass all four examples, seven integration projects, both test-mode
exit-status fixtures, and the original review probes with approved syntax
updates. The original host-fairness probe passes against the extracted kernel.
See `docs/release-packaging-hardening.md` for exact paths, commands, and limits.
All verification processes finished; this is not a clean-commit completion gate.

The nearby JSON audit adds `record_options/src/json_fields.ald`: prototype-
named fields retain payloads and an omitted optional field becomes None. The
first fixture used an unqualified constructor and was corrected to the existing
`Fields::Fields` syntax. Its dedicated `option_record_defaults_execute` test
and packaged CLI run pass; the earlier standalone multi-project test did not
include this fixture. No production fix was needed. Fixture integration and
acceptance notes remain part of the pending record/Option checkpoint.

Dependency-discovery checkpoint committed locally as `209141d`: transitive
source loading, canonical package-root collision checks and alias coalescing,
bounded cyclic discovery, and workspace-member import ownership. Full workspace
tests and doctests finished successfully, including 186 driver tests; strict
workspace Clippy, formatting, staged Rust parsing, and staged whitespace checks
pass. This validates the integrated worktree, not an isolated checkout of this
partial checkpoint. Previous release archives predate this production change;
fresh packaging remains required. No push or merge was performed.

Confirmed workspace dependency-ownership defect: a member importing widgets
activated a sibling's unused widgets declaration and tried reading its missing
path. The regression failed before the fix. Discovery now groups source imports
by the most-specific owning member, shared with canonical identity resolution.
The regression passes in both member orders. Strict workspace Clippy and
full integrated validation pass for the combined discovery checkpoint.
Transitive loading, canonical-root collisions/aliases, and cycle checks remain
covered by the expanded CLI regression. Fresh packaging is still required.

Dependency-cycle check: the cold/transitive CLI fixture now closes the package
graph with core -> widgets and the module graph with core/api ->
widgets/instances -> core/api. Compilation finishes within the 30-second bound
and produces the normal import-cycle error, not repeated discovery or a missing
interface. The focused regression passes without another production change.
Workspace member-specific import ownership and fresh packaging remain for the
uncommitted discovery checkpoint.

Dependency-root collision follow-up: the new cold/transitive regression
confirmed that app and dependency manifests could select distinct roots for
`vendor/core`, with the first-discovered one silently winning. Discovery now
tracks package -> canonical root, seeds named workspace members, and rejects
conflicts with both sorted paths in a dedicated driver diagnostic. Equivalent
`core` and `core/../core` references are accepted and emit one core/api artifact.
The original collision regression failed before the fix and passes afterward.
This is part of the uncommitted transitive-discovery checkpoint; graph-cycle,
workspace import ownership, and final package checks remain to review.

Confirmed transitive source-dependency defect: app -> widgets -> core failed
because discovery scanned only app imports/configuration. The cold-dependency
regression failed with a missing core interface and then missing trait evidence.
Discovery now queues source projects, scans their imports with their own
manifest/path base, and shares the visited package set to bound traversal.
The app and both packages need no prebuilt semantic caches; widgets/core retain
distinct `api` module identities. The focused compile/bundle/execute regression
now passes. Dependency test dependencies remain excluded from this traversal.
All 185 driver tests, 17 CLI tests, associated doctests, and strict workspace
Clippy pass. Formatting and whitespace checks pass. Package refresh and
additional graph/root-conflict review remain required before committing this
discovery checkpoint. Changeset: `transitive-source-dependencies.md`.

Trait-overlay regression checkpoint committed as `a7e2a67` (four test/fixture
files only). The new inferred-merge helper cases complement existing direct-
spread default/method regressions; they are not the first trait-overlay checks.
The complete staged patch was reviewed and staged Rust files pass standalone
rustfmt parsing/checking. Full integrated workspace tests/doctests and strict
Clippy pass, including 461 solver integration, 185 driver, 17 CLI, 68 codegen,
and 60 kernel tests. This is integrated-tree validation, not an isolated build
of the partial checkpoint. Unrelated hardening edits are preserved; no runtime
or compiler production behavior changed in this commit.

Trait/overlay stored-runtime follow-up: the producer interface now has a
serialization/destruction/fresh-consumer regression for independent default
and override row arguments. A separate traits fixture module executes both
dictionaries cross-module and checks input non-mutation. The packaged CLI runs
the expanded fixture from `/tmp`. All 185 driver tests, 17 CLI tests, associated
doctests, strict workspace Clippy, formatting, and whitespace checks pass.
No production source changed; see the record-overlay plan for bounded scope.

Trait/overlay contract follow-up: defaults and overrides now have direct
regressions rejecting an unsupported Number return hidden by an unknown
right-hand overwrite, with GenericSpecialization required. A final explicit
Number overwrite is accepted in both forms at independent call-site row types.
No production change was needed. All 461 solver integration tests, 14 unit
tests, doctests, strict workspace Clippy, formatting, and whitespace checks
pass. See `plans/record-overlay-hardening.md`; these bounded cases do not close
the remaining symbolic/cyclic or stored/runtime trait-overlay checks.

Cold source-dependency acceptance: `source_dependency_builds_without_saved_interfaces`
creates a fresh package/app pair, verifies no dependency `.alder` directory
exists, and executes a trait implementation from an indirectly discovered module.
The build creates no dependency cache. This directly covers cold discovery
alongside the saved-body/removal regression from `cc5d41e`. No production change
was needed. All 17 CLI tests/doctests, strict workspace Clippy, formatting, and
whitespace checks pass. The isolated regression is committed as `490e5cb`;
validation used the integrated worktree. Existing package verification still
covers unchanged production code, but predates this test source.

Release refresh at `cc5d41e` plus the integrated hardening worktree: all 17
publishable crates package and verify successfully in the new, separate
`/tmp/alder-package-current.xhI60N` target. All 1,510 extracted source files
match current sources; Cargo.lock is unchanged. The packaged CLI passes all
four examples, seven integration projects, original review probes (with only
approved syntax updates), and passing/failing test-runner cases from fresh
cache-free copies launched from `/tmp`. The extracted kernel passes the original
10,000-ready-Promise host-fairness probe. See
`docs/release-packaging-hardening.md` for exact commands, outcomes, and limits.
This refresh does not resolve pending language choices or the final clean-tree
acceptance audit; subsequent production changes require refreshed evidence.

Confirmed source-dependency cache defect: removing an implementation after
saving its package index still let a consumer compile against stale evidence.
The CLI regression failed before the fix. Source-backed dependency discovery
now bypasses saved interfaces/indexes; interface-only loading stays separate.
The focused regression passes, including updated body execution and rejection
after implementation removal. Full integrated workspace tests/doctests pass
(184 driver, 16 CLI, 68 codegen, 60 kernel, 459 solver integration tests), as
do strict workspace Clippy, formatting, and whitespace checks. The final fixture
uses direct package imports and its focused test passes again. Committed the
isolated production fix, regression hunk, and changeset as `cc5d41e`; the rest
of the dirty hardening worktree is preserved. Validation is of the integrated
worktree, not an isolated export of the staged tree. Fresh packaging remains.
See `.sampo/changesets/source-dependency-cache-authority.md`.

Generated-code/cache ABI follow-up: source inspection confirms the CLI rebuilds
dependency AST artifacts and bundles the current embedded kernel rather than
loading saved JavaScript. The path-dependency CLI regression now changes a
dependency body while retaining its saved semantic interfaces and verifies the
updated behavior on a second build/run. The focused regression, formatting,
and strict workspace Clippy pass. See `docs/release-packaging-hardening.md`;
semantic cache validity and final release packaging remain separate gates.
Cycle-operation policy is still pending explicit confirmation.

New explicit record decision: the user confirmed optional-field shorthand is
exactly an Option-valued field, with omitted and explicit None equivalent.
This resolves the record-presence question; removing the old semantic distinction
across inference/interfaces/codegen/runtime is now required implementation work.
See `plans/hardening-language-decisions.md`. Public unbounded traversal spelling
is still separate and unanswered.

Recursive tuple follow-up: three inference regressions verify whole-SCC
projection collection in both declaration orders, exact caller arity, and
independent untouched element types in whole-tuple returns. Two bounded
cross-module calls execute successfully in the records fixture. No production
changes were needed; 436 solver integration tests and 14 unit tests pass.
See `plans/tuple-projection-hardening.md` for scope and remaining audit.

Tuple/Option interaction follow-up: both projection/lifting statement orders
preserve fixed tuple length and independent unobserved element types. New
negative probes reject wrong length, projected element, and returned element;
four cross-module calls execute with the freshly packaged CLI. All 433 solver
integration tests and 14 unit tests pass. No production changes were needed.
See `plans/tuple-projection-hardening.md`; joint acceptance remains open.

Fresh packaging checkpoint: all 17 publishable crates verify from the current
worktree in `/tmp/alder-package-current.VmQGic`. The packaged CLI runs all four
examples and the traits, explicit_async, control_flow, and successful
pattern_bindings fixtures from `/tmp`. Archived solver, kernel, and Fiber
sources match the worktree; Cargo.lock is unchanged. See
`docs/release-packaging-hardening.md`. Final clean-commit acceptance remains open.

Connected Option-depth review: added a sparse direct-match fast path and compared
small graphs against the dense solver. This exposed and fixed ambiguity taking
precedence over an impossible later component. A separate inferred error-row
lifting regression now preserves its row kind and passes actual CLI execution.
These changes remain in the pending joint solver checkpoint. Positive-slack
components still use dense closure. The failure-site follow-up reproduced labels
on unrelated valid arguments and now carries original edge provenance into
source diagnostics; two reviewed rendered snapshots and component tests cover
it. See `plans/optional-arguments-hardening.md` for the remaining joint audit.

Joint solver review: reproduced and fixed contextual Option lifting specializing
an explicitly universal relay parameter before its callers were checked. The
depth solver now treats universal inputs as opaque and inserts Some as needed.
Four solver regressions, a codegen snapshot, stored-interface validation, and
cross-module CLI execution cover the fix; full working-tree tests/doctests,
formatting, and strict Clippy pass. This remains in the pending joint solver
checkpoint. See `plans/optional-arguments-hardening.md`; the broader constraint
integration audit and final packaging remain open.

Coherence diagnostic checkpoint: validate the frozen package registry before
body compilation, retain errors on their defining source modules, and report
dependency-only errors once without fabricated spans. Resolve implementation
labels by identity, not source ordinals into canonical items. A newly reproduced
cross-module superclass-cycle label now selects a local trait declaration.
See `docs/coherence-diagnostics-hardening.md` for regressions and validation.

Explicit module-identity checkpoint: removed URI-based fallback identities and
metadata-free driver entry points. Both package and source-relative path are
required before graph/build publication. Missing metadata yields deterministic
build-level diagnostics without fabricated spans. Isolated staged-tree driver
validation passed all 124 tests; see `docs/module-identity-hardening.md`. This
does not close the separate package-coherence or final release-readiness audit.

Canonical pattern checkpoint review: match-only permission is now carried by
binding mode rather than leaked expression depth. Pins use pre-pattern lexical
scopes; alternatives share the first pattern's local IDs and must bind equal
name sets. Six canonicalization regressions cover missing/extra names, forbidden
pins in nested let/lambda bindings, valid nested matches, and self-pattern name
lookup. Source-aware snapshots were reviewed, including the two new tuple-based
alternative errors. Rendered alternative-binding diagnostics pass. This does
not close the related solver compatibility or codegen evaluation-order audit.
All 84 canonicalization tests and its doctests pass, as do formatting and strict
workspace Clippy. No pending snapshots remain. The prior full workspace pass
covers the same production source; this checkpoint additionally added two tests.

The eleven defects from the September 4 review are minimum acceptance scope.
The M2–M4 milestone checkmarks describe the feature work that landed, not proof
that the contracts below are sound. Further milestones remain out of scope.

Approved scope amendments: removal of `mut` (section 2) and the explicit async,
capture, synchronization, and bounded traversal decisions in
`plans/async-concurrency-hardening.md`. These supersede conflicting inferred-
async or mutation-permission assumptions; they do not waive original acceptance
requirements. Amendment implementation and remaining acceptance work are
tracked below and in the detailed plans.

Latest approved language choices are recorded in
`plans/hardening-language-decisions.md`: fixed tuple arity after collecting
projections, Result error rows/groups only, conditional structural error-row
capabilities, trailing optional parameters, and `?` propagation for Result and
Option only. These decisions supersede earlier pending-choice checkpoints;
read them before resuming implementation. They are approved scope, not completed
features.

Option propagation implementation and remaining acceptance coverage:
`plans/option-propagation-hardening.md`.

Record implementation overlap findings and remaining acceptance coverage:
`plans/record-coherence-hardening.md`.

Structural instance-selection audit: `plans/instance-selection-hardening.md`.

Current original-counterexample verification:
`docs/hardening-current-verification.md`. The rebuilt CLI produces the intended
results in fresh source-only project copies, with formatting checked across an
actual edit and the original timer probe rerun. This supersedes old failure
claims for those exact manifestations, not the still-open full acceptance audit.

## Working rules

- User-approved pre-1.0 policy: no backward compatibility obligations for
  syntax, APIs, runtime ABIs, interfaces, or caches. Remove superseded behavior
  cleanly and migrate repository consumers; do not add compatibility shims,
  legacy readers, deprecation paths, or migration-specific diagnostics.
  Treat removed syntax as though it never existed: ordinary parsing rules and
  errors apply. This supersedes earlier requests for helpful obsolete-syntax
  errors, including the former dedicated `mut` diagnostics.
- Reproduce before fixing; retain permanent regressions with actual Alder source.
- Preserve JS aliasing, accepted syntax, arena ownership, direct Oxc emission,
  owned interfaces, and shared miette diagnostics.
- Check related cases at each affected boundary; never accept a changed snapshot
  merely because it matches new output.
- Keep compiler, interface, runtime, and documentation contracts in agreement.
- Commit coherent checkpoints. No merge, push, publication, or tag changes.

## Work and acceptance matrix

### 1. Universal generic contracts

- [x] Regressions: a generic trait method returning a concrete Number must fail;
  valid generic identity implementations must continue to work.
- [x] Distinguish rigid contract variables from flexible inference variables;
  prevent specialization, identification of independent universals, and escape.
- [x] Audit explicit signatures, partial annotations, defaults, impls, HKT,
  associated equalities, bounds, recursive groups, and cross-module interfaces.
- [x] Source-aware diagnostics, compilation rejection of invalid contracts,
  and positive cross-module execution tests (invalid programs must not execute).

Current evidence: `docs/generic-contract-hardening.md` maps the contract check,
phase boundary, and regression tests. Deferred universal validation enforces
rigidity without a separate rigid Ty variant. A fresh stored-interface test and
CLI assertions cover independently instantiated overridden/default methods.
The current reconciliation in that document includes the completed row/overlay
review and the local/default annotation scope fixes. Thirteen focused annotation
cases cover the new boundaries, including higher-kinded trait parameters absent
from method signatures. Broader dictionary lowering, mutation-generalization,
source-fidelity, and final integration/package gates remain open; this is not a
claim of exhaustive type-system soundness.

Root cause: implementation checking instantiates its expected signature with
flexible variables and unifies away the method's universal contract.

### 2. Mutation and polymorphism

Current restriction and regression map:
`docs/mutation-generalization-hardening.md`. A new source-produced sparse-tuple
interface regression preserves captured Number arrays through serialization,
rejects String mutation, and independently instantiates untouched tuple slots.
The check passes without production changes.

Detailed migration analysis: `plans/mutation-syntax-hardening.md`. Baseline
direct/captured replaceable-function regressions pass under current syntax.
Resolved assignment metadata now replaces the keyword's generalization role;
syntax removal is implemented; further capture and generalization audits remain.
Ordinary local/top-level lets and named/lambda parameters now permit writes;
the replacement-safety regressions also run without `mut`. Actual CLI coverage
checks rebinding and mutation visible through caller aliases. Canonical AST
mutability fields and local pattern permission flags are now removed;
nonassignable-reference diagnostics no longer suggest `mut`. The parser now
uses the current grammar without `mut` or compatibility diagnostics, and source
AST flags are removed. Standalone and embedded test fixtures are converted;
1292 parser tests pass. All 113 changed parser snapshots preserve structure
after removing mutation fields and source-position numbers. The remaining
documentation/release audit and final validation are still required.
Assignment roots now participate in value dependencies: write-only references
close recursive groups, nested writers retain target/index dependencies, and
shadowed local targets do not create false top-level edges. The recursive-group
regression failed before the fix; 68 canonicalization tests and 258 solver
integration tests pass after it. This is a prerequisite, not completion of the
syntax migration. Follow-up coverage preserves independently instantiated
never-assigned functions, rejects escaping nested writers used incompatibly,
and avoids restricting a global because of assignment to a shadowed local.

- [x] User-approved design amendment: remove `mut` from the language entirely,
  rather than retaining an optional no-op keyword. Ordinary `let` bindings and
  parameters permit reassignment and field/index writes. Preserve existing
  shared-reference mutation semantics; do not introduce borrow checking or
  mutation permissions.
- [x] Update grammar, parser/source/canonical ASTs, binding checks, diagnostics,
  formatter, interfaces where affected, docs, examples, tests, and changesets.
  Migrate repository source to ordinary lets and parameters, without legacy
  syntax handling or migration diagnostics. Remove obsolete immutability claims
  and unused tracking. Current-source/documentation audit is recorded in
  `plans/mutation-syntax-hardening.md`; final release validation remains a
  separate goal-wide gate.
- [x] Preserve assignment type checking and alias-aware generalization. Audit
  reassignment of previously generalized functions/aliases as well as mutable
  containers: removing mutability flags must not allow a polymorphic contract
  to be replaced with a specialized value. Retain safe function polymorphism.
  Current source review confirms resolved assignment identity, restricted SCC
  members in environment free variables, and quantified-only instantiation.
  Re-ran four canonical assignment tests, three reassigned-function tests,
  28 shared-state/relationship tests, and both stored assignment/re-export tests.
  The joint deferred-constraint review is reconciled in the acceptance map below.
- [x] Regressions for a shared top-level empty Array used at incompatible types.
- [x] Sound generalization restriction accounting for reachable mutable state,
  while preserving safe function polymorphism and existing aliasing semantics.
  Active SCC/free-variable/constraint/instantiation/publication paths have been
  reviewed together; mixed tuple/overlay/error-row source and stored-consumer
  regressions supplement each individual boundary. See the current acceptance
  reconciliation in `docs/mutation-generalization-hardening.md`.
- [x] Arrays, maps, sets, nested records, aliases, captured state, reusable tasks,
  SCCs, and cross-module escape regressions; the selected restriction and
  constraint-only capture coverage are documented in
  `docs/mutation-generalization-hardening.md`. This finite implementation audit
  does not claim general type soundness or waive final integration validation.

Original root cause: `let mut` was used as the generalization criterion, although
ordinary bindings can contain shared mutable objects.

### 3. Formatter semantics

Post-async audit at 0db756d: added full parsed-source structural comparison,
excluding only Region metadata, to the repository-wide formatter test. It keeps
literal payloads, comments, declaration modes and expression structure; a
negative control confirms changed literal whitespace is detected. Explicit
async/nested-template cases pass for both LF and CRLF with an actual layout
change, and idempotence is checked separately. All 14 formatter tests pass.
The CLI formatting/execution regression now exercises a template returned from
an async block and still verifies exact whitespace and string length in V8.
That CLI test, formatter Clippy, formatting and diff checks pass. This is
additional finite evidence, not a substitute for the remaining full audit.

The independent structural-comparison and async CLI regression additions are
committed as `bf7729c`. The optional-parameter formatting test remains with the
pending syntax implementation. The current full workspace run passes all 15
formatter tests and CLI tests; strict workspace Clippy passes after the latest
AST guard tests as well.

- [x] Regressions for whitespace-only/trailing-space template payloads.
- [x] Syntax-aware preservation of templates, interpolation, escapes, raw macro
  bodies, markup text, comments, and supported line-ending semantics.
- [x] Compare meaning/literal payloads as well as reparsing and idempotence;
  CLI failure cannot overwrite input with an unsafe output.

Original root cause: line trimming occurred before literal context was
considered, and reparsing/comment comparison could not detect changed literal
values. Current formatting preserves parser-designated verbatim ranges and
checks their payloads plus physical-line token content before returning output.

Acceptance reconciliation: parser-selected verbatim ranges come from templates,
raw token trees, markup, and comments; parser backtracking restores the range
list (`lookahead_rolls_back_verbatim_ranges`). `format_with` copies every line
intersecting a protected range unchanged, reparses the candidate, and compares
protected payloads and physical-line tokens before returning it. This intentionally
conservative formatter does not rewrite interior expressions or reflow literals.
The 15 formatter tests cover nested interpolation/escapes, LF/CRLF and literal
carriage returns, raw macro definitions/calls, markup text, comments, empty input,
optional parameters, and actual async layout changes. Repository-source tests
compare complete parsed structure excluding Regions and separately test
idempotence; negative controls retain literal payload differences. CLI tests
execute an actually reformatted template and verify that a later invalid input
prevents writes to every file. The latest full workspace unit/integration run
passes all these tests. This closes section 3's listed acceptance checks; it
does not assert transactional recovery from unrelated filesystem write failures.

### 4. Deterministic modules

- [x] Reject `util.ald` alongside `util/mod.ald`, labeling both sources.
- [x] Canonical identities and import lookup use package/source-root context.
- [x] Audit root modules, workspaces, same-path dependencies, repeated `src`
  directories, interface/cache identities, and initialization/build ordering.
- [x] Repeated graph construction preserves build order and cycle diagnostics;
  fresh nested-workspace builds preserve graph order, module IDs, JavaScript,
  serialized interfaces, and package indexes under shuffled discovery.
- [x] Verify final bundle stability and source-ordered, exactly-once facade and
  unused-import initialization across fresh artifact-map insertion orders.
  Six compilations produce byte-identical bundles and pass bounded execution;
  see `docs/reexport-hardening.md` for the fixture's asserted effects.
- [x] External sibling member identities survive joint workspace relocation;
  a failing regression exposed absolute-path hashing and now passes with relative
  parent components. Distinct filesystem prefixes retain absolute identities.
- [x] Complete cache and re-export audits using the source-path and regression
  reconciliation in `docs/module-identity-hardening.md`, including the two
  subsequently reproduced cache defects fixed by `9b47b90` and `6b4af4c`.
  Final release validation remains a separate gate.
  The subsequent type/trait import-collision audit reproduced and fixed a
  namespace inconsistency in foreign binding insertion. Both import orders and
  renamed collisions reject publication; distinct aliases remain usable after
  interface serialization. See `docs/reexport-hardening.md` for current evidence.
  Follow-up coverage verifies enum/trait collisions, renamed private aliases/
  enums/traits/methods, and deterministic named/wildcard re-export cycle
  rejection across six discovery orders. All 181 driver tests pass; these
  cases required no further production change.
  Local checkpoint `08c77e5` commits only that import fix and its tests/snapshot/
  changeset. A fresh export of its exact staged tree passed full workspace
  tests, strict Clippy, and formatting independently of remaining dirty work.

Root cause: suffix-based graph resolution chooses one candidate; later maps
collapse duplicate canonical identities in nondeterministic traversal order.

Historical graph ordering checkpoint: permanent tests reproduced nondeterministic build
order and cycle selection. The ready queue now chooses the smallest source URI,
depth groups are sorted, and cycle DFS visits sorted roots/imports. Tests shuffle
discovery and import order across 32 independently allocated graphs. At that
checkpoint duplicate rejection and package/source-root-aware identity were still
open. Project-aware graph construction and compilation now share explicit
package/path maps and exact import lookup; see `docs/module-resolution-internals.md`.
Checkpoint validation: both new regressions failed before the fix and pass
after it; all 81 driver tests, full workspace tests (including CLI execution),
strict all-target/all-feature Clippy, and formatting checks pass. No snapshots
changed or remain pending. Final release packaging remains an open gate.

Nested workspace ownership audit: a permanent regression reproduced a source
file in an inner member being assigned its enclosing member's application
identity. Both package and path lookup selected the first containing source
root, so member order changed identity and local import resolution. A shared
`Project::source_owner` now selects the most specific containing source root.
The regression checks separate inner/outer identities and `~/util` edges, then
reverses member and module discovery order and compares emitted JavaScript,
module IDs, and serialized interfaces. It failed before the fix and passes
afterwards. This closes the nested-root finding, not every module audit item;
source-only driver fallback, external workspace members, and final packaging
remain to be reconciled. Formatting, strict Clippy, and full `cargo test`
including doctests pass. A fresh 17-crate package verification also passes,
followed by checks with its CLI against the nested workspace in both member
orders and execution of traits/explicit_async. Reusing an old package target
first produced a CLI linked to stale registry source; see
`docs/release-packaging-hardening.md` for the evidence and fresh-target rule.
The isolated source-ownership fix and changeset are committed as `adb35ac`.

Workspace member alias follow-up: loading `../external`,
`../external/../external`, and the member's config-file path treated equivalent
roots as different members. The filesystem-backed regression discovered four
modules for two actual source files before the fix. Member roots are now
canonicalized, deduplicated, and sorted before source directories and identities
are constructed. The regression now discovers one member/two modules, compiles
its local import, and emits two artifacts. On Unix it also checks that a symlink
member alias preserves package/path metadata and discovery. The initial
single-parent-path probe exposed noncanonical metadata, not a demonstrated
compilation failure by itself; the duplicate-discovery case is the confirmed
defect. Formatting, strict Clippy, all workspace unit/integration tests, and
the full cargo test/doctest run pass. Clippy's unnecessary-borrow finding was
fixed, then all unit/integration tests were rerun on the final production code.
The rebuilt CLI checks `/tmp/alder-member-aliases.t8tFDT/workspace` from `/tmp`
as one member/two modules and runs its member application successfully.
Commit `ca9ab0e` contains the root fix, regression, documentation, and changeset.
Final packaging must include this newer production change and use a fresh
target directory.

Production API cleanup now removes `source_identity`'s URI guessing and the
metadata-free build/graph entry points. Shared `source_identities` validation
requires both package and source-relative path entries, including applications;
missing metadata returns a deterministic driver/build-level error and no
interfaces or code. Regressions failed before the change for absent and partial
metadata. Fixture-specific metadata construction now lives under `cfg(test)`
with visibly named fixture helpers; production never infers a root from URI
text. Added tests cover missing-input ordering and the same explicit identity
under file and in-memory URLs. All 165 driver tests and the workspace
unit/integration suite pass, along with strict Clippy and formatting. Actual
rebuilt CLI checks pass for both nested and aliased workspace fixtures, and
the traits fixture executes successfully from `/tmp`. Full doctests pass with
the two pre-existing parser/runtime ignores. The final CLI rebuild and its
12 tests also pass after the build-error wording adjustment. No pending
snapshots or diff-check failures remain. This API cleanup was subsequently
committed as `9c16136`; coherence diagnostic ownership followed in `0ddfea3`.
The driver documentation example uses the actual project-aware
graph/build APIs and its separate compile-checked doctest passes instead of
being ignored, so it no longer teaches callers to discard source-root metadata.
Only the pre-existing parser/runtime ignored doctests remain.

### 5 and 11. Control flow and loop results

Follow-up review against `match_value` and the existing alternative-guard CLI
tests exposed a correction needed after `38a9ac9`: guard failure retries the
next alternative within the arm. Combining raw patterns before applying the
guard hid reachable pin exits. A Number function with a later String-valued
pin break after an irrefutable alternative guarded by false was accepted;
the permanent regression failed before correction. Applying `PatternFlow::guarded`
per alternative fixes both structural summaries and solver reachability. All
426 solver tests pass, including a successful-guard control. The rebuilt CLI
executes the guard-retry pin case successfully from `/tmp`; strict Clippy
passes. Full workspace tests, including doctests and all six direct AST tests,
now pass. The AST guard-transition correction is committed as
`008e94b`. Related solver/CLI edits still need coherent commit review.

Pattern-flow follow-up reproduced a second type-safety hole: a loop whose only
break was inside a pin was classified as divergent, so a String break payload
could satisfy a Number function. New `PatternFlow` distinguishes matching,
rejection, and pin exits; match summaries preserve those exits and process
alternatives/guards in order. Inference gates guards, bodies, and later arms
with the same match/reject outcomes. The negative regression failed before the
fix and now passes. All 421 pre-existing/current solver tests pass, plus the new
positive exiting-pin test (422 total). The rebuilt CLI executes the expanded
control_flow fixture from `/tmp`, including the exiting-pin assertion. Strict
Clippy is running; full workspace validation must be refreshed for these edits.
Nested sibling-pattern inference reproduced the corresponding false rejection:
`(^{ break 42 }, ^{ break "unreachable" })` still joined the second break.
`infer_pattern_child` now gates each child by earlier siblings' ability to match,
while preserving the enclosing reachability and return contract. The original
tuple regression failed before this fix and passes afterward; a conditional
first pin still makes the conflicting second break an error. Expanded positive
coverage includes array/record/constructor/error-tag children and now passes.
The rebuilt CLI executes the nested-pin case successfully from `/tmp`.
Strict Clippy and the fresh full workspace test run now pass, including
doctests. Two additional AST unit tests verify the match-versus-rejection
sequencing rules directly; all four AST unit tests pass. The isolated AST flow
implementation and unit tests are committed as `38a9ac9` with a changeset;
strict Clippy passes after these tests too. The related solver/CLI changes
remain uncommitted and require coherent review and final packaging.

Pin-expression contract audit found another confirmed hole: `fn invalid()
Number { match 0 { ^{ return } => 42, _ => 42 } }` was accepted because
infer_pattern called infer_expr with no enclosing return type. The regression
failed before the fix. Active match inference now threads its return contract
through recursive pattern inference, while ordinary binding patterns keep the
no-expression-context wrapper. All 420 solver tests pass, including nested pin
negative cases. Runtime return/constructor-skip/Option-propagation cases pass
with the rebuilt CLI invoked from `/tmp`. Strict workspace Clippy passes.
Structural summaries of exits from pin
expressions still require review; this does not close the pattern-flow audit.

Current requirement-to-code/test mapping: `docs/control-flow-acceptance.md`.
Use it alongside the outstanding checks below; it does not claim full acceptance.

Method-boundary coverage now directly verifies MissingReturn for zero-iteration
while paths in synchronous/asynchronous trait defaults and implementations.
All four negative cases pass. The actual control_flow CLI fixture also executes
default/override return branches and Option/Result propagation through valued
breaks, including suspended default methods, with twelve assertions in the new
methods.ald module. Both method forms route through the inspected shared
infer_function fallthrough/try-boundary checks. This is additional evidence,
not a new compiler fix or closure of the remaining pattern/guard audit.

Latest loop-target audit found a confirmed lowering mismatch: `loop { while
{ break 42 } {} break 0 }` checked as Number but returned 0 in the actual CLI.
Condition setup is emitted inside a synthetic while body; an unlabeled source
break was intercepted by that generated loop instead of its lexical target.
Codegen now tracks an explicit loop label with each result slot and labels all
source break/continue exits. Conditions are lowered before entering the new
while-body target scope. The actual control_flow fixture now passes both the
valued-break reproduction and an awaited condition that continues its enclosing
loop, with bounded counters distinguishing the targets. A source-aware codegen
snapshot verifies the outer label; the existing for-loop snapshot changes only
by adding its label. All 63 codegen tests pass, along with formatting, strict
workspace Clippy, and full workspace tests including doctests (two pre-existing
parser/runtime ignores). All four examples run successfully from `/tmp` against
the rebuilt CLI. The isolated fix, regressions, documentation, and changeset
are committed as `a9f1d44`; the broader acceptance checks below remain open.

- [x] Regressions for Number-returning zero-iteration loops and valued `break`.
- [x] Explicit fallthrough/divergence model, distinct from contains-return.
- [x] Each loop owns a result variable and the correct break/continue target.
- [x] Check blocks, branches, matches, early exits, `?`, lambdas, functions,
  methods, nested loops, while/for, async bodies, and unreachable paths.
- [x] Inference, diagnostics, and executed lowering agree for the specified
  control-flow regression matrix; see `docs/control-flow-acceptance.md` for
  the current source review and packaged execution evidence. This is not a
  general proof or closure of the separate row/generic/evaluation-order audits.

Root causes: any return in a loop bypasses fallthrough checks; loop expressions
always infer unit and break payloads do not constrain a target result.

Operand-reachability audit: `[ { break 42 }, { break "unreachable" } ]`
inside a Number-valued loop was rejected even though the second element cannot
execute. Array and tuple inference now carry structural fallthrough between
elements, including contextually checked array initializers; unreachable code
is still type checked, but its breaks do not join a live loop result. Regressions
cover both aggregate shapes, annotated arrays, and a potentially reached second
break that must still be rejected. The target-stack checkbox above is supported
by `infer_loop_body`, fresh Expr::Loop variables, unit while/for targets,
lambda/async stack isolation, the labeled codegen fix, and nested-loop execution.
The follow-up applies the same rule to records (including checked initializers
and spreads), tags, ordinary/tagged templates, indexing, and assignment indices
and values. The previous CLI rejected the template regression, and the expanded
CLI fixture now passes the first six added expression cases. A separate
assignment-index regression also failed before the fix. The prior 416-test
solver suite and strict Clippy passed for arrays/tuples; the expanded solver
suite now passes all 418 tests. The rebuilt CLI executes all added cases,
including assignment, successfully from `/tmp`. The older full workspace run
completed successfully but does not validate these newer edits; the fresh full
run and strict Clippy have now also passed, including doctests. The subsequent
method-only coverage passes all 419 solver tests and actual CLI execution.
This combined fix remains uncommitted;
no whole-control-flow completion claim is made.

### 6 and 7. Scheduler fairness and task frames

Current-source evidence map: `docs/async-hardening-acceptance.md`. Checklist
completion below concerns the specified implementation/test contracts, not
final clean-tree release approval.

- [x] Implement `async fn` and `async {}` with lazy execution, one explicit
  Task layer, lexical binding capture, and async-local control-flow boundaries.
- [x] Introduce Ref, SynchronizedRef, and a cancellation-safe semaphore with
  the contracts and tests in `plans/async-concurrency-hardening.md`.
- [x] Implement bounded fiber.map/forEach/tryMap/tryForEach; distinguish ordinary
  Result collection from explicit fail-fast propagation and parent cancellation.
- [x] Implement the approved `fiber.unbounded` concurrency-limit value. The bounded
  worker/adaptor implementation is verified separately in the async acceptance
  map; its completion does not deliver this public export.
- [x] Migrate inferred-async syntax, interfaces, examples, diagnostics, and docs;
  validate affected compiler/runtime and release boundaries. Current source,
  full-suite, and fresh packaged-CLI evidence is recorded in
  `docs/async-hardening-acceptance.md`; final clean-tree validation remains a
  separate delivery gate, as does the named unbounded traversal API implementation.
- [x] Regressions for immediately fulfilled Promise starvation and deep awaits.
- [x] Stack-safe task frames/trampoline with scheduler-visible composition;
  sequential await remains in the current fiber.
- [x] Bounded host yielding across resumptions, Promise/Join/All/Race/Fork,
  completion, and cleanup; avoid recursive scheduler execution/reentrancy.
- [x] Deep execution before/after suspension, cancellation/unwinding, reusable
  lazy tasks, error rows, scopes/finalizers, and provider inheritance.
- [x] Revisit pinned Effect reference and document ABI/semantic divergences.

Root causes: budget resets on every resumed run; JS `yield*` delegation nests
native generator calls outside the scheduler's visibility.

### 8. Record rows

Current implementation and named evidence: `docs/record-row-acceptance.md`.
Presence flags are removed; fresh defaults and Option lifting are separate from
ordinary row unification. The three overlay joint-audit criteria are reviewed in
`plans/record-overlay-hardening.md`; final integration and release gates remain.

- [x] Regressions for inferred `record.x + record.y` and correlated row tails.
- [x] Real record-row variables, symmetric unification, occurs/kind checks,
  instantiation/generalization, and stable interface round trips.
- [x] Access-order independence, spreads/extensions, optional fields, aliases,
  patterns, assignment, cross-module use, and incompatible constraints.
  Identity-copy regressions reject contradictory inherited field reads.
  Recursive overlay instantiation now has paired valid-overwrite/invalid-empty
  tests, serialized-interface consumers, a reviewed source-aware error snapshot,
  and imported CLI execution at zero and multiple recursive steps. All 459
  solver integration, 184 driver, and 15 CLI tests pass with strict Clippy;
  no production fix was needed at that checkpoint. The subsequent ordered-
  producer, contract, retained-equation, and occurs-check source reviews are
  recorded in the overlay plan, with 48 overlay integration tests passing.

Root cause: record extensions are reduced to an openness boolean, losing row
identity and making the first field access freeze the inferred field set.

### 9. Option representation

Recent cycle finding: a recursive enum containing a mutable array reproduced
a stack overflow in derived equality. The CLI reproduction and invariants are recorded in
`docs/option-constructor-hardening.md`. Its initial undefined `$self` failure
also exposed nested recursive evidence losing the emitted dictionary binding;
that codegen bug is fixed with source-level and compiled acyclic regressions.
Active nominal-pair tracking now fixes the original cyclic Eq reproduction,
with kernel and generic/non-generic CLI checks. Mutually recursive generic
graphs and subsequent mutation also pass through the CLI. The user-facing
Hash/Show/JSON/Ord cycle policy is implemented with derived/container, source Ord,
mutual-cycle, shared-child, and mutation coverage recorded in
`plans/cyclic-value-operations.md` and `docs/cyclic-values.md`; the broader
operation/law acceptance boxes below stay open.

Large-payload follow-up: hashing a 250 KB UTF-8 string reproduced RangeError
from spreading the bytes as call arguments. Text and BigInt magnitude encoding
now append iteratively, preserving the existing hash byte stream. Independent
kernel byte-stream checks and compiled large String/Option Hash calls pass;
all 58 kernel and 15 CLI tests plus associated doctests pass. See
`docs/collection-runtime-acceptance.md`; final packaging remains outstanding.
The four-file large-payload fix is committed as `eaea2b7` after full workspace
tests/doctests, strict Clippy, and formatting passed. Other pending hardening
changes were not staged, and nothing was pushed or merged.

- [x] Regression for equality of separately constructed nested Some(None).
- [x] Audit equality, mapping, patterns, derives, ordering/hash/show/JSON,
  unit payloads, nested containers, and higher-kinded operations.
- [x] Relevant equality/hash laws and round trips; centralize payload handling.
  Current source paths, granular regression coverage, compiled generic evidence,
  four-layer law matrix, and deliberate law limits are mapped in
  `docs/option-operation-acceptance.md`. Full tests/doctests (69 kernel, 17 CLI),
  strict Clippy, formatting, and whitespace checks pass. Final integration and
  package verification remain separate gates.
  Numeric-law audit found primitive Hash superclass evidence using `$equal`
  (Object.is) instead of primitive Eq (`===`): the compiled cross-module
  `hash_equal(0, -0)` failed. Emit strict equality for bare primitive Hash
  dictionaries; retain payload-aware container/derived evidence. Permanent CLI
  cases cover signed zero, nested Options/arrays, derived positional/record
  payloads, NaN inequality, infinities, and the other primitive Hash instances.
  The source-aware codegen snapshot checks superclass emission directly.
  Full workspace tests/doctests, strict Clippy, and formatting pass after the
  fix; reviewed primitive/container/error-row snapshots and no pending files.
  Final packaging and coherent integration commits remain required.
  The independent primitive Hash fix and cross-module CLI fixture are committed
  as `b8924f2`, after exact-index export passed full workspace tests/doctests,
  strict Clippy, and formatting. Broader hardening changes remain uncommitted;
  this checkpoint does not close joint inference or final release gates.

Root cause: Option equality passes a box to the payload dictionary without
unwrapping the payload according to the runtime representation.

### 10. Local extern files

- [x] Regression for `./client.js` beside an Alder source module.
- [x] Carry physical origins through virtual modules; resolve relative externs
  consistently regardless of shell cwd and across package boundaries.
- [x] CLI build/run with local Promise wrapper, typed Result fulfillment,
  throw/rejection, AbortSignal cancellation, and contextual resolution errors.

Root cause: virtual module imports lack a physical importer location.

## Related audit

- [x] Prelude module members carry actual stdlib signatures, rejecting unknown
  members and invalid calls. Removed the untyped `ValueRef::Builtin` path.
- [x] Audit stdlib declarations against their runtime implementations.
  Current inventory and per-family review: `docs/stdlib-contract-audit.md`.
  All 51 extern targets and 17 packaged source copies match, and all-builtin
  linking plus scalar kernel/CLI checks pass. The non-advancing Array instance
  is replaced by independent ArrayIterator cursors in `edbcd63`, with mutation,
  exhaustion, Option payload, associated-type and generalization regressions;
  see `plans/array-iterator-hardening.md`.
  Array callback auditing reproduced native index/source-array arguments
  leaking to unary extern-returned functions; map/filter/flatMap/apply adapters
  now enforce the declared arity. See `docs/collection-runtime-acceptance.md`
  for kernel and actual CLI reproduction/verification.
  Checkpoint `6080822` independently commits this fix, two kernel regressions,
  externs execution case, acceptance note, and changeset. Its isolated staged
  tree passes full workspace tests, strict Clippy, and formatting; unrelated
  dirty compiler/runtime work was not included.
  The unchecked `json.decode` bypass is fixed with bounded dictionary dispatch;
  codec operation coverage is mapped in `docs/json-hardening.md`. This closes
  the declaration/runtime family review, not the separate generic dictionary
  audit or final integrated release gates.
- [x] `map.get` wraps present values so a present `None` differs from absence.
  Compiled cross-module collection coverage now also checks present unit,
  alias-visible replacement and nested payload mutation, Map/Set identity keys,
  mutation return units, and option.map's None/unit layers. The actual standalone
  CLI suite passes; see `docs/collection-runtime-acceptance.md`.
- [x] Nested lambda annotations share same-named enclosing type variables as
  promised in `docs/language.md`, including nested lambda scopes and HKT.
- [x] Generic evidence and interface contract fidelity.
  `docs/dictionary-lowering-hardening.md` maps selection/coherence, checked
  publication, imported/default methods, slot ordering, capture, initialization,
  recursive evidence, and bounded output-size regressions. Added Ord/Eq/Show
  depth controls and compiled imported Ord/Eq closure tests pass. Full integrated
  tests/doctests (72 codegen, 199 driver, 72 kernel, 17 CLI, 467 inference), strict
  Clippy, and formatting pass. Integration commits and final package/clean-tree
  gates remain open; this finite review is not a general soundness proof.
- [x] Evaluation order/exactly-once codegen and control-flow boundaries.
  Source-to-regression mapping: `docs/evaluation-order-hardening.md`, including
  calls/pipes, aggregate operands, mutation, patterns, exits, and lazy tasks.
  The audit reproduced incorrect Coalesce typing and corrected its Option
  representation handling. Focused codegen and actual CLI regressions pass;
  full workspace tests/doctests, formatting, and strict Clippy also pass
  (70 codegen, 199 driver, 72 kernel, 17 CLI, 467 inference tests). Validation
  is for the integrated worktree; committed-tree/release gates remain separate.
- [x] Async inference versus runtime representation.
- [x] Promise throws/mappers/malformed returns/reentrant cancellation/late exits.
- [x] Exactly-once completion/observers, child ownership, all/race cleanup,
  finalizer registration while/after closing, and masked interruption.
  Reconciled against current source and the 63-test kernel suite after `92beacb`.
  See `docs/async-hardening-acceptance.md` for the implementation-to-regression
  mapping, including throwing mappers/getters and late cancellation. These close
  the specified related-audit items, not the pending public unbounded API choice,
  general compiler contract audits, or final committed-tree release gates.
- [x] Deterministic cache identities, source fidelity, and deferred constructs.
  Current evidence: `docs/module-identity-hardening.md` and
  `docs/source-boundaries-hardening.md`. Deferred declarations publish types,
  not fake runtime implementations; failed runtime use does not publish a
  consumer artifact/interface. Diagnostic byte spans retain the actual source.
- [x] Public wildcard/name re-exports, including aliases, type/trait namespace
  collisions, privacy, package dictionary ownership, shared mutable contracts,
  and source-ordered exactly-once initialization. Source-to-regression review
  is recorded in `docs/reexport-hardening.md`, beyond initial value publication.
- [x] Identify unlinked Elm-era files in can/constrain/solve and correct the
  stale union-find pipeline claim; see `docs/compiler-implementation-map.md`.

## Evidence log

- Refreshed integrated package verification after `08c77e5` and the overlay
  fixes: all 17 publishable crates verify successfully in a fresh target
  directory, with 1,507 extracted source/stdlib/kernel files matching the
  worktree. The packaged CLI runs all four examples and four integration
  projects from fresh source/config-only copies, and the extracted kernel
  passes the original timer-fairness probe. Full integrated workspace tests
  also pass. Details and limits are in `docs/release-packaging-hardening.md`;
  this does not close joint audits or the clean-committed-branch gate.

- Current documentation reconciliation: corrected language-guide record rules
  from presence-based reads/writes/spreads to ordinary Option field semantics,
  contextual construction lifting/defaults, exact assignment types, and
  rightmost None overwrites. Corrected codegen's stale inferred-async description
  to explicit lazy async fn/blocks and documented Option propagation alongside
  Result. Two error-row examples now declare async and await their Task result.
  Replaced the obsolete M2 provider-tail question with the current statement
  boundary and explicitly retained the deferred provider-checker milestone.
  Source/parser comments no longer advertise a mutation modifier. These are
  documentation corrections, not new language behavior or completed release gates.

- Record spread evaluation order (active, uncommitted): CLI execution reproduced
  later block setup running before an earlier spread copied its source. Record
  codegen now captures earlier field values and shallow spread copies before
  later setup. Runtime assertions verify exact event order, copy timing, and
  the different ordinary-call argument behavior. Added a codegen patch changeset.
  Full workspace tests including doctests, formatting, and strict Clippy pass;
  no existing codegen snapshots changed. Other prefix-combining expressions
  remain in the evaluation-order audit.

- Declared overlay tail replacement (active, uncommitted): positive/negative
  regressions distinguish an explicit replacement field from an invalid promise
  to preserve the original universal tail unchanged. The valid case preserves
  unrelated fields while replacing Number with String. All 244 solver tests,
  formatting, and strict Clippy pass; general tail/cycle entailment remains open.

- Late overlay unions (active, uncommitted): a three-way generic optional
  fallback reproduced error-inclusion growth from 16 to 20 after error solving.
  Constraint-producing overlay contract checks now run inside a joint fixed
  point with both solvers; final generic rigidity validation runs afterward.
  Three-way positive/negative contracts pass. All 242 solver tests, full
  workspace tests including doctests, formatting, and strict Clippy pass.
  General unresolved/cyclic overlay entailment remains open.

- Result union isolation (active, uncommitted): three regressions verify
  independent generic calls retain precise separate error rows and error
  widening does not hide incompatible mutable array element types or nested
  required/optional field presence. All 240 solver integration tests pass.
  Broader late-constraint/cache acceptance remains open.

- Result fallback union (active, uncommitted): row-valued Result alternatives
  now join their error sets through a reused exact-union target, while success
  payloads retain equality checking. Closed and independent generic fallback
  positives pass; narrowed return contracts reject. Actual imported CLI calls
  verify absent/present fallback behavior. All 237 solver tests, full workspace
  tests including doctests, formatting, and strict Clippy pass. Cache reuse,
  mutable payloads, and late-added constraints remain explicit audit work.

- Optional Result fallback investigation (active, uncommitted): a new positive
  regression fails when required/optional fallback Result values have different
  closed error rows. `join_values` imposes row equality before propagation can
  retain both possible errors. The unignored failure and implementation concerns
  are recorded in `plans/record-overlay-hardening.md`. The paired negative fails
  at the same early point and is not evidence of correct narrowed-row rejection.

- Overlay/error-row soundness (active, uncommitted): reproduced a wrong closed
  error promise accepted after `?` on a merged Result field. Scheme preparation
  eliminated a tail referenced by the overlay before connected-variable
  selection. It now protects residual overlay variables from existential
  elimination. Positive selected-row and negative dropped-error regressions
  pass; all 233 solver tests, full workspace tests including doctests,
  formatting, and strict Clippy pass. Imported CLI coverage executes
  propagation/matching of the rightmost selected error.

- Captured overlay factories (active, uncommitted): three regressions verify
  returned lambdas preserve merge relationships, reject incompatible overwrites,
  and cannot re-generalize captured shared array payloads. Actual imported CLI
  execution verifies mutation through the returned merge preserves the captured
  array alias. All 231 solver integration tests and the CLI fixture suite pass.
  This is additional boundary evidence, not completion of the overlay audit.

- Shared overlay expansion (active, uncommitted): reproduced 12 shared binary
  nodes expanding into 8,192 operands. Contract checking now expands each
  producer once at its rightmost occurrence. A structural regression checks
  the retained operand count and ordering around an intervening write. All
  228 source-level solver tests, full workspace tests including doctests,
  formatting, and strict Clippy pass. This removes exponential
  DAG unfolding; it does not resolve the remaining general cycle-validity work.

- Recursive overlay payloads (active, uncommitted): permanent paired tests
  distinguish an infinite nested payload (asserting the precise InfiniteType
  error) from a required overwrite that breaks the cycle. Both pass, alongside
  recursive forwarding; all 228 solver integration tests pass. No compiler
  implementation change was needed for these cases. Unknown-tail cycle
  entailment and graph-expansion bounds still require investigation.

- Overlay result consistency (active, uncommitted): reproduced conflicting
  Number/String field promises from repeated merges of the same typed inputs
  escaping without a concrete caller. Equal ordered operands now share result
  constraints; reversed input order remains distinct. Recursive forwarding and
  the reversed-order positive case pass. The fixed-point loop now notices
  substitution bindings as well as new fields/discharged relations. All 226
  solver tests, full workspace tests including doctests, formatting, and strict
  Clippy pass. General cycle validity and
  expansion complexity remain unfinished.

- Partial overlay fields (active, uncommitted): reproduced optional-field reads
  failing with an unrelated universal tail. Rejected an eager whole-row
  reduction because it constrained unrelated hidden overwrites. The solver now
  exposes only provable field presence/payloads and retains the residual ordered
  relationship. All 223 solver tests pass, including known required fallbacks
  and a negative unknown fallback contract. CLI fixtures execute imported open
  row reads for absent, present-None, and present-Some values while allowing an
  unrelated field's type to change. Full workspace tests including doctests,
  formatting, and strict Clippy pass; broader termination,
  cyclic validity, and constraint interaction work remain open.

- Overlay shape timing (active, uncommitted): reproduced optional fields being
  inferred as required because closed instantiated overlays were solved only
  after field access. Calls now solve available closed shapes after argument
  checking. The regression passes, and actual imported CLI merges preserve
  absent versus present-None optional values. All 220 solver tests, full
  workspace tests including doctests, formatting, and strict Clippy pass.
  Reviewed the stored-interface diagnostic snapshot change to the consumer's
  ordinary return-contract region. Truly partial open shapes remain unfinished.

- Stored generic overlays (active, uncommitted): actual inferred merge metadata
  now replaces the synthetic transport fixture. Driver consumers accept
  independent valid calls and reject a wrong overwrite result using a serialized
  interface after all producer/copy arenas are destroyed. Reviewed the new
  colorless diagnostic snapshot: it labels the current consumer call, and the
  failed consumer publishes no interface or artifact. All 110 driver tests pass.
  This closes the basic stored-generic-merge evidence gap, not the wider overlay
  solver/contract acceptance work.

- Composed overlay contract check (active, uncommitted): reproduced an invalid
  Number promise hidden by merging an intermediate generic overlay with an
  unrelated closed field. Contract validation now follows exact intermediate
  results back to ordered input operands, with path-local revisit protection.
  The negative case rejects; composed disjoint merges, higher-order forwarding,
  independent top-level alias calls, and a final required override pass. All
  217 solver integration tests, full workspace tests (including doctests),
  formatting, and strict all-target/all-feature Clippy pass.
  Partial-shape propagation, cyclic validity, and full constraint entailment
  remain open; the broader overlay implementation is not ready to commit.

- Overlay generic-contract checks (active, uncommitted): field guarantees now
  respect rightmost required presence and reject hidden restrictions on a
  universal tail. All payload unification precedes final contract validation.
  A second adversarial test reproduced treating independent input tails as the
  left result tail; it now rejects that promise while accepting the shared-tail
  counterpart. The 211 solver integration tests and strict Clippy pass. Broader
  symbolic/partial overlay solving and contract entailment are still required;
  do not treat these targeted checks as completion of overlay acceptance.

- Overlay inference integration (active, uncommitted): added scheme transport,
  connected-variable generalization, instantiation, and a fixed-point pass for
  closed operand shapes. The two original independent-merge regressions now
  pass, as did the original 207 solver integration tests. A new adversarial
  declared-universal test fails, demonstrating that unresolved overlays can
  impose an unproved hidden restriction on an annotated input tail. It remains
  unignored. Strict Clippy passes, but this is not a sound completed overlay
  implementation; see `plans/record-overlay-hardening.md` for next work.

- Independent overlay transport (active, uncommitted): added ordered operand/
  result metadata to canonical annotations, deep arena copy, and owned
  interfaces; advanced the interface wire format to 4. A focused synthetic
  transport test proves round-trip preservation after arena destruction and
  operand-order-sensitive fingerprints. Workspace checking and strict Clippy
  pass. Solver integration remains pending and the two merge regressions still
  fail; this scaffold is not a completed semantic fix.

- Independent overlay investigation: two new unignored regressions reproduce
  rejection of disjoint fields and right-biased different-type overwrites in
  `fn merge(left, right) { { ..left, ..right } }`. The explicit tail equality in
  `infer_record` is the root cause. Revisited Elm's row equality implementation;
  it does not implement ordered overlay. Integration requirements and the
  deferred-overlay design direction are recorded in
  `plans/record-overlay-hardening.md`. These tests currently fail and are left
  uncommitted as active implementation work, not ignored or accepted as the
  intended behavior. The overall hardening goal remains incomplete.

- Open spread overlaps: reproduced an inferred spread operand accepting a
  String overwrite in a function promising Number. Fields hidden in open tails
  were omitted from the payload alternatives. Surviving overlaps now add
  optional-field requirements to the relevant tail, covering both later hidden
  overwrites and earlier hidden fallbacks for optional fields. Deferred checks
  preserve universal input tails when a later required field overwrites the
  alternatives. Four solver regressions and cross-module CLI calls verify
  rejection, absence, unrelated fields, compatible payloads, and final overrides.
  Full workspace tests (205 solver integration tests), strict all-target/all-
  feature Clippy, formatting, and diff checks pass; no snapshot changes or
  pending snapshots. Added a solve patch changeset and updated row design docs.
  General merging of independent open tails remains unresolved; this is a
  targeted soundness fix, not completion of record-row acceptance.

- Optional spread fallbacks: reproduced accepting a numeric fallback as
  `Option[String]` while rejecting the safe required Number result. Inference
  copied the optional spread type and discarded the value retained at runtime
  when that property is absent. It now joins surviving payload alternatives
  and preserves required fallback presence. Adversarial testing caught the
  initial fix rejecting a final required overwrite; joins now happen only
  after all fields are processed, discarding overwritten alternatives. Five
  new solver regressions and expanded CLI cases cover these paths, including
  optional/optional absence and required replacement with a different type.
  Full workspace tests (201 solver integration tests), strict all-target/all-
  feature Clippy, formatting, and diff checks pass; no snapshot changes or
  pending snapshots. Updated record/language docs and added a solve patch
  changeset. Independent open tails and the broader row acceptance work remain
  unresolved, and final package verification must include this later fix.

- Package verification checkpoint: all 17 publishable workspace crates package
  and build from extracted archives using a fresh temporary target directory
  and Cargo's staged registry. The old target directory produced stale API
  errors; a fresh directory passed without compiler changes. Inspected embedded
  stdlib/kernel archive contents. The packaged CLI runs hello and async/externs/
  traits/records/control-flow fixtures from `/tmp`. cargo-dist 0.32.0 plans all
  five configured platforms/installers successfully. Added CI archive build
  verification in a fresh runner target directory and removed an obsolete M1
  workspace-build comment. Full tests, strict Clippy, formatting, and diff
  checks pass; no pending snapshots. See `docs/release-packaging-hardening.md`.
  No publishable source changes, so no changeset needed. Final-commit packaging,
  version-update verification, remote platform builds, and the broader semantic
  acceptance audit remain open; no tags, pushes, or publication performed.

- Wildcard privacy/collision coverage: exact owned-interface assertions verify
  that only public values, aliases, enums, and traits are published, without
  copying private diagnostic names or dependency instances. A consumer with
  only the facade interface checks public declarations successfully, while
  attempts to import private functions, aliases, enums, traits, and methods
  fail without publishing consumer artifacts/interfaces. Conflicting wildcard
  value exports reject facade publication and label both imports in a reviewed
  colorless snapshot. No implementation defect was found in these cases.
  Full workspace tests (108 driver tests), strict all-target/all-feature
  Clippy, formatting, and diff checks pass; no pending snapshots. Tests/docs
  only, so no changeset needed. Cross-namespace and broader package/cycle
  boundaries remain open.

- Trait-method import aliases: the CLI reproduced a false collision when one
  method was re-exported under two distinct aliases. Environment insertion used
  the method identity's original name instead of the selected local binding.
  Imported methods now use that binding for insertion/collision checks while
  retaining their original dispatch identity. CLI coverage executes both
  aliases through named/wildcard facades and a further consumer rename. Two
  driver negatives reject leaked original names and actual local alias
  collisions, asserting no consumer interface/artifact publication; reviewed
  their colorless source-aware snapshots. Full workspace tests (106 driver
  tests), focused CLI execution, strict all-target/all-feature Clippy,
  formatting, and diff checks pass. No pending snapshots. Added a can patch
  changeset; broader privacy/collision and package boundary audits remain open.

- Re-export initialization: reproduced a CLI runtime failure where references
  resolved directly to the original owner and omitted facade initialization.
  Codegen now emits explicit imports as bare AST imports in source order and
  retains them in dependency metadata. CLI tests verify shared array identity
  through multiple aliases, ordered exactly-once initialization through a chain,
  and unused sibling imports whose source order reverses filename order. Driver
  regressions assert facade/owner dependency retention. Full workspace tests,
  focused CLI execution, strict all-target/all-feature Clippy, formatting, and
  diff checks pass. No snapshots changed or pending snapshots remain. Added a
  codegen patch changeset. Wider cyclic/package initialization and other
  re-export acceptance work remain open.

- Re-export boundary coverage: extended the CLI fixture with an enum, generic
  record alias, and trait renamed by one facade and wildcard-exported by another.
  Construction and both generic/qualified trait dispatch execute correctly.
  A driver test serializes a package-root interface and successfully checks an
  alias/function consumer with only that stored root, no leaf interface/arena.
  A negative test rejects explicit private value re-export and asserts that the
  facade publishes neither interface nor artifact; reviewed its colorless
  source-aware diagnostic. No implementation changes were necessary. Direct
  public module-namespace imports are currently prohibited by parser syntax;
  canonical support alone is not a source-language promise. Wider mutation,
  collision, initialization, cycle, and runtime package boundaries remain open.
  Validation: full workspace tests pass, including 104 driver tests and expanded
  CLI execution. Strict all-target/all-feature Clippy, formatting, and diff checks
  pass. Reviewed the single new private-name snapshot; no pending snapshots.
  This checkpoint is tests/documentation only and needs no changeset.

- Public re-export publication: driver tests reproduced named and wildcard
  facades publishing no values, leaving consumers with unknown-name errors.
  Interface builders now take dependency interfaces and copy selected public
  entries while retaining their original identities and schemes. A second
  regression caught selecting only the first of repeated-source aliases; each
  alias is now processed independently. Tests compare owned value identities
  and schemes, and the real CLI traits fixture calls a generic function through
  a wildcard facade. No forwarding JS source is generated. The API signature
  change and wider remaining audit are documented in `docs/reexport-hardening.md`.
  Validation: full workspace tests pass, including 102 driver tests and the
  expanded real CLI fixture. Strict all-target/all-feature Clippy, formatting,
  and diff checks pass. No snapshots changed. Package/release and the wider
  re-export acceptance gates remain open.

- Active/legacy implementation audit: traced crate-root module declarations and
  driver call sites. Listed the unlinked can/constrain/solve remnants and their
  inactive tests in `docs/compiler-implementation-map.md`. Documented that the
  compiled `run` helper skips coherence/trait-obligation resolution, while the
  driver and solved-codegen tests use `solve`. Added API warnings and corrected
  the canonical-internals pipeline's obsolete union-find argument. Source macro
  invocations/comptime are rejected before dormant later-phase arms; table/schema
  and macro declarations emit no runtime implementation. Publication semantics
  for deferred declarations remain open. No compiler behavior was changed.
  Validation: full workspace tests, strict all-target/all-feature Clippy,
  formatting, and diff checks pass. Documentation/API comments only; no snapshot
  changes or publishable behavior change requiring a changeset.

- Deferred markup/style boundary: driver regressions reproduced successful
  executable builds of markup containing `@if` and a dimension-valued style.
  Markup emitted object descriptors, dropping If/For/Match children as undefined;
  style emitted ordinary objects rather than the planned CSS assets/classes.
  Build/Test now reject these expressions with source-aware diagnostics while
  preserving provisional Check support. Removed the misleading style/markup/
  element/child lowering helpers (recoverable from Git). Reviewed both colorless
  snapshots and checked no executable artifacts. M2/M6/M8 plans record the real
  implementation boundary. Declaration-only deferred forms and a final sweep of
  all placeholder paths remain open; no web/CSS milestone was implemented here.
  Validation: full workspace tests pass, including 99 driver tests and CLI
  fixtures. Strict all-target/all-feature Clippy, formatting, and diff checks
  pass. Both new snapshots were reviewed; no pending snapshots remain.

- Deferred reactivity execution boundary: separate driver regressions reproduced
  successful executable builds of `state(0)` and `component Counter() { <div /> }`.
  Codegen erased state to its initializer and compiled components as ordinary
  functions, with no documented signal tracking, memoization, or lifecycle.
  Build/Test now reject these forms at their source regions until M6. Check-only
  support stays provisional. Reviewed both colorless diagnostics and asserted
  no artifacts in both executable modes. M2/M6 plans record the guards and their
  eventual replacement. This does not implement web reactivity. Standalone
  markup/style and declaration-only deferred forms remain separate audit work.
  Validation: full workspace tests pass, including 97 driver tests and the CLI
  fixtures. Strict all-target/all-feature Clippy, formatting, and diff checks
  pass. Both new snapshots were reviewed; no pending snapshots remain.

- Deferred query execution boundary: a driver regression reproduced a successful
  build of a module declaring `table users {}` and a public function returning
  `query { select * from users }`. Codegen discarded the
  query and emitted `$query()`, whose kernel stub only throws until M7. Build
  and Test emission now fail at the query's source region instead. Check mode
  preserves the provisional M2 syntax/type representation and emits no artifact.
  Reviewed the colorless driver snapshot with the actual source; both executable
  modes assert no artifact. This does not implement M7. Macro calls were also
  inspected: canonicalization already rejects them before the solver's dormant
  Any arm, and codegen has a defensive rejection. Markup/style/component/state
  placeholder semantics and declaration-only deferred forms still need audit.
  Validation: full workspace tests pass, including 95 driver tests and real CLI
  fixtures. Strict all-target/all-feature Clippy, formatting, and diff checks
  pass. The new diagnostic snapshot was reviewed; no pending snapshots remain.

- Result error-kind audit: full solver probes confirm that an identity over
  `Result[Number, String]` is accepted while constructing either variant or
  matching it is rejected. Bare variables in the error position are forced to
  ErrorRow kind, but concrete non-group types pass through normal conversion;
  Result return checking also assumes row inclusion. Existing HKT fixtures and
  row-focused language docs leave the intended ordinary-error policy unclear.
  Requested the user's choice between rows/groups only and support for ordinary
  error types as well. `docs/result-error-kind-audit.md` preserves the exact
  reproductions, code paths, and required downstream checks. Do not silently
  select a policy or mark this issue resolved. Temporary diagnostic probes were
  removed; no compiler behavior or regression expectations were changed.

- Repeated bound clause fix: a permanent canonicalization test reproduced the
  bounds-length assertion panic. The first pass merged bounds by variable and
  the second pass incorrectly reused that aggregate for each clause. Keep
  clause-local resolved traits separately from a distinct per-variable set used
  for projection lookup. This preserves source order and prevents duplicate
  mentions of one trait from becoming false associated-name ambiguity.
  Tests cover interleaved variables, repeated traits, projection equalities
  before later bounds, and a genuine two-trait ambiguity. Reviewed the structured
  error snapshot: it identifies Item and both distinct candidates at the correct
  source region. CLI fixtures now use split/duplicate clauses in local/imported
  externs and in a generic function executing both Show and Eq dictionaries.
  Non-row Result inference remains the next recorded follow-up.
  Validation: full workspace tests pass, including 65 canonicalization tests
  and the expanded CLI fixtures; strict all-target/all-feature Clippy,
  formatting, and diff checks pass. Only the reviewed new ambiguity snapshot
  was added, with no pending snapshots. Release and broader audit gates remain open.

- Constrained extern ABI fix: the emitted wrapper declared only source
  parameters although callers supplied leading dictionaries. A bounded identity
  therefore returned its Show dictionary instead of Number, reproduced in the
  actual CLI fixture. Wrappers now declare the solved dictionary slots before
  source parameters, without forwarding those slots to foreign JavaScript.
  Regressions exercise multiple bounds, direct/first-class/imported/generic calls,
  synchronous Result wrapping, and Promise wrapping with the final AbortSignal. Foreign
  helpers assert argument counts; a codegen snapshot covers the adapter shape.
  A separate confirmed canonicalization defect surfaced while writing the test:
  `where a: Show, a: Eq` panics at `canonicalize_constraints`' bounds-length
  assertion. The first pass accumulates bounds by variable, but the second pass
  assumes that aggregate belongs to each individual bound clause. Reproduce in
  a permanent canonicalization test and fix clause handling (now completed in
  the follow-up above); the extern fixture initially used `a: Show + Eq` to
  isolate the ABI and now exercises split clauses too.
  Another follow-up: comparing `bounded_result(45): Result[Number, String]`
  directly with `Ok(45)` reports expected String/found a; matching it fails too.
  The adapter test uses a declared error row to isolate its calling convention.
  Audit non-row Result constructor/pattern inference separately; do not assume
  the row-specific paths cover it.
  Validation: full workspace tests, strict all-target/all-feature Clippy,
  formatting, and diff checks pass. Reviewed the new source-aware codegen
  snapshot; no pending snapshots remain. No runtime scheduler changes were made.

- Json module boundary fix: module encode/decode now require Json evidence and
  execute the selected dictionary via kernel forwarding helpers. Removed the
  unchecked parser/stringifier entry points. The original CLI wrong-payload
  assertion failed before the fix. Imported direct dictionary calls additionally
  exposed a private `$v_` import bug; foreign references now use public names.
  CLI tests cover direct, first-class, imported generic, custom-codec, and nested
  failure cases. Full-solver tests cover absent instances and missing bounds;
  a reviewed colorless diagnostic snapshot labels the offending decode reference.
  Follow-up inspection risk at that checkpoint: ordinary extern wrappers omit hidden
  dictionary parameters. `docs/traits-internals.md` already specifies that these
  adapters accept dictionaries but forward source arguments only to foreign JS.
  The follow-up above reproduces and fixes that contract violation; built-in Json
  exports bypass those wrappers and target internal dictionary-aware helpers.
  Validation: full workspace tests pass (196 solver tests, 94 driver tests,
  15 kernel tests, and real CLI fixtures), as do strict all-target/all-feature
  Clippy, formatting, and diff checks. The new diagnostic snapshot was reviewed;
  no pending snapshots remain. Wider audit and release-packaging gates stay open.

- Primitive Json contract fix: reproduced a compiled Number decode returning
  Ok with a string payload. Primitive instance evidence previously collapsed
  all types to `JsonKernel`; codegen supplied the unchecked JSON.parse wrapper.
  Dedicated Number/String/Bool/BigInt/unit intrinsics now retain the requested
  codec kind. Runtime and CLI regressions cover mismatches, overflow, unit and
  BigInt round trips, and path-qualified nested/derived failures. BigInt uses
  decimal JSON strings; non-finite Number encoding rejects rather than silently
  returning null. `docs/json-hardening.md` records the design and open work.
  The separate module bypass was still open at this checkpoint and is fixed in
  the follow-up above; the wider Json audit is not complete.
  Validation: full workspace tests pass, including 15 kernel runtime tests and
  the expanded CLI traits fixture. Strict all-target/all-feature Clippy,
  formatting, and diff checks pass. No snapshot changes or pending snapshots.

- Higher-order error-row boundary check: added positive exhaustive-matching and
  negative narrowed-result tests where a factory returns a row-polymorphic
  function and a separate function invokes it. Both pass without solver changes.
  The CLI errors fixture now traverses the imported factory and indirect call
  for its existing left failure, right failure, and successful sum assertions;
  execution passes. This verifies those paths, not all higher-order contracts.
  Next stdlib audit target remains Json: code inspection confirms not only the
  unbounded `std/json.ald` decode extern but also `Intrinsic::JsonKernel` primitive
  dictionaries route decoding through the same unchecked `$jsonDecode` parser.
  Adding a bound to the module wrapper alone would therefore not establish
  typed decoding. Runtime reproductions and type-specific validation are next.
  Validation: full workspace tests, strict all-target/all-feature Clippy,
  formatting, and diff checks pass. No snapshots changed. This checkpoint adds
  tests/documentation only and does not need a publishable-crate changeset.

- Recursive error-union follow-up: two mutually recursive functions first
  reproduced a closed-row mismatch because an early `return Err` fixed the
  unannotated output to its first tag. Early row-valued Result returns now create
  an exact accumulating output, including the variable row of `Ok`. A wider
  attempted final-return change regressed non-row higher-kinded Result uses and
  was replaced with this early-return-specific handling; those tests pass.
  The recursive reproduction then exposed the independent cycle issue: exact
  tails were treated as externally open merely because they depended on each
  other. Candidate elimination now closes only exact components with no external
  open/universal dependency, after known tags reach a fixed point. Positive tests
  cover a closed cycle and concrete instantiation of an external source; the
  negative checks an actual open-row exhaustiveness error, not generic failure.
  CLI coverage executes both error tags through an imported recursive function.
  Validation: full workspace tests pass (192 solver integration tests, 93 driver
  tests, and runtime/CLI fixtures), strict all-target/all-feature Clippy passes,
  and formatting/diff checks pass. No snapshot changes or pending snapshots.

- Deferred error-row diagnostic origins: a narrowed annotated binding after an
  imported call reproduced a label at consumer byte 85 instead of the reference
  at byte 233. The deferred inclusion carried its definition's line/column pair
  into the consumer source. All scheme/annotation instantiation paths now require
  a local region and use it for the instantiated constraints. Local-function
  and imported-function regressions check exact locations; the new reviewed
  no-color driver snapshot contains the actual Alder source and highlights only
  `utils.combine`. Direct definition checks retain their original local regions.
  Validation: full workspace tests pass (187 solver integration tests and 93
  driver tests), strict all-target/all-feature Clippy passes, and formatting/
  diff checks pass. One new reviewed snapshot; no pending snapshot files.

- Exact error-union checkpoint: inferred `?` result rows now retain exact-target
  metadata through schemes and owned interfaces. Stable lower bounds determine
  concrete union closure without closing declared open rows. Annotation-free
  exhaustive matching passes locally, in a lambda, and across a module boundary.
  An additional regression exposed equality of an unknown returned value with
  the accumulated output; returned values and bare error variables now contribute
  directional inclusions instead. Module-final existential simplification covers
  local lambdas while preserving binding/obligation/universal dependencies.
  Full workspace tests pass (186 solver integration tests, 92 driver tests,
  runtime and CLI suites), as does strict all-target/all-feature Clippy. The
  extended CLI errors fixture executes imported left/right failure and success
  cases with an exhaustive error match. Formatting and diff checks pass. No
  expected snapshot changes or pending snapshots. The earlier failing union and
  pipe snapshot checkpoints below are historical and now resolved. Follow-up
  remains for cyclic/SCC/higher-order cases, aliases, diagnostic source attribution,
  and the broader hardening acceptance matrix; see the error-row design note.

- Error-row bound-solving continuation: replaying known inclusion lower bounds
  after input instantiation now accepts the covering concrete union and rejects
  a missing source tag, both locally and through an imported interface. Flexible
  source tails under closed upper bounds are resolved only after known bounds
  stabilize. An overlapping-upper-bound test exposed lambda return equality;
  lambdas now use the named-function directional check. Scheme-local removal of
  hidden source-only existential tails restores the existing pipe/await snapshot
  unchanged. Full workspace tests passed with 182 solver tests and 91 driver
  tests, and strict Clippy passed. A subsequent adversarial regression adds the
  still-failing annotation-free exhaustive-union case (183rd solver test): all
  known cases are matched but an unrelated open tail remains. Current work is
  therefore not fully green or ready to commit. Preserve this test and finish
  exact inferred-union semantics without closing intentionally open contracts.

- Error-row inclusion continuation: solver schemes now collect connected
  inclusion relationships and preserve their variable identity through fresh
  instantiation and annotation publication. Permanent solver and driver tests
  exercise real generated metadata, local helper calls, binary storage,
  hydration, and arena copying. Universal-contract checking follows directed
  tail-inclusion paths and now rejects the confirmed direct-return and `?`
  independent-universal counterexamples; shared-tail widening still passes.
  This is not complete: independent-source concrete union inference still
  fails, and the wider solver suite exposes a hidden quantified parameter in
  the existing pipe/await snapshot. Do not accept that snapshot or commit this
  checkpoint as finished. See `docs/error-row-inclusion-hardening.md` for the
  remaining solving, simplification, and imported-call acceptance work.
  Validation: strict all-target/all-feature Clippy passes; all 90 driver tests
  pass. The full workspace run reaches solver integration with 178 passing and
  two failing tests (the concrete union and unchanged pipe/await snapshot).
  This also verifies rejection through an inferred helper's instantiated
  inclusion metadata. The generated `.snap.new` was removed without accepting
  it; the original expected snapshot remains unchanged. No commit yet.

- Record-row investigation: added four regression tests to active solver
  integration tests. Multi-field inference and the documented row-preserving
  rename/spread example are rejected; discarding an explicitly promised row
  and passing an optional record field to a required reader are accepted.
  All four new tests fail as expected before implementation. The false-row
  promise is checked at the declaration alone so rejection at a later caller
  cannot conceal unchecked universality. Inspected active conversions,
  access/spread, unification, substitution, generalization, and publication,
  plus Elm's unifyRecord/gatherFields. Added `docs/record-rows-internals.md`
  with the required representation/boundary changes. No solver fix has landed
  for these tests yet; the worktree intentionally contains failing regressions.
- Record-row implementation checkpoint (uncommitted): replaced Boolean tails
  with typed variable tails and a distinct record-fragment wrapper. Added
  residual-row unification, open-tail field access, single-spread preservation,
  tail-aware traversal/instantiation/publication, and normalization of empty
  row fragments. Original CLI records probe now exits successfully. Three of
  the four original row regressions pass; optional-to-required flow still fails.
  New independent-instantiation and shared-tail-incompatibility tests pass.
  Last complete solver integration run: 145 passed, one expected outstanding
  optional regression failed; two subsequently added focused tests also pass.
  Strict solver all-target/all-feature Clippy passes. No snapshots changed.
  Multiple open spread semantics, lacks/shadowing, kind consistency, trait
  matching, and cross-module tests remain required. Work remains uncommitted.
- Optional compatibility checkpoint (uncommitted): annotated let bindings now
  retain their declared shape; a new positive local/global construction test
  failed before that fix. Deferred directional presence checks now cover calls,
  annotated bindings, assignment values, and returns, rejecting the original
  optional-to-required counterexample. Container/nested-payload invariance
  rejects field weakening through shared arrays. Contextual checking of fresh
  array literals preserves safe annotated construction; both cases have tests.
  All 151 solver integration tests pass without snapshot updates. Five temporary
  snapshot failures exposed reversed actual/expected call diagnostics; restored
  diagnostic unification order and removed the generated pending snapshots.
  Remaining work includes branch-result presence, bare pipe destinations,
  lambda returns, optional field writes/reads in codegen, deeper contextual
  construction, and the rest of the row acceptance matrix. No completion claim.
- Presence-boundary follow-up (uncommitted): negative regressions reproduced
  bypasses through bare pipe destinations and annotated lambda returns; both
  now use compatibility checks. Branch joining promotes common optional fields
  in the resulting record rather than selecting the first branch's presence.
  Tests cover both if orders, match arms, and rejection by required-field
  readers; all 155 solver integration tests pass and strict solver Clippy passes.
  CLI validation is currently red: existing `modules` fixture passes a fresh
  nested record literal to an optional-field parameter and is over-rejected.
  Extend contextual construction to call arguments and nested records instead
  of weakening mutable-alias checks. Added `examples/records` and registered
  it for execution to cover renamed row tails and optional missing/present reads.
  Running that new fixture directly compiles but exits with an assertion
  failure. Code inspection shows Access still emits raw member reads while
  option.none is null; investigate omitted-field undefined versus Option
  representation, and nested/nullable present payloads, in the codegen seam.
- Contextual construction/runtime checkpoint: fresh record fields now receive
  recursive expected types, and fresh record/array call arguments receive their
  parameter context. This fixes the existing modules fixture without weakening
  alias invariance. The records runtime failure was fixed by carrying optional
  read sites in SolveOutput and emitting direct Oxc calls to `$optionalField`.
  Its own-property check distinguishes absence from present None/unit payloads.
  Extended CLI fixture verifies nullable nesting and exactly-once record
  evaluation; kernel regression verifies absent/null/unit and getter count.
  All standalone CLI fixtures now execute successfully. Broader row, pattern,
  write, kind, and cross-module acceptance work remains open.
  Checkpoint validation: full workspace tests (155 solver integration tests,
  14 kernel tests, CLI fixtures), strict all-target/all-feature Clippy,
  formatting, and diff checks pass. No snapshots changed or remain pending.
  Added a solve/codegen/kernel changeset. Changed-crate release packaging and
  the complete acceptance audit remain open.

- Task-frame checkpoint: the original compiled 20,000-deep await probe and a
  permanent kernel regression both failed with RangeError before the fix.
  Task iteration now yields one Call operation. Each fiber owns an explicit
  stack of caller iterators, driving child entry, success, and thrown failure
  through the budgeted scheduler loop. Existing direct-AST `yield* task`
  emission remains valid and no sequential-await child fiber is introduced.
  Kernel tests exercise 20,000 calls before suspension, suspension at every
  level, task reuse/laziness, same-fiber execution, and deep defect/interruption
  unwinding with suspending finally blocks and exactly-once scope finalization.
  Compiled async fixtures add deep success, Promise suspension, and typed
  Result propagation. Rechecked pinned Effect continuation-stack/Iterator
  code; no code copied. Broader lifecycle and provider-context audit remains
  open, along with the other hardening acceptance gates.
  Validation: the original deep CLI probe now exits successfully. All eleven
  kernel tests, full workspace tests including CLI execution, strict Clippy,
  formatting, and diff checks pass. Kernel packaging verifies successfully.
  No snapshots changed or remain pending. Final changed-crate packaging and
  the full acceptance audit remain open.
- Operation-defect checkpoint: a null-operation regression confirmed that
  directly closing the fiber skipped nested generator finally blocks. A
  throwing operation-tag getter also escaped into the host event loop. Invalid
  operations and synchronous handler exceptions now resume the yielding
  iterator with throw, preserving suspended cleanup and caller unwinding.
  Queue bookkeeping is restored in finally. Tests include null operations,
  a throwing tag getter, a Mask-handler coercion failure, suspending cleanup,
  and unrelated concurrent work completing normally. Revisited the pinned
  Effect interpreter exception boundary; Alder uses iterative throw resumption
  rather than recursive interpreter reentry, and no code was copied.
  Validation: all thirteen kernel tests, full workspace tests including CLI
  execution, strict Clippy, formatting, and diff checks pass. Kernel packaging
  verifies successfully. No snapshots changed or remain pending. Broader
  runtime and compiler acceptance work remains open.

- Scheduler-budget checkpoint: the original Node probe printed `false` for
  timer progress across 10,000 fulfilled Promise awaits. A permanent bounded
  runtime regression also failed with `promise starvation`. Ready work now
  uses a shared FIFO drain and 1,024-generator-step budget, replenished only
  after a host-timer yield rather than on each fiber resumption. Tests cover
  fulfilled Promises, completed joins, fork/join, all, race, and runs of fresh
  finalizer fibers; timer-driven interruption checks exactly-once finalization.
  The original probe now prints `true`. Rechecked the pinned Effect v4 commit
  `bd393d63c19bdd0ab212d95576cec89051c8501c`, specifically interpreter runLoop
  and MixedScheduler; no code copied. Stack-safe task composition, bulk work
  inside individual runtime operations, and deeper cleanup audit remain open.
  Validation: all eight kernel tests, full workspace tests including CLI
  execution, strict all-target/all-feature Clippy, formatting, and diff checks
  pass. `cargo package -p alder-kernel --allow-dirty` packages and verifies
  successfully. No snapshot changes or pending snapshot files. Remaining
  changed-crate packaging and final acceptance gates stay open.
- Partial child-construction cleanup: a bounded regression confirmed that
  `createChildren` could strand already-owned but unstarted children after a
  later task factory threw. Interrupted partial children now start solely to
  reach terminal exits; all/race wait for those exits before propagating the
  original failure. Tests cover both combinators, escaping and caught failures,
  no child-body execution, empty child ownership at recovery, and exactly-once
  parent finalization. Rechecked pinned Effect interpreter completion and child
  interruption middleware for the ownership-before-completion invariant; no
  source copied. Broader lifecycle cases remain open.
  Validation: all nine kernel tests, full workspace tests including CLI
  execution, strict Clippy, formatting, and diff checks pass. Kernel packaging
  verifies successfully with `cargo package -p alder-kernel --allow-dirty`.
  No snapshots changed or remain pending. Other acceptance gates remain open.

- Conditional-exit checkpoint: new solver regressions reproduced breaks in
  `false && ...`, `true || ...`, and false-guarded match arms incorrectly
  constraining live loop results. Inference now respects those reachability
  boundaries, including non-continuing scrutinees/guards and binary left
  operands. Structural flow excludes skipped exits and recognizes required
  Boolean operands, preserving divergence and unconditional operand returns.
  Negative tests retain constraints for unknown Boolean conditions/guards.
  CLI fixtures execute skipped and required operands and guarded loop exits.
  Pattern selection and general expression-sequencing reachability remain
  open; this is not complete path-sensitive analysis.
  Validation: all 142 solver integration tests, full workspace tests (including
  executed CLI fixtures), strict Clippy, formatting, and diff checks pass.
  No snapshots changed or remain pending; final packaging stays open.

- Loop-result checkpoint: positive valued/nested/async loops and negative
  incompatible break/statement-loop cases failed before target-stack inference.
  Each loop now owns a result type; bare breaks supply unit and while/for
  targets require unit. Lambda inference clears enclosing loop targets.
  An additional failing regression exposed dead breaks constraining live results;
  block sequencing and literal conditional reachability now isolate those exits.
  All 138 solver integration tests pass. Executed CLI fixtures verify nested
  String/Number loops, enclosing early return, statement-loop isolation, async
  loop values, unit breaks, and exactly-once payload effects. Match guards and
  general expression-sequencing reachability remain open, as does the broader
  flow/diagnostic acceptance matrix.
  Validation: full workspace tests, strict all-target/all-feature Clippy,
  formatting, and diff checks pass. No snapshots changed or remain pending.
  The original `loops` CLI counterexample now executes successfully and prints
  42. Release packaging remains a final acceptance gate.

- Control-flow checkpoint: four solver regressions failed before the change:
  zero-iteration loops could satisfy a return contract, all-return branches and
  lambdas were rejected as unit, and diverging loops required a fake value.
  The canonical AST now has an explicit structural flow summary; active
  inference distinguishes no continuation from unit and checks possible
  fallthrough. Added a dedicated source-aware missing-return diagnostic with
  a reviewed colorless snapshot. Regression extensions cover async functions,
  nested loops, reachable breaks, unreachable literal branches, and lambda
  scope boundaries. Two flow-algebra tests cover composition and loop exits.
- Executed `control_flow` CLI fixtures confirmed ordinary and async branch
  returns, then exposed discarded loop-tail effects and discarded break-payload
  effects. Both bounded failures were reproduced before fixing codegen.
  Loop tails now execute for effects; statement-loop scopes no longer borrow
  an enclosing value-loop result slot. Valued-break typing and the full flow
  audit remain open. Updated canonical documentation to describe active
  inference instead of obsolete rank-based constraint claims.
  Validation: full workspace tests (135 solver integration tests, 86 driver
  tests, CLI execution fixtures), strict Clippy, and formatting pass. The
  original returns probe now exits 1 with `alder::type::missing_return`. The
  new diagnostic snapshot was reviewed and accepted; no pending snapshots
  remain. Final release packaging and the rest of the flow matrix stay open.

- Extern package/diagnostic checkpoint: a temporary path-dependency fixture
  executes the package's Promise wrapper returning 42 even though the application
  has a same-named wrapper returning 99. Removing the dependency wrapper then
  reproduced the missing-declaration-snippet problem. Source text and extern
  regions now survive into bundling; resolution failures return shared
  `alder-report` diagnostics, and CLI conversion retains their labels. The
  integration test verifies the actual dependency declaration appears in the
  rendered error. A reviewed no-color bundle snapshot covers the declaration,
  import specifier, physical path, and resolver explanation.
  Validation: full workspace tests (ten CLI tests, three bundle tests), strict
  Clippy, and formatting pass. The new snapshot was explicitly reviewed; no
  pending snapshots remain. Final release packaging remains an open gate.

- Local extern checkpoint: the new permanent `externs` CLI fixture failed to
  resolve `./client.js` before the fix. Emitted modules now retain physical
  source paths and the bundler delegates foreign resolution using that importer.
  The AST handoff remains direct; virtual IDs survive consuming their ASTs.
  CLI execution now covers sibling/nested wrappers, a wrapper importing its
  parent JS module, fulfilled Ok/Err data, raw rejection, synchronous throws,
  retained runtime defect context, and exactly-once AbortSignal cancellation.
  The original extern probe now runs successfully. Dependency-package wrappers
  and source-labeled declaration diagnostics for resolution failures remain
  required follow-up work; the current resolver error includes the source path
  and import specifier but not a labeled declaration span.
  Validation: full workspace tests, all nine CLI tests, strict Clippy, and
  formatting pass. The fixture also ran using the real CLI from `/tmp` with
  an absolute project path. Its generated cache was moved out of the repository
  after the manual check. No snapshots changed or remain pending. Final package
  verification against the pending inter-crate release set remains open.

- Workspace application regression reproduced two distinct members both being
  assigned `Application`. Workspace-aware package mapping now assigns opaque
  `ApplicationMember` keys from the full relative member path. Keys are bounded
  SHA-256 hex strings so deep member paths do not exceed a cache path segment's
  length limit. The regression checks distinct graph edges, successful Number
  versus String calls to same-named local modules, separate emitted IDs, four
  distinct interface-cache paths, and two instance indexes. Reordering members
  and relocating the workspace preserve package identities. Standalone app
  identities are unchanged. External members and overlapping roots remain open.
  Validation: the regression failed before the fix, then passed after it;
  full workspace tests (85 driver tests), strict Clippy, and formatting pass.
  No snapshots changed or remain pending. Final release packaging is still open.

- Module identity checkpoint: duplicate-source regression failed before the fix.
  Preflight now rejects conflicting package-qualified identities before any
  interface discovery, with a reviewed colorless diagnostic labeling both
  source files. The original CLI duplicate probe now exits 1 with that error.
  Project builds supply explicit source-root-relative module paths, and CLI
  check/build use a graph resolver sharing those identities with compilation.
  A positive test checks graph edges, interfaces, and emitted IDs for equal
  module paths in two packages, repeated/nested `src` directories, package-root
  imports, and reversed discovery order. Root imports now target the documented
  empty-path `mod.ald` identity instead of a package-named child module.
  Workspace application identities, overlap handling, low-level source-only
  API fallback paths, cache validation, and full CLI package execution remain
  audit work; these changes do not close the whole module-resolution finding.
  Validation: all 84 driver tests, full workspace tests (including existing
  CLI package execution), strict all-target/all-feature Clippy, and formatting
  pass. One new duplicate diagnostic snapshot was reviewed and accepted; no
  pending snapshots remain. Packaging against the final pending release set
  remains a final gate.

- Formatter checkpoint: 12 formatter tests, 1,292 parser tests, and the CLI
  no-write regression pass. A further CLI test applies a real formatting change,
  runs format-check, bundles the application, and executes exact template-value
  and length assertions. Full workspace tests pass; the additional execution
  test also passes separately. Strict Clippy and formatting checks pass.
  `alder-parse` package verification passes; `alder-fmt` package verification
  passes with pending local parser/source/region dependency patches. No
  snapshots were changed or left pending. Remaining formatter audit is tracked
  above rather than inferred complete from these regressions.

- Baseline review at `21994e0`: eleven defects reproduced through CLI or direct
  kernel execution; temporary probes remain under `/tmp/alder-review.D3XIzP`.
- Start: clean `main`, dedicated branch created; saved objective read in full.
- Active inference is `alder-solve/src/inference.rs`; `alder-constrain` currently
  packages the canonical module and collects trait requirement seeds. Old files
  not declared in crate roots do not participate in compilation.
- First contract regressions: five invalid signatures were accepted before the
  fix. Ten tests now cover concrete specialization, independent universals,
  defaults, impl signatures/bodies, HKT specialization, mutable escape through
  a typed extern, and recursive peers. Full contract audit remains open.
- Separate deferred universal obligations now validate unbound/distinct roots
  and escape after inference, before solved interfaces can be published.
- Two colorless driver snapshots cover specialization and generic escape.
- Method-bound audit reproduced both an accepted implementation-only bound
  (missing dictionary at runtime) and a rejected inherited method bound.
  Method givens now come from the trait ABI; implementation bounds are proof
  obligations. Seven solver regressions cover extra/unused bounds, inherited
  bounds with renamed variables, reordered dictionary arguments, and rejected
  extra versus accepted inherited projection equalities, and associated return
  signatures. The traits
  CLI fixture exercises inherited and reordered bounds with actual execution.
- Method-contract checkpoint: formatting, strict all-target/all-feature Clippy,
  full workspace tests (107 solver integration tests), and the explicit CLI
  standalone fixture suite pass. The reviewed specialization snapshot now
  labels the method name rather than its entire impl. No pending snapshots.
- Lambda scope audit also found canonicalization rejected all lowercase names
  in lambda annotations. It now admits annotation variables, while inference
  explicitly enters/restores the enclosing signature scope. Six regressions
  cover shared generics/bounds, nested scopes, siblings, separate functions,
  and higher-kinded annotations. A reviewed colorless diagnostic snapshots
  specialization via an unused lambda; the CLI trait fixture executes a
  lambda using its enclosing dictionary for Number and String.
- Lambda checkpoint validation: formatting, strict Clippy, and full workspace
  tests pass (113 solver integration tests and 75 driver tests, plus CLI
  execution fixtures). No pending snapshots or whitespace errors.
- Stdlib checkpoint: four invalid builtin calls reproduced before the fix.
  Canonicalization now loads cached package-local `.ald` signatures into
  annotated foreign references, in an isolated builtin namespace. Seven solver
  regressions cover arguments, callbacks, arity, indirect use, and inferred
  result contracts. Two reviewed driver diagnostics cover invalid arguments
  and unknown members. Two package-source tests check parity and every module's
  signatures; added the missing `Ref.ald` declaration.
- Corrected call mismatch orientation: actual arguments precede the expected
  callee type. Reviewed five updated solver snapshots, including previously
  stale source descriptions; program acceptance did not change for those cases.
- `cargo package -p alder-can --allow-dirty` includes the embedded sources but
  plain registry verification cannot yet use the unreleased async AST field
  `abort_signal`. The existing `async-fibers` changeset already bumps that AST.
  Tarball verification succeeds with explicit local patches for alder-ast,
  alder-region, alder-source, and alder-parse. Release-version verification
  remains part of the final gate; no versions or tags were manually changed.
- Direct kernel probes additionally confirmed `json.decode("42")` returns
  `Ok(42)` without target-type validation and `map.get` returns identical null
  values for a present None and an absent key. These runtime-contract defects
  remain required follow-up work, not covered by signature loading alone.
- Stdlib validation: formatting, strict Clippy, full workspace tests (120 solver
  integration tests, 77 driver tests), and CLI execution fixtures pass. The
  package tarball verifies with the pending local dependency set. No pending
  snapshot files remain.
- Option audit: reproduced nested equality and map-result collapse in permanent
  kernel tests. All payload consumers now unwrap once; map/apply/traverse and
  map.get rewrap successful payloads. Some of an existing box adds a layer;
  private box identity avoids collisions with user enums named Some.
- Four kernel regressions cover equality/symmetry/hash consistency, show,
  layers, unit, mapping/applicative/monadic/traversal behavior, map presence,
  and nested JSON round trips. The nullable JSON codec now uses an escaped
  singleton `$alderSome` envelope, with collision escaping; simple non-null
  encodings are unchanged. Actual CLI trait fixtures verify nested options,
  derives, hashing, showing, JSON, map lookup, and a user Some enum payload.
- Option pattern/optional-field and ordering audits remain open; current solver
  intrinsic selection has no builtin Ord[Option]. The unrelated unchecked
  json.decode entry point also remains open.
- Option checkpoint validation: formatting, strict Clippy, full workspace tests,
  explicit CLI fixtures, and plain `cargo package -p alder-kernel --allow-dirty`
  verification pass. The original `/tmp/alder-review.D3XIzP/option` counterexample
  now runs successfully through the CLI. No snapshots changed or remain pending.
- Mutation checkpoint: reproduced incompatible instantiations of shared arrays,
  maps, and captured state. Top-level calls/aggregate construction now remain
  monomorphic; safe function/lambda/reference/scalar values retain generalization
  after subtracting free environment variables. Restricted SCC members contribute
  free variables before any peer is generalized. JS aliasing is unchanged.
- Reproduced a second interface hole: annotations quantified every serialized
  variable even for monomorphic schemes. Only actual quantified variables now
  appear as parameters, and exports with unresolved shared types are rejected
  with a reviewed source-aware diagnostic suggesting a concrete type or factory.
- Eleven solver regressions cover arrays/maps/sets, aliases, nested records,
  captured state/tasks, export completeness, and polymorphic fresh factories.
  A driver regression rejects an incompatible use across an actual module
  interface. Additional SCC and variance/aggregate cases remain to audit before
  calling the entire mutation requirement complete.
- Mutation checkpoint validation: formatting, strict Clippy, and the full
  workspace suite pass (131 solver integration tests, 79 driver tests). The
  original shared-array CLI reproduction now fails at the incompatible String
  annotation. No pending snapshots or whitespace errors remain.
- Formatter checkpoint replaces backtick guessing with parser-produced verbatim
  byte ranges for templates/interpolation, markup, raw macros, and comments.
  Backtracking rolls the range table back transactionally. Protected lines and
  their line endings remain byte-identical; payload delimiters are masked from
  indentation counting. No global CR/CRLF replacement remains.
- Output validation compares raw protected slices and physical token lines,
  besides reparsing/comments. Tests compare actual cooked literal values and
  separately check idempotence. CLI no-write-on-invalid-input and parser
  lookahead regressions cover the safety boundary. Broader adversarial formatting
  and final CLI execution checks remain in the final audit.
- First checkpoint validation: formatting, strict Clippy, and full workspace
  tests pass (100 solver integration tests). The original trait reproduction
  now fails at CLI check with `alder::type::generic_specialization`, before JS
  can execute. Other hardening findings are not claimed fixed.

## Final gates

Fresh post-record-migration package verification passes all 17 publishable
crates in `/tmp/alder-package-records.FkiXbJ`, with extracted source comparisons,
unchanged Cargo.lock, and packaged-CLI execution of all four examples and four
integration projects. Unreleased record changeset descriptions now match
ordinary Option semantics rather than obsolete presence/fallback behavior.
This is dirty-tree local-host evidence; clean committed delivery and the full
acceptance review remain open. See `docs/release-packaging-hardening.md`.

Latest original-counterexample refresh: rebuilt the workspace CLI after the
record/Option migration and ran fresh source-only copies in
`/tmp/alder-recheck.TECPbq` from `/tmp`. All eleven reported manifestations have
the expected outcomes, including actual formatter editing, Promise timer
progress, and 20,000-deep awaits. All four repository examples execute as well.
See `docs/hardening-current-verification.md`. This is current worktree evidence;
the final-tree rerun gate still applies if subsequent production changes land,
and packaging/clean commits/the broader acceptance audit remain open.

- Error-inclusion representation work: added canonical/owned annotation metadata
  carrying source row, target row and source region, with lossless arena copy
  and binary serialization/hydration. Bumped interface format to 3. The explicit
  metadata round-trip test passes after destroying the source arena; workspace
  `cargo check`, strict all-target/all-feature Clippy, formatting and diff checks
  pass; all 90 driver unit tests pass. This is infrastructure only: active solver scheme
  generation, instantiation and constraint solving remain unimplemented, and
  the three directional-inclusion regressions remain expected failures. No
  completed fix or green workspace is claimed; changes are uncommitted.

- Directional error-row audit: confirmed declaration-level acceptance of
  arbitrary tail replacement (`e` to unrelated universal `f`) for direct return
  and `?`. Added failing negative regressions and a failing positive regression
  for precise union of independent source tails. Shared-tail widening remains
  a passing positive control. See `docs/error-row-inclusion-hardening.md` for
  the source-tail-loss root cause and required scheme/interface preservation.
  These five tests are uncommitted baseline work (three failures); no production
  fix or green workspace is claimed at this point.

- Error-row alias follow-up: reproduced generic Result identity rejection both
  directly and through an alias (`GenericSpecialization e = [_]`). Error-row
  unification constructed empty residual wrappers around fresh shared tails.
  Empty residuals now bind directly to the shared variable, preserving universal
  identity. Regressions cover open/concrete aliased tails, propagation, wrong
  payloads and unlisted tags; added imported CLI execution. Full workspace tests
  pass (173 solver integration tests), as do strict Clippy, formatting and diff
  checks. No snapshot changes or pending snapshot files.
  Next soundness check: `include_error_rows` currently returns success for an
  open target when residual source tags are empty without constraining a source
  tail, and constructs a fresh target extension otherwise. Investigate whether
  arbitrary source/target tails can be incorrectly treated as unrelated; do not
  treat the identity fix as proof of directional inclusion soundness.

- Alias follow-up: reproduced rejection of an AbortSignal extern returning a
  chained Task alias; the canonical Task check now unwraps alias targets, as
  codegen already did. Both canonicalizer and executable Promise-backed CLI
  regressions pass. Added passing tests for open/concrete record-row arguments,
  capture avoidance with swapped caller/declaration variable names, and rejection
  of generic specialization or incompatible payloads through aliases. Core
  checkpoint validation passes: full workspace tests (170 solver integration,
  63 canonicalizer and 89 driver tests), strict all-target/all-feature Clippy,
  formatting and diff checks. One existing inference snapshot was deliberately
  corrected and one new no-color diagnostic snapshot reviewed; none are pending.
  Higher-kinded/error-row/interface acceptance work
  remains explicit in `docs/type-alias-hardening.md`.

- Alias expansion checkpoint: registered imported definitions by canonical
  identity and local definitions in dependency order; type references now emit
  instantiated Filled alias targets. Added capture-avoiding canonical type
  substitution. All three initial solver regressions pass, and the CLI records
  fixture now executes imported optional and generic aliases. Reviewed the one
  changed existing snapshot (Wrapped now correctly expands to Result). Strict
  Clippy and full workspace tests pass (167 solver integration tests); acceptance work listed
  in `docs/type-alias-hardening.md` remains open. Changes remain uncommitted.

- Alias implementation in progress: added iterative dependency-first alias
  ordering and cycle rejection before enum/trait canonicalization in both full
  and header-only modes. All 62 canonicalizer tests pass, including direct and
  indirect cycles and shared dependency ordering. Reviewed the new no-color
  recursive-alias diagnostic snapshot. Alias expansion/substitution is still
  unimplemented, so the three new solver regression tests remain failing.
  The worktree is intentionally uncommitted pending a coherent green fix.

- Alias investigation baseline: added intended-contract solver regressions for
  optional record aliases, generic forward-reference chains, independent
  instantiations, and generic identity aliases, plus a canonicalization test for
  recursive aliases in full/header-only modes. They are failing regressions,
  not completed fixes. Read `docs/type-alias-hardening.md` for the traced active
  path, Elm references, and required substitution/interface work. No production
  code has changed in this baseline; do not commit it as a completed green fix.

- Cross-module record evidence: the records CLI fixture now imports a separate
  `rows` module, independently instantiates a row-preserving rename function,
  reads two inferred fields, preserves two independent input/output tails,
  and reads an inferred optional result. A driver interface test serializes to
  bytes after dropping the source arena, deserializes/rehydrates, and checks
  distinct tail names, corresponding output tails, and optional field presence.
  Negative driver cases check incompatible shared tails and an imported optional
  field used as Number. Full workspace tests pass (88 driver tests), as do
  strict all-target/all-feature Clippy, formatting, and diff checks. No snapshot
  changes or pending artifacts. This test/documentation checkpoint changes no
  published behavior and needs no additional version bump.
- Newly confirmed alias defect (unresolved, not waived): the documented source
  below fails with `expected { name: String }, found OptionalName`:

  ```alder
  pub type OptionalName = { name?: String }
  pub fn choose(flag: Bool, record: OptionalName) {
      if flag { { name: "present" } } else { record }
  }
  ```

  `alder-can/src/types.rs` translates named references unconditionally to
  `CanType::Named`; the active solver skips TypeAlias declarations. It can
  expand existing `Type::Alias` nodes but these references never become such
  nodes. Investigate local/imported alias expansion, generic substitution,
  declaration ordering and cycles next. The cross-module record test uses
  structural types to isolate row-interface coverage, not to waive aliases.

- Loop/record interaction follow-up: reproduced first-break-order dependence
  that accepted an absent optional field as a `Number` and rejected the correct
  `Option[Number]` result. Ordinary unification retained the first record's
  presence map. Reachable breaks now update their loop frame with `join_values`,
  and loop-body inference returns that accumulated exit type rather than the
  body type or original result variable. Solver tests cover both orders and
  reject the required-value escape; CLI cases cover both paths and a nested
  value-producing loop. Full workspace tests pass (164 solver integration
  tests, including existing loop/divergence cases, and the CLI fixtures).
  Strict all-target/all-feature Clippy, formatting, and diff checks pass; no
  snapshots changed or pending snapshot files remain.

- Optional record-pattern follow-up: reproduced acceptance of a `Number`
  return extracted from an absent optional field, alongside rejection of the
  correct `Option[Number]` return. Regular record patterns invented required
  fields and symmetric unification erased the presence distinction; enum
  record patterns likewise bound raw payload types. Both now use field-read
  typing, with optional pattern field regions carried to codegen. Binding and
  matching paths use the centralized `$optionalField` helper. Focused solver
  tests pass; CLI regressions cover absence, present nested None, enum payloads,
  and pinned Option comparisons. Full workspace tests pass (162 solver
  integration tests); strict all-target/all-feature Clippy, formatting, and diff
  checks pass. No snapshot changes or pending snapshot files. The wider row and
  effectful pattern-evaluation audits remain open.

- Optional-field assignment follow-up: reproduced two opposite errors in the
  active solver: `record.value = 42` was rejected for `value?: Number`, while
  `record.value = option.some(42)` was accepted. `place_type` reused read typing
  although lowering writes the raw payload. Final-field assignment now uses
  the declared payload type; intermediate fields retain read typing, so an
  absent optional parent cannot be traversed. Four focused solver regressions
  pass after failing the two affected cases before the fix. Added CLI coverage
  for absent-to-present writes, nested Option payloads, and a required parent.
  Adversarial follow-up exposed an initial-fix regression allowing `+=` on an
  absent optional field; compound assignments now also check read/write type
  compatibility, and the new negative regression proves that rejection.
  Checkpoint validation: full `cargo test --quiet` passes (159 solver integration
  tests, 14 kernel tests, and all CLI fixtures); strict all-target/all-feature
  Clippy, formatting, and diff checks pass. No snapshot changes or pending
  snapshot files. Optional patterns and other record-row acceptance work remain
  open.

- [x] Re-run every original reproduction against the final compiler.
  Packaged CLI at `d9e7699`: expected rejection diagnostics for generic/array/
  return/duplicate probes; success for records, loops, nested Option, local
  externs, 20,000 awaits, and template payload before/after a real formatting
  edit. Extracted-kernel timer probe passes. See release-packaging-hardening.
- [x] Adversarial review of alternate forms and cross-feature interactions.
  Final report links the completed generic/mutation/row/Option/dictionary,
  control-flow/evaluation-order, runtime, module and source-boundary matrices.
- [x] `cargo fmt --all` and strict all-target/all-feature Clippy.
  Refreshed after rebuilding parser/report binaries; session 21565 exits 0.
- [x] Full `cargo test`, snapshots reviewed, no pending/stale artifacts.
  `cargo insta test --check --unreferenced reject -- --quiet` exits 0 in session
  6089 after clearing stale parser/report build artifacts with embedded temporary
  checkout paths. All tests/doctests pass, with no unreferenced or pending
  snapshots. No source/snapshot deletion was needed.
- [x] Actual CLI build/run/test, affected examples, runtime bounded regressions.
  Packaged CLI runs all four examples and 17 integration projects from fresh
  source/config-only copies, with 45-second subprocess bounds and expected
  failing test-mode exit. The full kernel suite covers fairness and cleanup.
- [x] Release package verification for affected crates.
  Clean-tree, offline workspace packaging verifies all 17 crates without
  `--allow-dirty`; 1,517 extracted source/stdlib/kernel files match exactly.
- [x] SPEC/design docs reflect behavior; per-crate Sampo changesets.
  Current syntax, Option contracts, explicit async, structural capabilities and
  iterator semantics match source/tests. Every touched publishable crate has a
  Sampo entry; packaged stdlib copies match. Historical logs are identified.
- [x] Clean committed branch; final requirement-by-requirement evidence audit.
  `docs/compiler-hardening-final-report.md` reconciles the original objective
  and amendments against inspected source, permanent tests and clean package
  execution. Completion documentation is the only post-verification change.

- Expression evaluation-order follow-up: the shared `values` lowering helper
  collected every operand's setup before evaluating any final expressions.
  A CLI array regression reproduced later block effects running before an
  earlier call. The helper now materializes operands preceding later setup,
  preserving left-to-right evaluation without cloning referenced values.
  CLI coverage includes arrays, tuples, call arguments, an early return that
  must retain preceding effects and skip later operands, scalar mutation, and
  mutable-array alias preservation. The helper also lowers tag payloads;
  dedicated tag and template coverage remains open. All 39 codegen snapshot
  tests and strict all-target/all-feature Clippy pass without snapshot changes.
  Full workspace tests, including the final doctest process exit, and the CLI
  regression run pass. Formatting and diff checks pass; no pending snapshots.
  The wider expression/pattern ordering audit is not complete.

- Template follow-up: reproduced later interpolation setup running before
  earlier effects in both ordinary and tagged templates. Both now use the
  operand sequencing helper; ordinary templates sequence string conversion,
  not just reference capture. CLI cases verify effect order and a mutable
  array converted before a later interpolation mutates it.
- Tagged-template contract follow-up: three solver regressions reproduced an
  accepted non-function tag, accepted wrong interpolation type, and rejected
  valid Number-returning tag. The solver previously skipped tag inference and
  unconditionally returned String. It now checks the emitted call contract
  (`Array[String]` followed by interpolation arguments), retains the real
  return type, and resolves resulting record overlays before field access.
  All three regressions pass; CLI coverage also executes independent generic
  instantiations, Show dictionary evidence, and a task-returning tag. Further
  cross-module and diagnostic snapshot coverage remains open.
- Tagged-template segment follow-up: the adjacent-interpolation CLI regression
  reproduced omission of empty leading, intermediate, and trailing strings.
  Lowering now accumulates literal text into a segment and emits a segment at
  each interpolation boundary plus the final segment. This is literal payload
  assembly into Oxc string nodes, not generated JavaScript source/reparsing.
  Checkpoint validation: full workspace tests including final doctest exit pass
  (247 solver integration tests, 39 unchanged codegen snapshots), as do strict
  all-target/all-feature Clippy, formatting, and diff checks. No pending
  snapshots. Template typing and segment regressions were observed failing
  before their respective fixes. This does not close the broader hardening
  acceptance audit or the remaining template cross-module/diagnostic coverage.

- Tagged-template boundary verification: a serialized dependency interface
  accepts independently instantiated identity tags and a Show-constrained tag;
  a wrong argument is rejected at the current consumer call. Added and reviewed
  the source-aware, colorless diagnostic snapshot. Actual CLI coverage imports
  generic, dictionary-constrained, task-returning, and record-overlay tags.
- Assignment ordering follow-up: a CLI regression reproduced an assignment
  incorrectly writing to a replacement root object installed by the RHS.
  Setup statements previously ran before resolving the target. Nontrivial
  targets now capture each receiver/index before subsequent target or RHS
  effects; compound assignments capture the old value before RHS setup.
  Indexed assignments no longer take the path rejecting lifted index setup.
  CLI coverage includes root replacement, nested index mutation, exactly-once
  index effects, old scalar payload retention, and generic Num dispatch.
  Reviewed two codegen snapshot changes: explicit receiver and old-value
  captures precede assignment; no dictionary ABI changes. The wider audit,
  including async assignment targets, remains open.
  Checkpoint validation: full workspace tests and final doctest exit pass,
  including all CLI fixtures, 111 driver tests, and 39 codegen tests. Strict
  Clippy, formatting, and diff checks pass; no pending snapshots remain.

- Assignment-index effects follow-up: a CLI case with suspension in both the
  index and RHS verifies captured receivers/old payloads survive suspension.
  Removing the RHS await exposed a separate defect: `contains_await_stmt`
  skipped assignment indices and published the function as synchronous. The
  scan now visits every computed index while retaining nested-lambda boundaries.
- Assignment-index return context follow-up: a Result-returning function using
  `?` in its assignment index was rejected with `invalid_try`; `place_type`
  discarded the enclosing return type. It now passes that context to index
  inference. CLI regressions verify failed indices skip the write and early
  returns preserve the original array. Solver negatives reject incompatible
  errors and return payloads; a positive nested-async-lambda case guards the
  suspension boundary. Runtime scheduler implementation was not changed.
  Checkpoint validation: full workspace tests, including final doctest exit,
  pass (250 solver integration tests and all CLI fixtures). Strict Clippy,
  formatting, and diff checks pass; no snapshots changed or remain pending.

- Bare-function pipe overlay timing: reproduced a valid optional-field read
  rejecting because this call path delayed overlay solving until after access
  had guessed required presence. It now discharges newly closed overlays like
  ordinary and tagged calls. Solver and imported CLI regressions pass,
  including absent versus present nested None. Updated the overlay plan to
  distinguish historical reproduction states from current acceptance gaps.
  Checkpoint validation: full workspace tests including final doctest exit pass
  (251 solver integration tests); strict Clippy, formatting, and diff checks
  pass, with no pending snapshots.
- Tuple-inference investigation queued: the initial pipe fixture used
  `fn merge_pair(pair) { merge(pair.0, pair.1) }`, which fails during inference
  of the second access before reaching the overlay. Destructured parameters
  isolate the overlay regression. Audit inferred tuple arity/access order
  separately; do not attribute that rejection to overlay solving.
  Reproduced after 0db756d with forward/reverse projection tests. Root cause,
  resource-risk finding, language-choice request, and acceptance coverage are
  recorded in `plans/tuple-projection-hardening.md`; the forward-order test is
  currently red pending the fix.

Argument-diagnostic follow-up: reproduced reversed expected/actual types when
a later contextual array literal specialized a generic parameter before an
earlier argument was checked. infer_call now checks known parameters from left
to right, including the leading pipe argument, before inferring the next argument.
The direct and piped regressions require expected Number/found String. Reviewed
Elm Reporting/Error/Type.hs CallArg's left-to-right rule. Eleven reviewed snapshot
changes narrow labels to offending arguments, correct Ref/SynchronizedRef's
direction, and retain the constructor arity rejection with a more specialized
expected signature. All 119 driver tests and strict workspace Clippy pass.
All 281 other solver integration tests pass; the separate tuple regression
remains red. Follow-up runs pass all 43 codegen and 11 CLI tests, including
standalone compilation/execution fixtures. This is scoped validation, not
completion of the wider compiler acceptance matrix.

Packaging checkpoint for the synchronization/runtime changes: plain
`cargo package -p alder-kernel --allow-dirty` verifies the extracted package.
`alder-can` package verification passes with explicit local patches for the
pending alder-ast/alder-region/alder-source/alder-parse dependency set. Inspected
the actual .crate archive: it contains stdlib/ref.ald, stdlib/semaphore.ald,
and stdlib/synchronized_ref.ald. Cargo.lock is unchanged. This does not claim
registry-only verification against versions not yet released; final release
dependency/version validation remains required.
The actual CLI also builds/runs examples/async and examples/pipes successfully
after the argument-checking change ("Async fibers are alive!" and "Pipe showcase
passed!"). Formatting/diff checks pass; no version, tag, or publication changes
were made.

JSON boundary checkpoint: reproduced Result container decoding passing JS
undefined to a String-taking child decoder when `_0` is missing. The decoder
now checks payload presence first; the permanent regression covers both tags,
zero child invocations on missing fields, and present null/unit decoding.
All 45 kernel tests pass. The attempted actual CLI extension exposed missing
derived error-group Json evidence through Result; reproduction and remaining
work are recorded in `docs/json-hardening.md`. No end-to-end Result JSON
completion is claimed, and the unsuccessful fixture extension was removed.

Finite regression coverage is evidence for these contracts, not a claim of
exhaustive compiler correctness. Do not mark unresolved items complete.

Error-row equality / JSON checkpoint: discovered that generated structural Eq
metadata omitted the runtime tag's leading colon, silently skipping every
error payload comparison. Codegen now preserves that prefix. A reviewed
source-aware emission snapshot and actual CLI unequal-first/second-payload,
equal/symmetric, and nested Option[Result] regressions cover the correction.
This invalidates the strength of earlier Err-message equality assertions alone;
the affected CLI suite has now been rerun with payload comparison active.
The fix exposed the intended failing unknown-JSON-tag assertion, now corrected
by requiring own variant-map entries instead of looking through Object.prototype.
The focused kernel regression covers inherited names and a valid-tag control.
The named-error-group/structural-row dictionary mismatch was traced but not
silently resolved; design tradeoffs and a user decision request are recorded in
`docs/json-hardening.md`.

Validation after these changes: actual traits CLI passes; full `cargo test`
passes CLI (11), codegen (44), driver (121), formatter (14), kernel (46), parser
(1304), and the preceding suites, then fails only the existing tuple-projection
regression (291/292 solver integration tests pass). This is not a green full
workspace result; subsequent doctests were not reached. Formatting/diff checks
pass, and no pending snapshots remain. Sampo changesets record both fixes.

Optional-payload checkpoint: four new kernel regressions reproduced absent
record fields reaching Eq/Ord/Hash payload dictionaries and present None being
omitted by Json. Runtime operations now check own-property presence first:
equality distinguishes absent from present unit/None, ordering puts absence
first, hashing skips absent fields while retaining declaration indices, and
JSON preserves present nullable fields. Closed-record and derived-enum CLI
regressions include symmetry, equal hashes for equal values, nested payloads,
unit/None round trips, and imported generic codec dispatch. Trait/language/JSON
docs and a kernel/CLI Sampo changeset record these rules.

Validation: all 50 kernel tests, 44 codegen tests, 121 driver tests, 14 formatter
tests, 11 CLI tests (including the updated actual traits fixture), and 1304
parser tests pass in `cargo test --quiet`; the run still stops at the known
tuple-projection failure (291/292 solver integration tests pass). Strict full
workspace Clippy and formatting/diff checks pass. Kernel packaging verifies
the extracted package; Cargo.lock is unchanged and no pending snapshots remain.
These checks do not resolve the pending tuple/error-row/traversal choices or
the remaining hardening acceptance work.

Option syntax checkpoint: an actual CLI reproduction showed the documented
Some/None constructors missing from the canonical builtin environment. Added
arena-owned constructor annotations and direct AST lowering through existing
Option helpers, including representation-aware nested tests and extraction.
Actual CLI coverage includes qualified constructors, first-class references,
unit/nested None, and a user enum also named Some. A reviewed source-aware
codegen snapshot checks nested lowering; solver tests reject incompatible
payloads. Full tests pass all preceding suites (45 codegen, 50 kernel, 121
driver, 11 CLI, 1304 parser), then stop only at the existing tuple-projection
failure (292/293 solver integration tests pass).

This exposed unchecked refutable pattern bindings outside match expressions:
payload extraction can violate checked types on a nonmatching input. Root
locations, an Option counterexample, and required general follow-through are
recorded in `docs/option-constructor-hardening.md`. Constructor registration is
not a claim that those binding sites are safe; fixing them remains required.
The recursive/cyclic runtime-value audit also remains open.

Refutable-binding checkpoint: reproduced a compiled `let Some(number) = None`
printing null and exiting successfully. All non-match binding callers now use
a shared checked-binding path before extraction; it reuses existing pattern
decisions and source-located matchFailure, while irrefutable bindings stay
direct. Actual CLI cases cover top-level/local lets, parameters, lambdas,
for bindings, array lengths, distinct enum variants, nested aliases, and async
parameters/bodies. Success controls verify exactly-once source evaluation and
lazy async parameter checks. A maintained local JS probe verifies finalization
once per failure across two executions of a reusable compiled task. Each run
has a bounded timeout. The new source-aware lowering snapshot was reviewed.

Validation: `cargo test --quiet` passes 12 CLI, 46 codegen, 121 driver, 14
formatter, 50 kernel, and 1304 parser tests, then fails only the known tuple
projection case (292/293 solver integration tests pass). Full strict Clippy,
formatting, and diff checks pass. Documentation and a codegen/CLI changeset
record the fix. The broader pattern/pin lexical-scope and evaluation-order
audit remains open as detailed in `docs/option-constructor-hardening.md`.

Pin-scope checkpoint: the actual CLI reproduced ReferenceError when a pattern
bound `value` before `^value`. Canonicalization now snapshots pre-pattern
lexical scopes for nested pins, preserving outer references independently of
field order while exposing new bindings to the guard/body. A pin with no outer
binding is an ordinary unknown-name error. Fresh identities and assignment
tracking remain shared with the containing environment. Tests cover tuple
orders, records, nested Option patterns, differently typed aliases, and one
scrutinee evaluation followed by exactly-once pins in source order. Reviewed
source-aware canonical and codegen snapshots demonstrate the scope distinction.

Validation: full strict Clippy, formatting/diff checks, and all preceding
workspace test suites pass (77 canonical, 12 CLI, 47 codegen, 121 driver, 50
kernel, 14 formatter, 1304 parser); the full test command still stops at the
known tuple-projection failure (292/293 solver integration tests pass). No
pending snapshots remain. Language/pattern docs and a canonical/CLI changeset
record the correction. Alternative-pattern identity and match-only permission
boundaries remain explicit audit work, not inferred complete from these tests.

Alternative-pattern checkpoint: reproduced ReferenceError when the second of
`First(value) | Second(value)` matched. Alternatives now share the first
pattern's local identities by name. Canonicalization rejects missing/extra
names; inference unifies matching binding types, including aliases and array
rests, rather than replacing a previous alternative's contract. Both expression
and markup match canonicalization use the shared mode. Actual standalone CLI
tests cover first/second alternatives, guards, rest patterns, aliases, and
escaping closures. Three solver negatives cover incompatible plain/rest/alias
payloads. Reviewed source-aware codegen and colorless missing/extra-name
diagnostic snapshots verify identities and source labels. Documentation and a
canonical/solver/driver/CLI changeset record the fix.

Validation: full tests pass the preceding suites (77 canonical, 12 CLI, 48
codegen, 123 driver, 14 formatter, 50 kernel, 1304 parser), then fail only the
known tuple projection case (293/294 solver integration tests pass). Formatting
and diff checks pass; no pending snapshots remain. Overlapping alternatives
with side-effecting guards and match-only permission boundaries remain audit
items; deferred markup execution is not claimed tested by the standalone run.

Match-permission checkpoint: reproduced a nested let pin being accepted merely
because its enclosing expression was a match arm. Replaced lexical match depth
with explicit match-pattern binding mode for ordinary and markup matches,
including alternatives. Pins and unqualified constructor lookup no longer
inherit match permission in unrelated nested bindings. Removed the unused depth
counter. Added reviewed source-aware let/parameter error snapshots and a positive
nested-match-inside-lambda test; all 80 canonical tests pass. This closes the
match-only permission leak above; overlapping alternative guards remain open.

Validation for this checkpoint: formatting, strict all-target/all-feature
Clippy, diff checks, and the actual CLI refutable-binding regression pass.
Full `cargo test` reaches solver integration with only the known tuple
projection failure (293/294 pass); no new failure or pending snapshot was found.
The branch remains uncommitted and is not ready to merge.

Alternative-guard audit checkpoint: the codegen design explicitly specifies
continuing to the next decision after guard failure. Preserved that contract
rather than changing it to once-per-arm execution. Extended the actual CLI
fixture with overlapping tuple alternatives whose bindings differ, exact guard
effect logs, successful-first stopping, both-failed fallback, skipped-pattern
guards, and a host-timer-suspending guard. The bounded CLI regression passes all
cases, closing the overlapping-guard audit item for these forms. This provides
execution evidence, not a claim of exhaustive pattern correctness.

Nested-pin checkpoint: reproduced side-effecting pins running for unmatched
Option, empty array, and earlier tuple mismatches. All composite pattern tests
were hoisting nested prefixes ahead of their Boolean short-circuit checks.
Introduced shared effect-aware pattern-test composition so prefixes execute only
after preceding tests pass. Kept pure conjunctions and direct Oxc construction.
CLI tests cover Option, array, tuple, ordinary enum and record cases plus an
awaiting pin; the regression fails before and passes after the fix. All 49
codegen tests pass; reviewed both new nested-pin and updated lexical-pin
snapshots. Added codegen/CLI changeset and corrected the documented pin order.

Validation: formatting, strict workspace Clippy, diff checks, and all 12 CLI
tests pass. Full tests pass through the parser/runtime suites and reach solver
integration with the same single tuple-projection failure (293/294 pass).
No pending snapshot files remain. No commit or release-readiness claim yet.

Packaging checkpoint: all 17 publishable workspace crates package and verify
offline from extracted archives in fresh `/tmp/alder-package-hardening.tGBPLq`.
Both synchronization stdlib modules and kernel source/build script are present.
The packaged CLI reruns original generic, mutation, return, module, record,
loop, and Option counterexamples with the intended rejection/success outcomes;
current explicit-async copies pass deep recursion and sibling Promise extern
checks. Restored template whitespace survives an actual formatting rewrite and
execution, followed by an idempotent second format. Original Node timer fairness
probe passes. Packaged hello/pipes and traits/records/explicit_async/pattern
fixtures execute from `/tmp`. See `docs/release-packaging-hardening.md` for exact
commands, probe locations, and limits. This is working-tree evidence, not final
release readiness or a waiver of the tuple failure and remaining decisions.

Package re-export audit checkpoint: extended the dependency CLI tests to cover
renamed facade plus wildcard root publication at runtime. Original extern paths,
shared array identity, exactly-once ordered facade initialization, and the
original missing-wrapper diagnostic all hold. Another path-package test covers
renamed enum/trait/method exports with generic and qualified dispatch into
original instances from two dependency modules, including an Array prerequisite.
Both targeted tests pass; no production compiler change was needed. Recorded
the narrower evidence and remaining re-export audit scope in its design note.

Known-tuple diagnostic checkpoint: replaced the bogus unit-type expectation
for out-of-range tuple reads and writes with a dedicated index/length error.
Reviewed source-aware, colorless snapshots label the index token and explain
zero-based bounds. All 125 driver tests and strict Clippy pass. Full tests still
fail only the existing unresolved tuple inference case (293/294 solver tests).
No choice about inferred tuple arity was made; that user decision remains open.

Index-overflow checkpoint: a parser regression reproduced silent saturation of
`.4294967296` to `.4294967295`. Tuple index scanning now reports overflow instead
of altering the value. The maximum u32 boundary is preserved, and a reviewed
colorless driver diagnostic labels overflow before inference. Updated the grammar
note and scanner contract. This does not address source-sized allocation for
large in-range indices on unknown tuples; sparse inference remains required.

Documentation alignment checkpoint: removed obsolete mutation-permission rules
from the M2/M6 plans and canonicalizer guide, matching ordinary writable lets
and parameters while preserving non-replaceable imported/function declarations.
Corrected the canonical diagnostic guide to require explicit async boundaries,
removed the obsolete parser test inventory entry, and updated the documented
index scanner signature to its checked-overflow result. Rust implementation
examples and historical defect evidence retain their legitimate `mut` mentions.
This documentation-only pass makes no additional implementation-completeness
claim; diff checks pass and the tuple-arity decision remains unanswered.

Generated-artifact cleanup: verified that the untracked hello, explicit_async,
pattern_bindings, records and traits `.alder` directories contain only generated
interfaces/instance indexes and no tracked files. Moved those directories plus
the untracked traits `dist/main.mjs` bundle to recoverable backup directory
`/tmp/alder-generated-backup.Ucg4ov` (named by fixture). Source files and all
compiler work remain in place. No untracked `.alder`/`dist` files remain in Git's
unignored output inventory; diff checks pass. This is not a clean-branch claim:
implementation changes and permanent tests are still uncommitted.

Imported error-group checkpoint: source and serialized-interface regressions
confirm the custom-implementation restriction survives imports and transparent
aliases. Exact-span checks exposed a reporter bug: source implementation
ordinals were incorrectly used as canonical-array indices after imports/value
declarations were removed. Identity-based lookup fixes the label. See
`docs/result-error-kind-audit.md` for reproduction and coverage. All 137 driver
tests pass (one existing ignored doctest); strict workspace Clippy, formatting,
and diff checks pass, with no pending snapshots. The new colorless consumer
snapshot was reviewed. This does not close structural error capabilities,
optional parameters/public Fiber traversals, sparse tuple completion, or the
final full-scope validation/packaging/commit gates.

Structural Result instance checkpoint: reproduced and fixed equivalent named
error rows being treated as disjoint implementation heads, and valid calls
failing dictionary selection against named heads. Coherence expands Result
error slots, including partial constructors; matching compares named row
templates structurally. All 364 solver integration and 137 driver tests pass;
the standalone CLI suite executes local/imported dictionaries on Ok and Err.
Strict workspace Clippy, formatting, and diff checks pass. Details and Sampo
changeset are recorded in `docs/result-error-kind-audit.md` and
`.sampo/changesets/structural-result-instance-heads.md`.

The CLI probe also exposed rejection of a singleton Err constructor assigned
to a named two-tag row. This remains an explicit next investigation; the
single-tag dictionary execution test does not close it. Structural capability
inventory, optional parameters/traversals, tuple completion, and final release
readiness remain outstanding.

Contextual Err checkpoint: isolated the preceding multi-tag construction
failure in a single-module solver test and fixed fresh Err calls to check row
inclusion against the expected Result context. Unknown tags and wrong payloads
still reject. All 368 solver integration tests, 137 driver tests, and the
standalone CLI execution suite pass; the CLI fixture has been restored to its
two-tag groups. Strict workspace Clippy, formatting, and diff checks pass.
Added `.sampo/changesets/contextual-error-construction.md`; reproduction,
invariant, and remaining expression-form audit are in the Result error-kind
audit. No broader completion or release-readiness claim is made.

Contextual block checkpoint: reproduced loss of expected types through a block
tail and fixed it with shared block inference accepting an optional expected
tail type. Existing branch behavior passed the adversarial probe unchanged.
All 372 solver integration and 137 driver tests pass, as does standalone CLI
execution including a side-effecting contextual block. Early-return and unit
fallthrough regressions preserve control-flow boundaries. Formatting and diff
checks pass; details are in the Result error-kind audit and the contextual error
construction changeset. This does not close the larger hardening audit.

Result identity checkpoint: a user `err` function in `Result.ald` incorrectly
received built-in bare-tag permission because the solver checked only the last
module segment. The reproduction compiled before requiring the Builtin package
and exact module path; it now rejects with the existing source-aware tag
diagnostic. Actual result.err remains covered by the passing contextual solver
tests. All 372 solver integration and 138 driver tests passed, strict workspace
Clippy passed, and formatting/diff checks pass. An attempted uppercase import
probe was invalid syntax and removed, as documented in the Result audit. Added
the result-intrinsic-identity changeset. Remaining core hardening requirements
and final acceptance gates are unchanged.

Sparse tuple design checkpoint: traced the unresolved dense allocation through
solver types and interface emission. `plans/tuple-projection-hardening.md` now
specifies an exact-arity sparse scheme-constraint implementation route, including
unobserved-slot independence, whole-tuple identity, consumer diagnostics,
generic checking, serialization, and 32-bit length arithmetic. This is a design
checkpoint, not an implemented allocation fix. Full workspace regular tests
passed at this state (including 372 solver, 138 driver, 12 CLI, 50 kernel, and
1306 parser tests); the full command's doctest phase was still running at this
checkpoint and must be polled to terminal before recording full success.

That full workspace command subsequently completed successfully, including all
doctests (three existing ignored doctests). This is evidence for the state before
the following tuple schema plumbing, not a final full-scope acceptance claim.

Sparse tuple storage implementation has begun: Annotation/owned Scheme now
carry exact-length sparse shape metadata; arena copies and owned serialization
preserve it; interface format is 5. Declaration builders and solver output still
emit empty metadata. Nonempty solver production/enforcement remains required
before this intermediate stage is mergeable. The tuple plan records this
boundary explicitly and the sparse-tuple-constraints changeset tracks affected
crates. No maximal-index source program has been compiled.

Imported sparse tuple inference checkpoint: constraints now survive scheme
selection, free-variable tracking, instantiation and re-emission. A stored
consumer probe reproduced wrong-element acceptance before concrete shape
checking was added. Its positive/negative cases now pass, including independent
calls, a generic inferred relay, and rejection of a rigidly annotated relay.
All 372 solver integration and 140 driver tests pass, as does strict workspace
Clippy. Source projection finalization remains dense; source production,
transitive cycles and the other tuple acceptance requirements remain open.

Sparse source tuple checkpoint: removed source-index-sized tuple finalization.
Observed projections now produce exact-length sparse constraints, and later
projections respect the fixed length. A maximum-u32-index source program and
its stored-interface round trip use compact metadata. Added iterative cycle
checking after a new transitive tuple-equation regression exposed acceptance.
Five reviewed snapshots now display constraints instead of silently omitting
them. All 374 solver integration and 140 driver tests, standalone CLI execution,
strict workspace Clippy, formatting and diff checks pass; no pending snapshots
remain. The detailed tuple plan lists outstanding adversarial acceptance checks.
Optional parameters/public Fiber traversals, structural error capabilities,
final packaging, coherent commits and clean-branch readiness remain unfinished.

Sparse tuple boundary validation: source-produced stored forwarders preserve
whole-tuple relationships and independent calls while rejecting wrong lengths
and returned slot types. A captured mutable-array regression rejects mixed
payload instantiations. The reviewed maximum-index consumer snapshot renders
the expected length compactly and labels the consumer reference. All 375 solver
integration and 142 driver tests pass, as do strict workspace Clippy, formatting
and diff checks; no pending snapshots remain. These cases needed no additional
solver changes and do not close the remaining whole-goal acceptance audit.

JSON emission-size checkpoint: a new regression measured Array codec output at
1,650 / 6,770 / 133,362 bytes for depths 2 / 4 / 8 before the fix. The emitter
duplicated recursively generated child evidence into both methods. Container
and structural error Json codecs now share a lazy descriptor closure built as
direct Oxc AST, emitting each payload once without eagerly evaluating recursive
dictionary references. Exact kernel-call-count tests cover both nesting forms;
the reviewed structural snapshot shrank from 123 emitted lines to 57. All 58
codegen tests and standalone CLI fixtures pass, including new recursive generic
JSON round trips with nested options and custom payload codecs. Detailed
evidence and limitations are in docs/json-hardening.md. Other multi-method
dictionary emitters (notably Hash plus its equality superclass) still need a
corresponding duplication audit; this checkpoint does not close that work or
the whole-goal validation/packaging/commit requirements.

Descriptor-sharing validation completed: full workspace `cargo test -- --quiet`
passes, including doctests (three existing ignored doctests). Strict workspace
all-target/all-feature Clippy, formatting, and diff checks pass; the intentional
snapshot was reviewed and no pending snapshots remain. Packaging is still a
separate outstanding gate.

Hash descriptor follow-up: the new depth regression failed before the fix:
two Array layers emitted three `$hashContainer` calls (1,037 bytes). Hash and
its Eq superclass recursively duplicated the same child Hash evidence. Both
now close over a shared lazy descriptor; Eq maps each child to `$super0` at
invocation, preserving dispatch and recursive initialization. Depths 2/4/8 now
emit exactly one hashing/equality kernel call per level. A reviewed source-aware
snapshot covers mixed Array/Option evidence. CLI tests exercise a generic
Hash-bound function in another module, colliding custom hashes with distinct
enum values, array mutation, nested Some(None), and recursive generic trees.
The standalone CLI fixtures pass. Broader validation is recorded below when
finished; this does not settle structural error Hash inventory or Ord policy.

Diagnostics follow-up discovered while constructing that fixture: adding an
explicit Eq implementation to `enum Key { Value(Number) }` correctly conflicts
with its generated Eq implementation, but compiling the multi-module traits
fixture repeated `overlapping_impl` in unrelated modules at line 1 and emitted
secondary unknown-name errors from their missing interfaces. The invalid custom
Eq fixture was removed (the final regression uses the enum's supported Eq plus
a custom colliding Hash). Preserve the valid overlap rejection, but reproduce
and fix foreign-span attribution and failure cascades during the diagnostics
audit; the current passing positive fixture does not validate those errors.

The initial source inspection locates a likely inconsistency: the driver's
header coherence pass filters by `coherence_belongs_to(error, home)`, whereas
errors returned by ordinary solving are rendered against the current module
without that ownership filter. Reproduce this with a small driver regression
before changing validation ownership; unrelated-module success must not hide
the original invalid implementation or weaken package-wide coherence.

Hash-sharing validation: all workspace unit/integration tests pass via
`cargo test --lib --tests -- --quiet` (60 codegen tests, all 12 CLI tests, and
the remaining suites). Strict workspace all-target/all-feature Clippy,
formatting and diff checks pass; no pending snapshots remain. The prior full
doctest run predates this Hash change; final doctests/packaging and remaining
hardening acceptance requirements are still outstanding.

Source coherence diagnostic checkpoint: the new three-module regression first
reproduced unrelated-source overlap errors with zero-length labels. A frozen
registry check now stops the final body pass when source-owned coherence fails;
owners retain diagnostics and other modules are explicitly Blocked, with no
published artifacts/interfaces/indexes. Both input orders pass. A second test
reproduced and fixed phantom labels for cross-module overlaps; its two rendered
snapshots were reviewed, and the broken-body package-overlap regression remains
green. All workspace unit/integration tests pass (152 driver, 60 codegen, 12 CLI,
and the other suites). Dependency-only invalid indexes and mixed external
diagnostic cases remain open in docs/coherence-diagnostics-hardening.md; no
coherence validation was removed from the solver.

Dependency coherence ownership checkpoint: combining independently valid stored
dependency indexes reproduced an overlap diagnostic falsely attached to the
application. The frozen registry now stops on all coherence failures, recording
dependency-only errors in BuildResult.diagnostics and source-owned errors on
their defining modules. Shared CoherenceError module ownership retains canonical
identity and deterministic ordering. CLI consumers display build-level errors;
even zero-source builds fail when their dependency registry is invalid. The
regression covers zero/one/two unrelated modules and a simultaneous independent
local conflict, with no published artifacts/interfaces and one dependency error.
Its colorless snapshot was reviewed. Mixed source/dependency overlaps, external
cycles, and broader trust-boundary testing remain open in the diagnostic audit.

Stored-dependency checkpoint validation passes: all workspace unit/integration
tests (153 driver tests and all CLI fixtures among them), strict workspace
all-target/all-feature Clippy, formatting, and diff checks. No pending snapshots
remain. These checks do not replace final doctest and release-package gates.

Mixed coherence boundary follow-up: source and stored implementations of the
same dependency-package subject retain one real source-owned overlap diagnostic
and the package-qualified stored module hint, for both canonical implementation
orderings. Removing the stored conflict compiles. Invalid builds publish no
artifacts/interfaces/indexes. Fingerprint-valid serialized superclass and
associated-type cycles are also rejected without application sources, whereas
their valid controls pass; diagnostics retain dependency identity without
phantom spans. All 156 driver unit tests pass. This checkpoint required only
regression tests and a reviewed snapshot, not further production changes, and
does not claim the entire stored-interface trust audit is complete.

Mixed-coherence verification finishes with strict workspace Clippy and driver
doctests passing (one existing ignored doctest), plus formatting/diff checks and
no pending snapshots. The nominal error-group derive migration, remaining row
capability work, and final whole-goal release gates remain separate unfinished
work; the diagnostic regression slice is not a completion claim for them.

Direct error-group normalization is implemented: a new relay between equivalent
named groups failed as First-versus-Second before routing direct references
through the existing guarded row expansion. The CLI fixture now round-trips
direct error values using custom-payload Json and Show dictionaries; a stored
producer/consumer test preserves equivalent group contracts and rejects changed
payload types with a reviewed diagnostic. Strict workspace Clippy passes.

The broader test run is intentionally not green yet: the codegen
error_group_ord_preserves_declaration_order snapshot changes its superclass to
structural Eq. Its pending snapshot was reviewed but not accepted because the
underlying nominal ordering contract conflicts with approved structural group
identity. Next migrate per-group derive synthesis and its repository consumers,
including the selected Hash behavior and removal of nominal fallback, rather
than enshrining the obsolete ordering behavior. The detailed error-kind audit
records this incomplete boundary; no final release gate has been satisfied.

Structural error-group migration checkpoint: removed per-group derive synthesis
and obsolete declaration-order Ord expectations. Closed rows now resolve Hash
from payload Hash dictionaries, sharing those dictionaries with Eq superclass
evidence. The kernel hashes active tags and payloads independently of nominal
identity, declaration order, and row width. Codegen regressions cover linear
nested evidence growth; kernel and CLI checks cover equal hashes under widening,
custom payload dispatch, recursive containers, and reordered groups. All 62
codegen tests and the standalone CLI fixtures pass; reviewed snapshot duplicates
were removed after acceptance. Docs/spec and a Sampo changeset describe the
selected structural capability inventory.

Full solver validation is not green: 398/399 pass. The existing
`nominal_wrappers_of_error_groups_can_have_custom_implementations` regression
fails deriving Eq for `Wrapper(Result[Number, Failure])`. The separate
`resolve_derived_variant_fields` path uses `ty_from_ast`, which leaves named
error groups nominal, unlike ordinary annotation conversion. Fix that evidence
normalization boundary without restoring nominal group dictionaries or rejecting
valid nominal wrappers. Then rerun broader suites and final release gates.

Derived-field follow-up: replaced the separate `ty_from_ast` converter with the
ordinary inference annotation converter, shared across each synthetic instance's
parameters, givens, and payload fields. This removes the duplicate incomplete
conversion rather than adding a nominal fallback. The existing nominal-wrapper
regression and all 399 solver integration tests pass. Added a CLI generic wrapper
case containing both a direct named group and Result's named error argument,
checking custom-payload Show, Hash/Eq, nested Option, and JSON round trips.
Broad workspace lint/test validation is running; release gates remain open.

This follow-up's workspace unit/integration suite now passes, including 399
solver integration tests, 157 driver tests, 62 codegen tests, 53 kernel tests,
and all 12 CLI tests. Strict all-target/all-feature Clippy passes. The only
snapshot adjustment found by the broader run was the expected rendered derive
diagnostic changing its supported target from enums/error groups to enums;
reviewed and updated without changing the source label. No pending snapshots
remain. Workspace doctests are running separately; packaging and whole-goal
completion verification are not covered by these results.

Stored structural Hash evidence now has a serialization/drop/reload regression:
generic fingerprint and Eq-superclass contracts accept Number/String payloads
and reordered named groups, while a function payload is rejected at the imported
call. The new source-aware colorless diagnostic snapshot was reviewed; all 158
driver tests pass. No extra production change was needed for this check.

The next optional-argument acceptance gap is reproduced as a permanent test:
`pipe_inputs_preserve_context_for_fresh_record_payloads` currently fails for a
fresh record piped into an Option-record parameter, despite the equivalent
direct call passing. Pipe inference discards the source initializer before its
parameter context is available. The detailed optional-arguments plan records
the required checked-call/refined reachability work. This newly added regression
means the current solver suite is not green; earlier passing counts do not
include it. Strict workspace Clippy completed successfully before this test.

The outstanding workspace doctest process completed successfully, with the
three existing ignored doctests in driver/parser/runtime. This validates the
preceding production checkpoint, not the newly reproduced pipe-input bug.

Pipe-input context is now implemented: `CallInput.leading` retains the source
expression and runs it once through ordinary contextual argument checking.
Destination inference respects input fallthrough; input inference retains its
original reachability. Existing mutable record aliases remain invariant. All
403 solver tests present at the initial follow-up pass, as do CLI tests for
bare/called destinations, arrays of fresh records, Option field wrapping, and
exactly-once left-before-callee side effects. Strict workspace Clippy passes.
An additional Await/Try context regression and the broad workspace suite are
running. No codegen/runtime ABI change was required; existing pipe emission
already preserves runtime evaluation order. A Sampo changeset records the fix.

Pipe checkpoint validation completed: all workspace unit/integration suites
pass, including 404 solver integration tests and the awaited-pipe regression.
No snapshot changes were needed. Formatting/diff checks pass. Doctests passed
at the preceding structural-error checkpoint, not rerun after this pipe change;
the final full-goal packaging, documentation, and commit gates remain open.

Next contextual audit reproduced failures through explicit Some and through
if-expression arguments. Some construction now receives its expected Option
result before argument inference; the checked argument path then contextualizes
fresh record fields. Canonical Some and option.some pass actual CLI execution.
The known branch-context failure remains a permanent failing regression; 405
solver tests passed with only that named test excluded. No full-green claim is
made while it remains. See optional-arguments-hardening.md for the boundary and
remaining branch/block/match/spread work. Added a constructor-context changeset.

Branch/block/match initializer context is implemented. Exact expectations and
pending lift-input expectations share the existing branch checking and block
reachability logic, with an exact aggregate-result check preserving rejection
of reachable unit fallthrough. All 410 solver tests pass after correcting that
refactor regression and a match-record fixture's required parentheses. Tests
retain alias invariance and reject implicit function-return wrapping. CLI
coverage for branch selection, Option depth, and effects plus strict workspace
lint/unit/integration checks are running. Added a branch-context changeset;
the broader optional-argument and whole-goal audits remain incomplete.

Branch checkpoint validation completed: strict workspace Clippy and all
workspace unit/integration tests pass (410 solver integration, 158 driver, 62
codegen, 53 kernel, and all 12 CLI tests). CLI assertions verify both selected
branches, match results, explicit Some depth, field contexts, and one effect
per selected branch. Formatting/diff checks pass; no pending or changed
snapshots. Doctests and packaging were not rerun for this checkpoint.

The next record-context gap is reproduced: a spread disables contextual typing
for all explicit fields, so `{ ..base, value: 42 }` cannot initialize an Option
field even when base only contributes an unrelated label. Added the permanent
positive and an inherited-nested-alias negative (the negative passes). A separate
overwrite control prevents a fix from constraining discarded intermediate
fields to the final type. The positive regression is currently failing; the
prior green workspace result predates it. The optional-arguments plan records
the required interaction with ordered/optional/unresolved overlay operands.

Contextual spread fields now retain independent deferred lift targets until the
ordered merge relates surviving payloads to the final annotation. This fixes
both field orders without forcing an overwritten Bool to satisfy Option[Number].
Inherited nested aliases are not converted. All 413 solver tests at the first
post-fix checkpoint pass; additional targeted tests cover nested records, Some,
arrays, and optional fallbacks. CLI effects/value assertions and broad workspace
lint/tests are running. Added a spread-context changeset; unresolved/generic
overlay interactions and final acceptance remain under audit.

Spread checkpoint validation completed: strict workspace Clippy and all
workspace unit/integration tests pass, including 414 solver integration tests.
The CLI confirms nested wrapping, field/source order, overwritten initializer
effects, and absent-versus-present-None fallback behavior. No snapshot updates
were needed. Language documentation now describes the implemented optional
parameter and contextual field rules. Doctests/packaging and the remaining
whole-goal acceptance work are not covered by this run.

Example/package readiness pass: the rebuilt CLI runs hello, pipes, traits, and
async with the expected success messages. Fresh `cargo package --workspace
--exclude stub --allow-dirty` verification is running in
`/tmp/alder-package-current.6yJzBM`; all 17 archives were created and their stdlib
and kernel include paths inspected. No packaging success claim until compilation
finishes. Commit `23dcc41` removes two generated example caches from Git tracking
and ignores `.alder/`; both cache files remain locally available. The broader
dirty hardening worktree is preserved, not committed by that hygiene checkpoint.

Package verification completed successfully for all 17 publishable crates.
The extracted-package CLI then ran all four examples plus traits and
explicit_async fixtures from `/tmp`, all exit zero. Workspace Cargo.lock is
unchanged. This verifies current local packaging and execution, not remote
platform builds or the remaining whole-goal acceptance requirements.

Workspace package identity audit: two distinct roots declaring vendor/widgets,
with nonoverlapping one.ald/two.ald sources, were accepted by Project::load and
the actual CLI compiled both into the same package. Loading now rejects duplicate
named package roots after canonical path deduplication and sorting, before source
discovery. The regression checks reversed member order yields the same diagnostic
with both paths and confirms repeated paths to one named package remain valid.
All four workspace regressions pass, as do formatting, strict workspace Clippy,
and full tests including doctests (166 driver tests; two pre-existing ignored
parser/runtime doctests). The rebuilt CLI rejects the original reproduction
from `/tmp` before module discovery. Committed the isolated fix, regression,
module documentation, and changeset as `cb89192`. The previous
`/tmp/alder-package-identities.UpMP5z` verification predates this fix.
