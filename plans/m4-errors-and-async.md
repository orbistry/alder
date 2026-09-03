# M4: Errors, async, and context

M4 has three related effect-like features: structural error rows on
`Result`, inferred `Task` functions, and compile-time-tracked context.
They share propagation ideas, but they do not share one undifferentiated
runtime or solver representation. Error rows land first as a complete
vertical slice; async and context follow without weakening that slice.

## Audited starting state (2026-09-03)

- The parser and source/canonical ASTs already represent `:tag(args)`, tag
  patterns, `error` groups, `[:tag(A) | r]`, `Result[a]`, `.await`, `?`,
  `use`, and `provide`.
- Canonicalization already publishes error groups and error-row annotations
  through interfaces. Owned `.aldi` serialization already preserves group
  tags, tag payloads, and row extensions.
- Error groups participate in M3 derives, but bare tags still carry
  `group: None`; their payload arity and types are not checked against a
  solved row.
- The solver currently collapses every error row to an opaque `Ty::ErrorRow`.
  Tag argument expressions are inferred and discarded, tag-pattern payloads
  receive unrelated fresh variables, and conversion back to the AST always
  emits the placeholder `[:_ | e]`.
- `?` already lowers to one evaluation plus an early `Err` return in codegen,
  but inference only unwraps `Result[a, e]`; it does not include `e` in the
  enclosing function's row.
- There is no post-solve exhaustiveness pass yet. M4 must add one rather than
  "extend the M2 pass" described by the old plan.
- `.await` currently requires an explicitly declared `Task` return during
  canonicalization and emits JavaScript `await` in an `async` function. The
  generator/fiber design and async inference have not landed.
- `alder-report` already owns miette-backed diagnostics and source-span
  helpers. Parser and compiler diagnostics are rendered by the driver, with
  color disabled only in snapshot tests.
- Oxc/Rolldown ASTs are the codegen boundary. Compiler-generated modules are
  built as AST nodes, not JavaScript string concatenation. Auditable kernel
  and standard-library sources may remain checked-in JS/TS/Alder files.

## Exit criteria

### Error-row vertical slice

- `Result[a]` introduces an inferred open error row. Tags preserve positional
  payload types, `?` includes every propagated tag, and an explicitly closed
  row rejects undeclared tags.
- Named `error` groups normalize to closed structural rows. Using `?` inside
  an open-row function flattens the group; explicitly naming the group keeps
  the accepted set closed.
- Tags are legal only in `Result`'s error position. Row variables are kinded
  separately from ordinary types and record rows.
- Matching a closed row must cover every tag (or `_`); matching an open row
  requires `_`. Tag patterns receive the row's payload types and arity.
- Public inferred rows survive interfaces and `.aldi` round trips with stable
  ordering and deterministic display names.
- Miette diagnostics cover row mismatch, missing/extra tags, payload arity and
  payload type errors, illegal tag placement, and non-exhaustive/open matches.
  Rendered diagnostics are snapshot-tested without color using Alder source,
  not the Rust expression that produced it.
- Existing tag and `?` runtime representations remain allocation-minimal and
  are covered by codegen plus standalone end-to-end execution tests.

### Remaining M4 features

- A function containing `.await` is inferred to return `Task[..]`; task
  functions lower to generator functions and execute on the kernel fiber
  scheduler. `main` may return `Task`.
- `.await?` remains ordinary postfix composition. A stage such as
  `request |> send(client).await?` forwards into `send` before applying
  `.await` and `?`; no fused `await?` construct or do-notation is introduced.
- `Fiber.fork/all/race/scope`, interruption, and structured concurrency work.
- `use Provider` requirements propagate through calls; `provide` discharges
  them lexically; unresolved entry-point requirements are diagnostics.

## Settled decisions

- Error rows exist only as `Result` error arguments. They are structural;
  named groups are aliases for closed rows, never runtime wrappers.
- `Result[a]` means `Result[a, [: | fresh_row]]`.
- Error-row tails, record-row tails, and ordinary type variables have distinct
  kinds. Cross-kind unification is an error.
- `?` adds a row-inclusion constraint from the operand into the enclosing
  `Result`; it does not equate both rows.
- Public functions may infer rows in M4. Interfaces expose the inferred row;
  semver-surface policy remains an M9 concern.
- User-facing rows omit internal tail names: `[:a | :b]` is closed and
  `[:a | :b | _]` is open. Internal/debug output may show stable variables.
- Diagnostics use `thiserror` for Rust error plumbing and
  `miette::Diagnostic` through `alder-report`. Normal CLI rendering keeps
  color when supported; deterministic snapshots explicitly disable it.
- `.await` stays postfix because `.await?` composes naturally. Pipe lowering
  reaches the destination call through postfix wrappers before those wrappers
  execute.
- `Task[Result[a]]` is the combined shape; there is no `TaskResult` type.
- Panics are not user-catchable. Framework error boundaries arrive in M6.

## Work breakdown

### Wave 0: audited contract

- [x] Audit parser, AST, canonicalization, solver, interfaces, diagnostics,
  codegen, stdlib, and runtime against current main.
- [x] Replace the stale starting-state assumptions in this plan.
- [x] Write `docs/effects-internals.md`, including row algorithms,
  diagnostics, async/context boundaries, and the postfix-await pipe rule.

### Wave 1: error-row model and inference

- [ ] Add kinded error-row variables, structural tag maps, payload vectors,
  closed/open tails, pruning, occurs checks, generalization, instantiation,
  and deterministic AST conversion in `alder-solve`.
- [ ] Normalize local and imported named error groups to closed rows.
- [ ] Infer singleton tag rows, enforce legal placement, and type tag-pattern
  payloads from the expected row.
- [ ] Implement row equality plus row inclusion for `?`, including multiple
  propagated calls and closed-row rejection.
- [ ] Preserve inferred rows through public interfaces and `.aldi` hydration.

### Wave 2: checking and reporting

- [ ] Add the post-solve match checker for closed and open error rows, treating
  guarded arms as non-covering.
- [ ] Add structured canonicalization/solve errors and Elm-quality wording,
  labels, hints, and related spans through `alder-report`.
- [ ] Snapshot semantic values with the project source-aware macros and
  `indoc`; snapshot rendered miette output with color disabled.

### Wave 3: runtime vertical slice

- [ ] Audit/update `Result` stdlib annotations and helpers for inferred rows.
- [ ] Verify Oxc AST lowering for tags, tag patterns, and one-evaluation `?`;
  add missing direct-AST cases without source-string code generation.
- [ ] Add runtime/e2e projects covering tag payloads, three-way `?` merging,
  group flattening, closed matching, and propagated `Err` identity.

### Wave 4: async and context

- [ ] Remove the explicit-Task canonicalization gate and infer asyncness.
- [ ] Lower task functions/await to generators/`yield*`; implement scheduler,
  fiber operations, scopes, interruption, and async entry points.
- [ ] Implement provider requirements, lexical discharge, interface storage,
  context propagation, and entry-point validation.

### Wave 5: sweep

- [ ] Update `SPEC.md`, language/runtime/tooling docs, and milestone checkboxes.
- [ ] Add granular Sampo changesets for every changed publishable crate.
- [ ] Run `cargo fmt --all`, full Clippy with warnings denied, full tests,
  package verification for changed crates, and standalone e2e execution.

## Required error-row tests

- Inference of one tag and an open row from `Result[a]`.
- Union of tags propagated by `?` across at least three calls.
- Repeated tag payload unification, including arity and type mismatch.
- Named local and imported group flattening; explicit group closure rejecting
  an extra tag.
- Tag expression outside a `Result` error position.
- Exhaustive closed match, missing closed tag, wildcard closed match, and open
  match both with and without `_`; guarded tag arms do not close coverage.
- Public inferred row interface round trip and deterministic ordering.
- Direct-AST codegen snapshots and standalone execution of success and each
  propagated error path.

## Risks and guards

- Row equality is not row inclusion. Keep separate APIs and tests so `?`
  cannot accidentally erase errors or require callers to have identical rows.
- Duplicate labels with unequal payloads can make row unification order
  dependent. Canonicalize maps by tag name and unify payload positions before
  reconciling tails.
- Named groups may be recursive through aliases. Normalize with a visited set
  and report a structured cycle instead of recursing indefinitely.
- Exhaustiveness needs solved subject rows. Record match sites during inference
  and check them only after substitutions are fully pruned.
- Error rows are erased at runtime. No type-system fix may change the stable
  `{ $: ":tag", _0: ... }` representation or evaluate a `?` operand twice.
- Generator tasks add a frame per async call. Keep non-task functions plain
  and retain `.await?` as composition rather than a combined runtime primitive.
