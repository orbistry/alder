# M4: Errors, async, and context

Three features that share the type system's row machinery and the
runtime's scheduler: open error rows on `Result`, inferred `Task` from
`.await` running on generator-based fibers, and `use`/`provide` context
resolved at compile time.

## Starting state

- M2: `Err(:tag(x))` gets a free error type variable; `.await` is typed
  `Task[a] -> a` with the enclosing function required to declare `Task`;
  `.await` compiles to JS `await` in an `async` function; `use`/`provide`
  are opaque.
- M3: traits exist (`Task` will get `Functor`/`Monad` instances).
- Parser: `:tag(args)` in expressions and patterns, `error` groups,
  `[:tag(A) | r]` error rows in types, `Result[a]` shorthand, `provide`
  as an expression (M2b).

## Exit criteria

- `fn find(id: Id) Result[User]` infers `[:not_found(Id) | r]` from the
  body; `?` merges rows across calls; a named `error` group flattens into
  an open row on `?` and closes it when written explicitly; `match` on a
  closed row is exhaustive and an open row requires `_`; hover, docs, and
  the emitted `.d.ts` (M8) show the inferred row.
- A function using `.await` is inferred to return `Task[..]`; `Task` is a
  visible type; un-awaited calls are `Task` values usable with
  `Fiber.fork`, `Fiber.all`, `Fiber.race`, `Fiber.scope`; interruption and
  structured concurrency work on the generator-based scheduler in the
  kernel; `main` may be a `Task`.
- `use Db` inside a function makes the function require a `Db` provider;
  `provide Db = value { ... }` satisfies it lexically through the call
  graph; a missing provider at an entry point is a compile error naming
  the path; tests swap providers.

## Settled decisions

- Error rows exist only in `Result`'s error position; `:tag` elsewhere is
  a type error. Tags carry positional payloads.
- `Result[a]` (one argument) means an inferred error row.
- Named groups are names for closed rows and never wrappers; `?` on a
  group inside an open-row function flattens it.
- `Task` is visible and writable in signatures; asyncness is inferred
  from `.await` (no `async` keyword).
- Fibers are generator-based (`yield*`), Effect-TS style, with structured
  concurrency, interruption, and scopes, implemented in the TypeScript
  kernel; user code never sees generators.
- Context is lexical through the call graph and the render tree; missing
  providers are compile errors at entry points.
- Panics are not catchable by user code; the framework installs error
  boundaries (M6).

## Open decisions (recommendation in bold)

1. Should `pub` functions be required to spell their error row? **No in
   M4; emit the inferred row into interfaces and docs, and revisit when
   semver diffing (M9) needs stable surfaces.**
2. Row variable naming in messages. **Render open rows as
   `[:a | :b | ...]` and closed as `[:a | :b]`; never show the row
   variable's internal name.**
3. Context representation at runtime. **A fiber-local map keyed by
   provider type id, propagated by the scheduler (AsyncLocalStorage
   semantics without Node), captured on `Fiber.fork`.**
4. `Task` and `Result` interplay. **`Task[Result[a]]` is the shape; `?`
   inside a `Task`-returning function works on the inner `Result` after
   `.await` (`f().await?`), and there is no combined `TaskResult` type.**
5. Interruption semantics. **Cooperative at `yield` points (every
   `.await`); `Fiber.scope` cancels children on exit; a cancelled fiber's
   pending `Result` is `Err(:interrupted)`.**

## Work breakdown

### Wave 0: contract

Design panel producing `docs/effects-internals.md`:

- Error rows in the solver: row kinds (record rows, optional-field rows,
  error rows share the union-find representation), tag unification, row
  merging for `?`, group flattening, closure on explicit annotation,
  exhaustiveness over closed rows (extends the M2 exhaustiveness pass).
- `Task` inference: a per-function "awaits" flag collected during
  constraint generation, the return type wrapped in `Task` when set and
  not already declared; call sites of `Task` functions without `.await`
  are `Task` values; the entry point runner.
- Context: `use` collects a provider requirement per function;
  requirements propagate up the call graph like effects (a lightweight
  effect row for providers only); `provide` discharges; entry points must
  have an empty requirement set; interfaces carry requirements.
- Kernel scheduler API: `Fiber` primitives, scopes, interruption, the
  generator protocol between compiled functions and the scheduler,
  context propagation, and the JS calling convention for `Task`
  functions (generator functions, `yield*` for awaits).
- Codegen: `.await` → `yield*`, `Task` functions → generator functions,
  `?` → early return, `provide` → scheduler call with a scope.
- Error types and rendering.

### Wave 1: front end (parallel)

- `alder-constrain` + `alder-solve`: error rows, `?` merging, groups,
  `Task` inference, provider requirements.
- `alder-can`: `error` groups in the env, `:tag` arity checks, `use`
  target validation (must name a provider type), exhaustiveness pass
  extension.
- Error rendering.
- Tests: inference tests for every rule; snapshot errors.

### Wave 2: runtime (parallel)

- `alder-kernel`: scheduler (fibers, scopes, interruption, timers,
  `Fiber.all/race/fork/join/sleep`), context map, entry runner, error
  boundary hook for M6.
- `alder-codegen`: generator emission, `?` lowering, `provide` lowering,
  `Task` value creation for un-awaited calls.
- `std/`: `Task`, `Fiber`, `Result` helpers with rows, `Task`
  `Functor`/`Monad` instances.
- e2e: concurrent fetches with `Fiber.all`, cancellation with
  `Fiber.race`, context swap in tests.

### Wave 3: sweep

- Docs (`language.md` Errors/Async/Context, `runtime.md` kernel), SPEC M4
  ticked, changeset, critic pass.

## Tests to add (minimum)

- Rows: inference of open rows across three calls; group flatten;
  explicit close rejects an extra tag; exhaustiveness with and without
  `_`; `:tag` outside `Result` rejected; arity mismatch on a tag.
- Task: inferred from nested `.await`; declared `Task` without `.await`;
  un-awaited call typed `Task`; `main` as `Task`.
- Context: requirement propagation through two calls; provider inside a
  lambda passed elsewhere (lexical, not dynamic); missing provider at
  entry with the path in the message.
- Kernel: scheduler unit tests in TypeScript (run via deno_core in
  `cargo test`): fairness, interruption of sleeping fibers, scope
  cleanup on error, context isolation between forked fibers.

## Risks

- Error rows and record rows in one union-find must not unify with each
  other; tag the row kind and test the cross case.
- Generator-based `Task` functions cost a frame per call; keep
  non-`Task` functions plain and make sure inference never wraps a
  function that does not await.
- Context as an effect row is the first effect-like thing in the type
  system; keep it minimal (provider set only) so it does not become
  general effect tracking by accident.
