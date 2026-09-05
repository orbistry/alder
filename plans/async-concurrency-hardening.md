# Async and concurrency decisions

Status: approved direction recorded September 5, 2026; implementation pending.
This extends compiler hardening, not the later web/data/provider milestones.
Existing inferred-async behavior is not the final contract below.

## Explicit lazy async bodies

- Introduce both `async fn` and `async { ... }` using the same keyword.
- Calling an async function creates a lazy, reusable task. Constructing an async
  block likewise creates a task; neither starts the body.
- An async function's return annotation describes its completed value. `async`
  adds exactly one outer `Task`: `async fn load() Result[Data, e]` has callable
  result `Task[Result[Data, e]]`.
- No automatic flattening. An async function annotated `Task[Number]` produces
  `Task[Task[Number]]`; await the inner task explicitly to return its value.
- Plain functions execute immediately and may return existing task values.
  A plain function annotated `Task[a]` is not thereby an async body.
- `.await` requires an enclosing async body. Do not infer execution mode from
  the presence or absence of awaits. Async bodies remain lazy when they contain
  no awaits, including after refactoring.
- `return` and `?` inside an async block complete that task, not its enclosing
  function. `break`/`continue` cannot escape across the async boundary.
- Named methods use the same explicit async distinction as named functions.
- Do not introduce separate async-lambda syntax. Use an ordinary lambda whose
  body returns an async block: `url -> async { fetch(url).await? }`.
  If annotated, the lambda's return type is its actual `Task[...]` type, not
  the completed-value annotation used by `async fn`. The async block owns its
  awaits, returns, and error propagation; lambda invocation creates the task
  without executing the block. Add parser/type/codegen and CLI coverage for
  these lambdas, including use with bounded Fiber traversal.
- Keep main's declared execution mode explicit. The standalone launcher handles
  a synchronous main or executes its returned task in the root fiber; root
  fiber execution does not implicitly authorize awaits in plain functions.

## JavaScript-style capture and mutation

- Async blocks capture lexical bindings like ordinary JavaScript closures, not
  value snapshots. Rebinding before execution is observed by the task.
- Arrays and records retain shared-reference behavior. Repeated executions may
  observe different shared state; reusability does not promise purity.
- Preserve argument evaluation at call time for async function calls and body
  execution at task execution time. Do not defer evaluation of call arguments.
- This combines with the approved removal of `mut`: retain type-safe assignment
  and alias-aware generalization, not mutation permissions or borrowing.
- Cover scope lifetime, shadowing, loop captures, escaping/reused tasks, and
  reassignment of captured polymorphic bindings in tests.

## Explicit synchronization primitives

Introduce a small library foundation informed by the pinned Effect v4 source:

- `Ref`: shared cells with atomic synchronous state transformations.
- `SynchronizedRef`: serialized state transformations that may suspend.
- A cancellation-safe semaphore for bounded access to shared resources.

Atomic here concerns cooperating fibers in Alder's runtime, not cross-worker
shared-memory atomics. Lexical capture does not provide synchronization.
Protection applies only to operations using the primitive; an escaped mutable
payload is not magically protected from direct writes through other aliases.
Specify exact signatures and failure behavior before implementation. Test
lost-update prevention, interruption of waiters, permit release/finalization,
and failure during a suspended state transition. Do not port Effect wholesale.

## Bounded Fiber traversal

Provide these distinct operations (schematic types, not final declarations):

| Operation | Callback result | Traversal result |
| --- | --- | --- |
| `Fiber.map` | `Task[b]` | `Task[Array[b]]` |
| `Fiber.forEach` | `Task[()]` | `Task[()]` |
| `Fiber.tryMap` | `Task[Result[b, e]]` | `Task[Result[Array[b], e]]` |
| `Fiber.tryForEach` | `Task[Result[(), e]]` | `Task[Result[(), e]]` |

- Support a concurrency setting, e.g. `{ concurrency: 8 }`. Default to
  sequential execution; unbounded concurrency must be explicit. Specify the
  configuration type and validation of invalid bounds before implementation.
- Bound active item operations and invoke each callback only when a slot opens.
  Do not create one waiting fiber per input and call that bounded scheduling.
- Construct traversal lazily; preserve input order in collected results,
  independently of completion order. Specify mutable-input iteration behavior
  rather than silently relying on incidental JavaScript iteration details.
- `forEach` does not collect an array of units and does not silently discard
  arbitrary result types. No boolean `discard` changing the return type.
- `map` treats each callback result as an ordinary value. If `b` is a Result,
  collect every success/error without cancelling siblings for an `Err`.
- `tryMap` and `tryForEach` explicitly interpret typed errors: stop on the
  first observed `Err`, stop scheduling new work, interrupt active children,
  and await their cleanup. Already completed side effects are not rolled back.
- Ordinary defects remain failures, not typed errors: clean up active children
  before completing a failed traversal. Do not silently inspect arbitrary
  payloads for `Err` in ordinary `map`.
- Parent cancellation stops new scheduling, interrupts children, and awaits
  cleanup in all variants, including error-collecting `map`.
- Preserve fairness, stack safety, scope ownership, provider context, and
  exactly-once completion. Test limits, ordering, empty inputs, callback throws,
  simultaneous completions/failures, and cancellation with bounded probes.

## All-settled distinction

`Fiber.map` over `Task[Result[a, e]]` is the agreed mechanism for collecting all
typed successes/errors. A broader settled-outcome API for defects and individual
interruption was discussed but is not approved for implementation yet. It would
need an explicit outcome type and must not defeat parent cancellation.

## Delivery gates

- Update SPEC, source/canonical ASTs, parser, inference, flow analysis, interfaces,
  direct AST lowering, runtime/stdlib, formatter, diagnostics, docs, and examples
  wherever affected. Migrate inferred-async examples and tests.
- Keep the original hardening invariants and regression cases; adapt their
  syntax without removing the failures they were designed to detect.
- Add source-aware, colorless diagnostic snapshots and actual CLI/cross-module
  execution tests. Validate typed error behavior and nested task representation.
- Revisit the exact Effect reference in `docs/effects-internals.md`, record
  deliberate divergences, and preserve attribution for adapted code.
- Run formatting, strict Clippy, full tests, affected release packaging, and
  changesets. These decisions are not implemented merely because recorded here.
