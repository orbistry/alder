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

- [ ] Reject `util.ald` alongside `util/mod.ald`, labeling both sources.
- [ ] Canonical identities and import lookup use package/source-root context.
- [ ] Audit root modules, workspaces, same-path dependencies, repeated `src`
  directories, interface/cache identities, and initialization/build ordering.
- [ ] Repeated and shuffled-discovery builds produce equivalent output/errors.

Root cause: suffix-based graph resolution chooses one candidate; later maps
collapse duplicate canonical identities in nondeterministic traversal order.

### 5 and 11. Control flow and loop results

- [ ] Regressions for Number-returning zero-iteration loops and valued `break`.
- [ ] Explicit fallthrough/divergence model, distinct from contains-return.
- [ ] Each loop owns a result variable and the correct break/continue target.
- [ ] Check blocks, branches, matches, early exits, `?`, lambdas, functions,
  methods, nested loops, while/for, async bodies, and unreachable paths.
- [ ] Inference, diagnostics, and executed lowering agree.

Root causes: any return in a loop bypasses fallthrough checks; loop expressions
always infer unit and break payloads do not constrain a target result.

### 6 and 7. Scheduler fairness and task frames

- [ ] Regressions for immediately fulfilled Promise starvation and deep awaits.
- [ ] Stack-safe task frames/trampoline with scheduler-visible composition;
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

- [ ] Regression for `./client.js` beside an Alder source module.
- [ ] Carry physical origins through virtual modules; resolve relative externs
  consistently regardless of shell cwd and across package boundaries.
- [ ] CLI build/run with local Promise wrapper, typed Result fulfillment,
  throw/rejection, AbortSignal cancellation, and contextual resolution errors.

Root cause: virtual module imports lack a physical importer location.

## Related audit

- [x] Prelude module members carry actual stdlib signatures, rejecting unknown
  members and invalid calls. Removed the untyped `ValueRef::Builtin` path.
- [ ] Audit stdlib declarations against their runtime implementations:
  `Json.decode` currently promises arbitrary `a` but only runs `JSON.parse`.
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
- [ ] Identify inactive Elm-era Rust modules and correct obsolete claims.

## Evidence log

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
