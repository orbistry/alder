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

- [ ] Regressions for a shared top-level empty Array used at incompatible types.
- [ ] Sound generalization restriction accounting for reachable mutable state,
  while preserving safe function polymorphism and existing aliasing semantics.
- [ ] Arrays, maps, sets, nested records, aliases, captured state, reusable tasks,
  SCCs, and cross-module escape tests; document the selected restriction.

Root cause: `let mut` is used as the generalization criterion, although ordinary
bindings can contain shared mutable objects.

### 3. Formatter semantics

- [ ] Regressions for whitespace-only/trailing-space template payloads.
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

- [ ] Regression for equality of separately constructed nested Some(None).
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

- [ ] **Confirmed additional defect:** prelude module members are canonicalized
  as `ValueRef::Builtin`, which inference resolves to `Ty::Any`. Load and check
  the actual stdlib signatures; reject unknown members and invalid calls. This
  currently bypasses contracts for Array/String/Fiber and the other modules.
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
