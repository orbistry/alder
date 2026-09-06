# Deferred services/layers dependency injection

Status: architectural direction agreed; implementation deferred at the user's
request. Do not start this plan automatically. Macros/comptime are also deferred
and are not a prerequisite for compiler-understood DI annotations.

Design authority: `docs/dependency-injection.md`. This plan replaces the older
M4 proposal to make body-level `use` and nested `provide` the primary public DI
interface. Existing context runtime behavior remains intact until migration.

## Agreed direction

- Effect-style service identities/contracts and dependency-aware provider factories.
- Function requirements above declarations: `#[using(db: Database, ...)]`.
- Nominal service identity, not local binding-name matching.
- Declarative composition roots with generated wiring and test overrides.
- Construction dependencies captured by implementations, not leaked to consumers.
- Compile-time availability checks across functions, callbacks, tasks, and modules.
- Owned resource acquisition/cleanup; proposed application/request lifetimes.
- No reflection, registration scanning, arbitrary container access, or mandatory
  nested `provide` blocks around application code.

## Before implementation

- [ ] Specify service declarations, service values, provider signatures, and root
  composition syntax; sketches in the design document are not grammar.
- [ ] Specify requirement representation in callable/task types and owned
  interfaces, including higher-order calls, recursive groups, trait methods,
  and explicit forwarding contracts.
- [ ] Specify lexical capture and lifetime checking for returned closures/tasks,
  scoped values, detached work, and foreign boundaries. Do not claim escape
  safety from dependency graph validation alone.
- [ ] Specify managed resource construction, typed initialization errors,
  cancellation, partial-startup rollback, and finalizer failure semantics.
- [ ] Specify request/job scope creation, externally supplied request inputs,
  instance sharing, graph composition, and explicit override rules.
- [ ] Define migration of existing `use`/`provide` syntax and fiber-local context.
  Keep the existing error-row, Task, and scheduler contracts separate.

## Implementation sequence when resumed

1. Land source/canonical/owned contracts and parser snapshots for settled syntax.
2. Type-check service values and explicit function requirements; preserve contracts
   through function values and interfaces without whole-program graph analysis.
3. Validate static composition graphs, provider outputs, ambiguity, and cycles;
   implement a plain-value application-root vertical slice with generated wiring.
4. Add asynchronous/fallible providers, deterministic acquisition and rollback,
   finalizers, and cancellation-safe teardown using existing runtime primitives.
5. Add scoped roots and enforce their agreed escape/lifetime rules before making
   request-scope safety a public guarantee.
6. Add composition reuse and test overrides; migrate existing context usage and
   publish documentation/examples only for accepted behavior.

## Acceptance

- [ ] Positive and negative source-aware tests for all static checks in the design.
- [ ] Cross-module and serialized-interface requirements; callbacks, aliases,
  recursion, async capture, and indirect calls cannot erase requirements.
- [ ] Actual CLI execution checks construction order, sharing, lexical capture,
  fresh test roots, overrides, and absence of context leakage.
- [ ] Runtime tests cover failed/cancelled startup, exactly-once cleanup,
  dependency-ordered teardown, scoped work, and escaped-resource rejection.
- [ ] CLI/editor diagnostics identify requirement and registration sites.
- [ ] Review grammar, language/runtime docs, SPEC, and changesets; pass formatting,
  strict Clippy, full tests, and relevant package verification.

No implementation checkboxes are completed by recording this design.
