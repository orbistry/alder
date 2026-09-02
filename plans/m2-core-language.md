# M2: Core language to JavaScript

Two gated halves. **M2a** makes the front end (canonical AST,
canonicalization, type inference, driver, `alder check`) work on the new
source AST and turns the workspace green again. **M2b** adds JavaScript
code generation, the TypeScript kernel, the embedded runtime, and the
first `alder run` / `alder build` / `alder fmt` / `alder test`. M2a merges
to `main` on its own; M2b follows.

## Starting state

- `alder-source` and `alder-parse` implement the M1 grammar (1288 tests).
  Contract: `docs/parser-internals.md`. Its §12 lists deferred findings;
  §10.40 records that `provide` is a statement in the parser.
- `alder-ast` (canonical AST), `alder-can`, `alder-constrain`,
  `alder-solve`, `alder-driver`, `alder-cli`, `alder-language-server` do
  not compile. First error: `alder_source::Docs` no longer exists. Every
  test in `alder-can` (about 180 snapshots) and `alder-solve/tests`
  (33) is written in Elm syntax and must be rewritten.
- `alder-config` has `target: cloudflare | standalone`, no
  `exposedModules`, no `sourceDirectories` (sources are `src/`).
- Reference material: `elm/compiler/src/{Canonicalize,Type,Optimize,
Generate}` and the current Rust ports of Canonicalize and Type.

## Exit criteria

M2a:

- `cargo build --workspace`, clippy, and `cargo test` green; CI green.
- `alder check` type-checks a multi-module `standalone` project written
  in the new syntax, including `~/` imports, `mod.ald` indexes, `pub`
  visibility, enums with namespaced constructors, records with optional
  fields, `let mut`, loops, `match`, lambdas with `_` placeholders, and
  reports Elm-quality errors for name resolution and type mismatches.
- Every docs example that is a full module canonicalizes; those using
  traits, error rows, `.await`, `use`/`provide`, markup, queries, styles,
  tables, schemas, components, or macros are accepted structurally (see
  "Deferred constructs") without type errors.

M2b:

- `alder run` executes a `standalone` program with `fn main()` on the
  embedded runtime, with `Result` from `main` mapped to the exit code.
- `alder build` emits a bundled ESM artifact per target via rolldown.
- `alder fmt` formats every `.ald` file in the repo idempotently and
  preserves comments.
- `alder test` runs `test` declarations and `tests` blocks with plain
  pass/fail output.
- A first-party stdlib compiles from Alder sources and is reachable via
  the prelude.

## Settled decisions

These are already in the docs; do not reopen them.

- Runtime semantics: `Number` is a JS double, `BigInt` is JS bigint,
  `Array` is a mutable JS array, `Option[a]` compiles to `a | null` with
  nested options boxed, records are plain objects, `mut` is a binding
  permission with JS aliasing.
- `provide Path = expr { ... }` becomes an **expression** whose value is
  its block's tail (parser change: `Stmt::Provide` → `Expr::Provide`,
  parsed by `primary` at the `provide` keyword; `docs/web.md`'s `handle`
  example depends on it).
- Stdlib modules are bound with **capitalized names** by the prelude
  (`Array.map`, `Http.get`, `Fiber.all`) even though user modules bind
  lowercase; `docs/language.md` gets a sentence saying so.
- Operator precedence is resolved in canonicalization from the fixed
  table in `alder-source` (`BinOp::precedence()`), never in the parser.
- Constructors are qualified except inside `match` arms and the prelude's
  `Some`/`None`/`Ok`/`Err`.
- No `self`, no method-call sugar, no explicit type arguments at call
  sites.
- Kernel is TypeScript, built by rolldown/oxc inside the compiler, shipped
  as JS. No Node compatibility (`deno_node` is not embedded).
- Test runner in M2b is minimal: pass/fail, no power-assert (M9).

## Open decisions (recommendation in bold)

1. Enum runtime representation. **`{ $: "Some", _0: x }` for tuple
   variants and `{ $: "Rect", width, height }` for record variants**, with
   the tag as a short string. Unit variants are shared frozen singletons.
   Alternative: arrays `["Some", x]` (smaller, worse debugging). Decide in
   the codegen design contract; it must be stable before the kernel's SSR
   serialization exists.
2. Blocks in expression position (`let x = if c { ... } else { ... }`).
   **Lift to statements with a temporary**, never IIFEs; the codegen keeps
   a statement context and hoists. Only closures capture blocks.
3. Pattern-match compilation. **Port Elm's decision-tree compiler**
   (`Optimize/DecisionTree.hs`) so `match` compiles to nested `if`/`switch`
   without allocation.
4. Where the stdlib lives. **`std/` at the repo root as Alder source,
   embedded into the `alder` binary at build time** (include_dir-style),
   so `alder run` needs no install step. Alternative: a published
   `@alder/core` package resolved like any other; do that in M9 once the
   registry story exists.
5. Comments in the AST for `alder fmt`. **Side table**: the parser records
   `(Region, kind, text)` for every comment into a `Comments` list on the
   `Module`, attached to the nearest following node by the formatter.
   Doc comments (`///`, `//!`) keep their kind so M9's doc generator can
   use the same table.
6. Recursion between statements and generalization. **Top-level items are
   mutually recursive (SCC as today); `let` inside a block is sequential
   and not generalized** (Rust/JS model, avoids Elm's `let` polymorphism
   complexity). `let` inside `tests { }` follows the block rule.

## Deferred constructs (what M2 does with syntax that later milestones own)

Canonicalization resolves names inside these but the type checker treats
them as opaque so programs using them do not fail in M2:

| Construct                      | M2 treatment                                                         | Owner |
| ------------------------------ | -------------------------------------------------------------------- | ----- |
| `trait` / `impl` / bounds      | canonicalized, recorded in the env, constraints ignored              | M3    |
| `:tag(...)`, `error` groups    | `Err(:tag(x))` gets a fresh error type variable; rows not unified    | M4    |
| `.await`                       | typed `Task[a] -> a`; a fn using it must declare `Task[..]`          | M4    |
| `use` / `provide`              | `use Path` binds an opaque value of the provider type; no checking   | M4    |
| markup                         | typed `Html`, holes type-checked, elements/attrs not validated       | M6    |
| `component`, `state`           | `component` is a fn returning `Html`; `state(x)` is identity typed   | M6    |
| `query { }`, `^`               | typed `Query[r]` with `r` fresh; pins type-checked                   | M7    |
| `table`, `schema`              | declared as opaque types; bodies unchecked                           | M7    |
| `style { }`                    | typed `Style`; values type-checked, properties unchecked             | M8    |
| `macro`, `name!()`, `comptime` | error "macros are not available yet" at use sites; declarations kept | M5    |
| `#[extern]` fns                | typed from the signature; codegen emits the import                   | M2b   |
| `#[derive(...)]`               | error "derives are not available yet"                                | M5    |

## Work breakdown

### M2a, wave 0: canonical AST contract

Run a design panel (see `plans/README.md`) producing
`docs/canonical-internals.md` with, as pasteable Rust:

- The new `alder-ast` canonical AST: items (`Fn`, `Let`, `TypeAlias`,
  `Enum`, `Trait`, `Impl`, `ErrorGroup`, `Component`, `Table`, `Schema`,
  `Test`, `Macro`, `Extern`), statements and blocks, expressions
  (including `Provide`, `Await`, `Try`, `Pin`, `Markup`, `Query`, `Style`,
  `MacroCall`, `Placeholder` desugared away), patterns, types (with
  optional fields and error rows as row kinds), canonical names
  (`Module` + `Name`, package-qualified), and the module interface.
- The environment model for `alder-can`: per-module scopes for values,
  types, constructors (namespaced), traits, and modules; visibility;
  import resolution for `@author/pkg/path`, `~/path`, `mod.ald` and
  sibling `path.ald` indexes, `as`, `.{ }`, `.*`, `pub import`.
- The error type for canonicalization (port and extend
  `Reporting/Error/Canonicalize.hs`): unknown names with suggestions,
  ambiguous imports, non-`pub` access, assignment to immutable binding,
  `break`/`continue` outside loops, `return` outside functions, duplicate
  definitions, unqualified constructor outside `match`, placeholder
  outside call, `^` outside query, and the deferred-construct notices.
- The row model additions for the solver: optional-field rows and error
  rows (M4 fills the latter in but the representation is fixed now).
- File ownership for waves 1 and 2.

### M2a, wave 1: canonicalization (parallel owners)

- `alder-ast` rewrite to the contract (one owner, first).
- `alder-can/environment`: scopes, imports, prelude, visibility.
- `alder-can/items`: fn, let, type alias, enum, trait/impl recording,
  error groups, opaque declarations for table/schema/component/test/macro.
- `alder-can/expression` and `statement`: precedence resolution, `_`
  placeholder desugaring to lambdas, block scoping, `mut` and assignment
  checks, loop labels, `?` and `.await` nodes, `provide` as expression.
- `alder-can/pattern`: namespaced constructors, `match`-arm
  unqualified constructors, `^` pins, array rest patterns, optional-field
  record patterns.
- `alder-can/types`: type expressions with `[ ]` application, HKT
  variables, `fn(A) -> B`, records with `?` fields and rows, error rows.
- Tests: rewrite every existing `alder-can` snapshot test into the new
  syntax as the constructs land (owners split the list by construct);
  keep the granular one-construct-per-test style.

### M2a, wave 2: type inference and driver

- `alder-constrain`: constraints for statements/blocks (tail typing,
  `return` unifies with the declared or inferred return), loops (`()`),
  optional fields on construction and read, `.await`, `?` on `Result`
  (error variable free until M4), `provide`/`use` opaque, deferred
  constructs per the table.
- `alder-solve`: primitives (`Number`, `BigInt`, `String`, `Bool`,
  `Array`, `Map`, `Set`, `Task`, `Option`, `Result`, unit, tuples),
  optional-field row unification, sequential non-generalizing `let`,
  no numeric supertypes (Elm's `number`/`comparable` go away; `+` is
  `Number` in M2, traits fix this in M3).
- Error messages: port `Reporting/Error/Type.hs` rendering for the new
  type syntax (`Map[String, Number]`, `fn(a) -> b`, `{ x?: Number }`).
- `alder-driver`: interface (de)serialization for the new canonical AST,
  module graph from path-first imports, `src/` root, index resolution,
  `~/` and `@pkg` resolution across workspace members, `target` from
  config.
- `alder-cli`: `alder check` over `.ald`.
- Tests: rewrite `alder-solve/tests/inference.rs` cases into the new
  syntax; add inference tests for optional fields, blocks, loops,
  `return`, `_` placeholders, `?`.
- Gate: workspace builds, CI green. Merge M2a to `main`.

### M2b, wave 0: codegen and runtime contract

Design panel producing `docs/codegen-internals.md`:

- Runtime representation of every canonical type (open decision 1).
- Name mangling and module-to-file mapping; one ESM file per module;
  `export` for `pub` items; entry points per target.
- Statement lifting rules for expression-position blocks (open
  decision 2), decision trees for `match` (open decision 3).
- `#[extern("module", "name")]` → `import { name as alias } from
"module"`; `Result`-returning externs wrapped in try/catch by a kernel
  helper; `Task`-returning externs awaited by the scheduler (M4; in M2b
  `.await` compiles to `await` inside an `async` function and the fiber
  scheduler replaces it in M4).
- Kernel layout: `crates/alder-kernel` holding `kernel/src/*.ts`, built
  by rolldown/oxc from a `build.rs` into a `dist/` embedded with
  `include_str!`; public surface documented as the `extern` contract for
  the stdlib.
- Stdlib layout: `std/` Alder sources, embedded; prelude module and
  capitalized module bindings.
- `alder fmt` architecture: formatter over the source AST + comment side
  table, Wadler-style pretty printer (port ideas from `elm-format` only
  where the new grammar matches).
- Test runner: `test` blocks compile to a registry the runtime executes;
  `tests { }` items compile only under `alder test`.

### M2b, wave 1: parallel owners

- `alder-codegen`: expressions, statements, patterns (decision trees),
  items, modules, externs.
- `alder-kernel`: TypeScript sources for value helpers (enum
  construction, structural equality, string interpolation), Option
  boxing, `Result` try/catch wrapping, test registry; build script.
- `std/`: `Option`, `Result`, `Array`, `String`, `Number`, `BigInt`,
  `Map`, `Set`, `Json` (encode/decode as plain functions for now), `Io`
  (print), `Cli` (raw `args()`; the `Args`/`Subcommand` derives come with
  M5), `Task` and `Fiber` minimal (`all`, `race`, `sleep`).
- Parser: comment side table (open decision 5) and `provide` as an
  expression.
- `alder-fmt` crate.
- `alder-cli`: embed `deno_core` + `deno_web`, `deno_url`,
  `deno_console`, `deno_fetch`, `deno_crypto`, `deno_net`, `deno_http`,
  `deno_fs`; `alder run`; `alder build` via rolldown as a library;
  `alder test`; `alder fmt`.

### M2b, wave 2: integration and sweep

- End-to-end tests: a `tests/e2e/` directory of small `standalone`
  projects (hello world, enums and match, records with optional fields,
  loops and `mut`, externs to `node:`-free web APIs like `fetch` against a
  local `deno_http` server) run through `alder build` and `alder run`
  with expected stdout.
- Every docs example that is a full `standalone` module compiles and,
  where it has a `main`, runs.
- `alder fmt --check` runs in CI over `std/` and `tests/e2e/`.
- Critic pass, docs and SPEC updates, changeset.

## Tests to add (minimum)

- `alder-can`: one snapshot per resolution rule listed in the error type,
  success and failure.
- `alder-solve`: one inference test per new typing rule; regression tests
  for optional-field row unification with three or more fields and
  nested records.
- `alder-codegen`: snapshot the emitted JS per construct (small inputs),
  plus a runtime assertion suite executed under deno_core in `cargo test`
  (a `#[test]` that builds and runs each e2e project).
- `alder-fmt`: idempotency (`fmt(fmt(x)) == fmt(x)`) over every `.ald`
  in the repo and every docs example; comment preservation tests.

## Risks

- The canonical AST is the widest blast radius in the compiler; the
  design contract must be complete before wave 1 or owners will collide
  in `alder-ast`.
- Rewriting 200+ Elm-syntax tests is tedious and easy to do wrong; each
  rewritten test's snapshot must be re-read, not just accepted.
- `deno_core` and the extension crates move in lockstep; pin one version
  set and record it in `docs/runtime.md`.
- rolldown as a library is young; keep the integration behind a single
  module so it can be swapped.
- `alder fmt` needs comments the parser currently drops; adding the side
  table touches the parser's hot path, so measure.
