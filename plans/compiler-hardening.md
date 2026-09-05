# Compiler hardening

Status: active on `compiler-hardening`, based on `21994e0`.

The eleven defects from the September 4 review are minimum acceptance scope.
The M2–M4 milestone checkmarks describe the feature work that landed, not proof
that the contracts below are sound. Further milestones remain out of scope.

## Working rules

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
- [ ] Distinguish rigid contract variables from flexible inference variables;
  prevent specialization, identification of independent universals, and escape.
- [ ] Audit explicit signatures, partial annotations, defaults, impls, HKT,
  associated equalities, bounds, recursive groups, and cross-module interfaces.
- [ ] Source-aware diagnostics and positive/negative execution tests.

Root cause: implementation checking instantiates its expected signature with
flexible variables and unifies away the method's universal contract.

### 2. Mutation and polymorphism

- [x] Regressions for a shared top-level empty Array used at incompatible types.
- [ ] Sound generalization restriction accounting for reachable mutable state,
  while preserving safe function polymorphism and existing aliasing semantics.
- [ ] Arrays, maps, sets, nested records, aliases, captured state, reusable tasks,
  SCCs, and cross-module escape tests; document the selected restriction.

Root cause: `let mut` is used as the generalization criterion, although ordinary
bindings can contain shared mutable objects.

### 3. Formatter semantics

- [x] Regressions for whitespace-only/trailing-space template payloads.
- [ ] Syntax-aware preservation of templates, interpolation, escapes, raw macro
  bodies, markup text, comments, and supported line-ending semantics.
- [ ] Compare meaning/literal payloads as well as reparsing and idempotence;
  CLI failure cannot overwrite input with an unsafe output.

Root cause: line trimming occurs before literal context is considered; the
current reparse/comment comparison cannot detect changed literal values.

### 4. Deterministic modules

- [x] Reject `util.ald` alongside `util/mod.ald`, labeling both sources.
- [ ] Canonical identities and import lookup use package/source-root context.
- [ ] Audit root modules, workspaces, same-path dependencies, repeated `src`
  directories, interface/cache identities, and initialization/build ordering.
- [ ] Repeated and shuffled-discovery builds produce equivalent output/errors.

Root cause: suffix-based graph resolution chooses one candidate; later maps
collapse duplicate canonical identities in nondeterministic traversal order.

Graph ordering checkpoint: permanent tests reproduced nondeterministic build
order and cycle selection. The ready queue now chooses the smallest source URI,
depth groups are sorted, and cycle DFS visits sorted roots/imports. Tests shuffle
discovery and import order across 32 independently allocated graphs. This only
fixes graph ordering: duplicate rejection and package/source-root-aware identity
remain open. In particular, `module_id_from_uri` splits on the first `/src/`,
while `resolve_source_import` ignores both its current module and package context;
both must be replaced by the same explicit identity mapping used by compilation.
Checkpoint validation: both new regressions failed before the fix and pass
after it; all 81 driver tests, full workspace tests (including CLI execution),
strict all-target/all-feature Clippy, and formatting checks pass. No snapshots
changed or remain pending. Final release packaging remains an open gate.

### 5 and 11. Control flow and loop results

- [x] Regressions for Number-returning zero-iteration loops and valued `break`.
- [x] Explicit fallthrough/divergence model, distinct from contains-return.
- [ ] Each loop owns a result variable and the correct break/continue target.
- [ ] Check blocks, branches, matches, early exits, `?`, lambdas, functions,
  methods, nested loops, while/for, async bodies, and unreachable paths.
- [ ] Inference, diagnostics, and executed lowering agree.

Root causes: any return in a loop bypasses fallthrough checks; loop expressions
always infer unit and break payloads do not constrain a target result.

### 6 and 7. Scheduler fairness and task frames

- [x] Regressions for immediately fulfilled Promise starvation and deep awaits.
- [x] Stack-safe task frames/trampoline with scheduler-visible composition;
  sequential await remains in the current fiber.
- [ ] Bounded host yielding across resumptions, Promise/Join/All/Race/Fork,
  completion, and cleanup; avoid recursive scheduler execution/reentrancy.
- [ ] Deep execution before/after suspension, cancellation/unwinding, reusable
  lazy tasks, error rows, scopes/finalizers, and provider inheritance.
- [ ] Revisit pinned Effect reference and document ABI/semantic divergences.

Root causes: budget resets on every resumed run; JS `yield*` delegation nests
native generator calls outside the scheduler's visibility.

### 8. Record rows

- [ ] Regressions for inferred `record.x + record.y` and correlated row tails.
- [ ] Real record-row variables, symmetric unification, occurs/kind checks,
  instantiation/generalization, and stable interface round trips.
- [ ] Access-order independence, spreads/extensions, optional fields, aliases,
  patterns, assignment, cross-module use, and incompatible constraints.

Root cause: record extensions are reduced to an openness boolean, losing row
identity and making the first field access freeze the inferred field set.

### 9. Option representation

- [x] Regression for equality of separately constructed nested Some(None).
- [ ] Audit equality, mapping, patterns, derives, ordering/hash/show/JSON,
  unit payloads, nested containers, and higher-kinded operations.
- [ ] Relevant equality/hash laws and round trips; centralize payload handling.

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
- [ ] Audit stdlib declarations against their runtime implementations.
  The unchecked `Json.decode` bypass is fixed with bounded dictionary dispatch;
  the wider codec and stdlib audit remains open.
- [x] `Map.get` wraps present values so a present `None` differs from absence.
- [x] Nested lambda annotations share same-named enclosing type variables as
  promised in `docs/language.md`, including nested lambda scopes and HKT.
- [ ] Generic evidence and interface contract fidelity.
- [ ] Evaluation order/exactly-once codegen and control-flow boundaries.
- [ ] Async inference versus runtime representation.
- [ ] Promise throws/mappers/malformed returns/reentrant cancellation/late exits.
- [ ] Exactly-once completion/observers, child ownership, all/race cleanup,
  finalizer registration while/after closing, and masked interruption.
- [ ] Deterministic cache identities, source fidelity, and deferred constructs.
- [ ] Public wildcard/name re-exports: `pub import ~/src/util.*` is accepted
  but its values are absent from the publishing module's interface. Reproduced
  through a package-root consumer calling the re-exported `answer`; investigate
  named re-exports, origin identity, codegen bindings, and cyclic re-export cases.
- [ ] Identify inactive Elm-era Rust modules and correct obsolete claims.

## Evidence log

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
  a permanent canonicalization test and fix clause handling next; the current
  extern fixture uses the equivalent `where a: Show + Eq` to isolate the ABI.
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
  unbounded `std/Json.ald` decode extern but also `Intrinsic::JsonKernel` primitive
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
  of weakening mutable-alias checks. Added `tests/e2e/records` and registered
  it for execution to cover renamed row tails and optional missing/present reads.
  Running that new fixture directly compiles but exits with an assertion
  failure. Code inspection shows Access still emits raw member reads while
  Option.none is null; investigate omitted-field undefined versus Option
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
- Direct kernel probes additionally confirmed `Json.decode("42")` returns
  `Ok(42)` without target-type validation and `Map.get` returns identical null
  values for a present None and an absent key. These runtime-contract defects
  remain required follow-up work, not covered by signature loading alone.
- Stdlib validation: formatting, strict Clippy, full workspace tests (120 solver
  integration tests, 77 driver tests), and CLI execution fixtures pass. The
  package tarball verifies with the pending local dependency set. No pending
  snapshot files remain.
- Option audit: reproduced nested equality and map-result collapse in permanent
  kernel tests. All payload consumers now unwrap once; map/apply/traverse and
  Map.get rewrap successful payloads. Some of an existing box adds a layer;
  private box identity avoids collisions with user enums named Some.
- Four kernel regressions cover equality/symmetry/hash consistency, show,
  layers, unit, mapping/applicative/monadic/traversal behavior, map presence,
  and nested JSON round trips. The nullable JSON codec now uses an escaped
  singleton `$alderSome` envelope, with collision escaping; simple non-null
  encodings are unchanged. Actual CLI trait fixtures verify nested options,
  derives, hashing, showing, JSON, map lookup, and a user Some enum payload.
- Option pattern/optional-field and ordering audits remain open; current solver
  intrinsic selection has no builtin Ord[Option]. The unrelated unchecked
  Json.decode entry point also remains open.
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
  `record.value = Option.some(42)` was accepted. `place_type` reused read typing
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

- [ ] Re-run every original reproduction against the final compiler.
- [ ] Adversarial review of alternate forms and cross-feature interactions.
- [ ] `cargo fmt --all` and strict all-target/all-feature Clippy.
- [ ] Full `cargo test`, snapshots reviewed, no pending/stale artifacts.
- [ ] Actual CLI build/run/test, affected examples, runtime bounded regressions.
- [ ] Release package verification for affected crates.
- [ ] SPEC/design docs reflect behavior; per-crate Sampo changesets.
- [ ] Clean committed branch; final requirement-by-requirement evidence audit.

Finite regression coverage is evidence for these contracts, not a claim of
exhaustive compiler correctness. Do not mark unresolved items complete.
