# M5: Macros and comptime

Real macros: Alder functions from syntax to syntax, compiled to JavaScript
and executed at build time inside the compiler's embedded V8, with
Elixir-style `quote`/`unquote` and Jai-style `comptime` blocks. Attribute,
derive, and function-like forms. Derives replace the compiler-built-in
ones from M3.

## Starting state

- Parser: `macro name(params) { raw }` and `name!( raw )` keep bodies and
  arguments as raw balanced text (`Located<&str>`); `comptime { }` is a
  block; `#[derive(...)]` is an attribute.
- M2b: the compiler embeds deno_core and can compile and run Alder.
- M3: built-in derives for `Show`, `Eq`, `Ord`, `Hash`, `Json`.

## Exit criteria

- A macro defined in a package can be used by another package in the
  three forms: `name!(tokens)`, `#[name(args)]` on an item, and
  `#[derive(Name)]` on a type.
- `quote { ... unquote(x) ... }` builds syntax with hygiene; `stringify`
  exists.
- `comptime { }` runs at build time and its value is spliced as a
  literal or as syntax.
- Built-in derives are reimplemented as macros in `std/` with identical
  output, and `Cli`'s `Args` and `Subcommand` derives ship.
- Macro expansion is cached per module and invalidated by the macro's
  source hash and its inputs; expansion is sandboxed (no I/O except the
  documented compile-time API) and bounded (time and recursion).
- Error messages from inside expansions point at the use site and,
  in verbose mode, the expansion site.

## Settled decisions

- Macros are Alder code run in V8 at build time; no Rust plugins, no
  declarative-only macros.
- Hygiene is required; the exact scheme is open decision 1.
- Macro bodies were parsed as raw text in M1 deliberately; M5 parses
  them.

## Open decisions (recommendation in bold)

1. Hygiene scheme. **Syntax-context marks on identifiers introduced by
   `quote` (Rust `macro_rules` hygiene for locals; items and paths resolve
   at the definition site unless `unquote`d).**
2. Syntax API surface. **A `Syntax` module exposing an AST-shaped data
   model (not raw tokens) that mirrors `alder-source` closely enough that
   the compiler can convert both ways; `TokenStream` only for the raw
   input of function-like macros.** The mirror is generated from
   `alder-source` by a build step so it cannot drift.
3. Expansion phase. **Expand after parsing and before canonicalization
   per module, in dependency order, so a macro can be defined in a
   dependency and used immediately; macros cannot see types.** Type-aware
   macros are out of scope.
4. Compile-time I/O. **Read-only access to the package source tree
   (`Fs.readDir`, `Fs.readFile`) for things like route discovery;
   nothing else.** Network and writes are denied.
5. Caching. **Content-addressed on (macro source hash, dependency
   interface hashes, input tokens); stored under `.alder/macros/`.**

## Work breakdown

### Wave 0: contract

Design panel producing `docs/macros-internals.md`:

- The syntax data model and its two-way conversion with `alder-source`
  (generated code, arena-aware), `quote`/`unquote` semantics, hygiene
  marks and how canonicalization respects them, the expansion pipeline
  and error mapping, the compile-time API and sandbox, caching, the
  derive protocol (input: the item's syntax; output: items to append),
  the attribute protocol (input: item + args; output: replacement items),
  and the function-like protocol.
- Codegen for macro bodies (they are ordinary Alder functions with
  `Syntax` types) and the host bridge in Rust (invoke a compiled macro
  bundle in deno_core, marshal syntax as JSON or a binary form, collect
  diagnostics).

### Wave 1 (parallel)

- Parser: parse macro bodies and `name!()` arguments into syntax
  (replacing raw text), `quote`/`unquote`/`stringify` forms, `comptime`
  block typing hook.
- `alder-syntax` (new crate or module): the data model, conversions,
  hygiene marks, serialization.
- Expansion driver in `alder-driver`: ordering, caching, sandboxed
  execution via deno_core, diagnostics mapping.
- `std/`: `Syntax` and `Macro` modules; `Fs` read-only compile-time API.

### Wave 2 (parallel)

- Derives as macros: `Show`, `Eq`, `Ord`, `Hash`, `Json`, `Args`,
  `Subcommand`; remove the M3 built-ins once byte-identical output is
  verified by snapshot.
- `comptime` blocks: evaluation, literal splicing, syntax splicing.
- e2e: a package defining a macro used by an app; a derive on an enum with
  record variants; a `comptime` that reads a directory.

### Wave 3: sweep

- Docs, SPEC M5 ticked, changeset, critic pass (every macro example in
  `language.md`), fuzz the expander with malformed macro output.

## Tests to add (minimum)

- Syntax round trip: every parser snapshot input converts to `Syntax` and
  back to an identical source AST.
- Hygiene: a macro-introduced `let x` does not capture a user `x`; an
  `unquote`d identifier does.
- Derives: output snapshots per derive per type shape; M3-versus-M5
  equivalence.
- Sandbox: a macro attempting `fetch` or a write fails with a clear error;
  a runaway macro is stopped by the time bound.
- Caching: unchanged inputs skip execution (assert via a counter exposed
  in tests).

## Risks

- A two-way syntax model that mirrors `alder-source` is a large surface;
  generating it is the only way to keep it in sync.
- Hygiene interacts with canonicalization's scoping; the contract must
  spell out how marks are compared during resolution.
- Compile-time execution makes builds nondeterministic if I/O leaks;
  keep the sandbox strict from day one.
