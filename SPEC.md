# Alder Language Specification

**Status: current direction, everything provisional.** The language was
redesigned on 2026-09-01. The parser (`alder-parse`, M1) implements the
grammar at the end of this document; canonicalization and the solver
are still the Elm port and are adapted in M2. Design rationale lives in
`docs/`:

- `docs/language.md` — syntax and semantics
- `docs/runtime.md` — targets, JS output, kernel, embedded V8, Cloudflare
- `docs/web.md` — routing, rendering, reactivity, styles, forms, TUI
- `docs/data.md` — tables, queries, migrations
- `docs/tooling.md` — CLI, dev server, tests, packages

## Compilation Target

Alder compiles to **JavaScript**, with a special focus on targeting
**Cloudflare**, and also runs as a general-purpose language on an
embedded V8 (`deno_core`) for servers and TUIs.

- Fork of the Elm compiler, ported to Rust. Type inference and rows are
  Elm's; syntax and runtime semantics are not.
- No TEA. Components with compile-time-tracked signals, SSR, stores.
- Built-in data layer: `table`, SQL-shaped queries, migrations, `schema`.
- Curly-brace syntax, `fn`/`enum`/`match`/`pub`/`trait`, `.await`,
  open `:tag` errors on `Result`, real macros.
- Source files use `.ald`; interface cache files use `.aldi` under `.alder/`.

---

## Pipeline

Elm's pipeline per module: parse → canonicalize → constrain → solve →
interface, then codegen. Interfaces only exist for solved modules; the
driver compiles in dependency order.

```
orbistry/alder/
├── crates/
│   ├── alder-region/           # Source spans/positions
│   ├── alder-source/           # Parsed AST types
│   ├── alder-parse/            # Parser for the grammar below (M1, done)
│   ├── alder-ast/              # Canonical/typed AST types
│   ├── alder-can/              # Canonicalization
│   ├── alder-constrain/        # Type constraint generation
│   ├── alder-solve/            # Constraint solving (type inference)
│   ├── alder-config/           # Project configuration (JSONC)
│   ├── alder-driver/           # Build orchestration, FileSource
│   ├── alder-codegen/          # JavaScript code generation (TBD)
│   ├── alder-language-server/  # LSP implementation
│   └── alder-cli/              # CLI binary (`alder`), embeds deno_core (TBD)
├── docs/                       # Design documents
├── tasks/                      # Workspace tasks (currently a stub)
├── .alder/                     # Build artifacts (gitignored)
└── alder.jsonc                 # Project config
```

---

## Foundation (Elm port, done)

These crates are complete against Elm's semantics and Elm's syntax. They
are kept through the redesign and adapted incrementally.

### Project configuration (`alder-config`) ✅

**Goal:** Define project configuration types and parsing.

**Status:** Complete

**Architecture Decisions:**

- **Format:** JSONC (`alder.jsonc`) - JSON with comments, parsed via `jsonc-parser`
- **Config types:** Three separate types: `application`, `package`, `workspace`
- **Field naming:** camelCase (`testDependencies`)
- **Target:** `target` (`cloudflare` | `standalone`) is required on applications and optional on packages (absent = target-neutral); the web framework is enabled by `src/routes/`, not by the target
- **Lock file:** Single shared `alder.lock` at workspace root (TBD in driver)
- **Dependencies:** Runtime-agnostic (no mandatory core deps), `author/project` naming
- **Dependency syntax:** Constraint string or object (`{ "workspace": true }`, `{ "path": "..." }`, `{ "git": "..." }`)
- **Source discovery:** fixed at `src/` (one root per package, so `~/` is unambiguous)
- **Error messages:** Line/column accurate via AST-based parsing

**Config Types:**

```jsonc
// Application - compiles to a JavaScript app
{
    "type": "application",
    "target": "cloudflare",
    "dependencies": {
        "alder/core": "1.0.0 <= v < 2.0.0",
        "alice/json": { "workspace": true },
        "bob/lib": { "path": "../lib" },
        "carol/experimental": { "git": "https://...", "branch": "main" }
    },
    "testDependencies": { }
}

// Package - publishable library
{
    "type": "package",
    "name": "author/package",
    "version": "1.0.0",
    "summary": "Short description",
    "license": "MIT",
    "target": "cloudflare",           // optional; absent = target-neutral
    "dependencies": { },
    "testDependencies": { }
}

// Workspace - collection of projects sharing dependencies
{
    "type": "workspace",
    "members": ["packages/*", "apps/my-app"],
    "dependencies": {
        "alder/core": "1.0.0 <= v < 2.0.0"  // available for { "workspace": true }
    }
}
```

**Crate contents:**

- `config.rs` - Config, Application, Package, Workspace, Dependency types
- `parse.rs` - AST-based JSONC parsing with position tracking
- `error.rs` - Position-aware error types
- `name.rs` - PackageName (`author/project` format)
- `alder.schema.json` - JSON Schema for IDE support
- `README.md` - Documentation

**Reference:** `elm/builder/src/Elm/Outline.hs`

---

### Driver and build system (`alder-driver`) ✅

**Goal:** File I/O abstraction, dependency graph, caching infrastructure.

**Architecture Decisions:**

- **Async model:** Runtime-agnostic (async traits, entry points pick runtime)
- **FileSource trait:** In driver crate with implementations:
  - `FileSystemSource` (native, `#[cfg(not(wasm32))]`)
  - `InMemorySource` (universal, for LSP unsaved buffers)
  - `OverlaySource` (composition: InMemory overlays FileSystem)
- **Build model:** Elm's pipeline per module — parse → canonicalize →
  constrain → solve → interface. Interfaces only exist for solved
  modules: the driver compiles in dependency order and deep-copies each
  solved module's interface into a build-wide arena for its dependents.
  Sources are fetched in parallel, but type checking is
  dependency-ordered.
- **Caching:** Interface-only (always regenerate JavaScript), bincode serialization
- **Invalidation:** Reverse dependency tracking for LSP
- **Arenas:** Per-module bumpalo arenas

**Implementation (`crates/alder-driver/`):**

- `source.rs`: `FileSource` trait + `FileSystemSource`, `InMemorySource`, `OverlaySource`
- `database.rs`: Compilation database with source caching and dependency tracking
- `project.rs`: Project loading from `alder.jsonc`, workspace member discovery
- `graph.rs`: Dependency graph construction with topological sort and cycle detection
- `compile.rs`: Compilation orchestration (async source fetch, CPU-bound work off the executor)
- `interface.rs`: Interface serialization with bincode for incremental builds
- `error.rs`: Driver error types with miette diagnostics

**CLI (`crates/alder-cli/`):**

- `alder check [PATH]` - Type check a Alder project

**Reference:** `polarity/lang/driver/`, `elm/builder/src/Build.hs`

---

### Canonicalization (`alder-can`) ✅

**Goal:** Name resolution, scope checking, desugar syntax.

**Transforms:**

- Resolve all names to fully qualified form
- Check for duplicate definitions
- Validate imports (module exists, exposed items exist)
- Desugar operators to function calls
- Bind type variables
- Collect module interface (public types, values)

**Reference:** `elm/compiler/src/Canonicalize/`

---

### Type inference (`alder-constrain` + `alder-solve`) ✅

**Goal:** Hindley-Milner type inference via constraint generation and
rank-based solving, ported from Elm's `Type/*`.

**alder-constrain:**

- Generate type constraints from the canonical AST, with the expectation
  contexts Elm uses for error messages (`Expected`/`Category`/...)
- Pattern constraints with binding headers
- Shared vocabulary: union-find variables, descriptors, inference types

**alder-solve:**

- Weight-balanced union-find unification with number/comparable/appendable
  supertypes, extensible records, and aliases
- Let-polymorphism via rank-based generalization (Elm's pools)
- Occurs check and infinite-type errors
- `toAnnotation`: solved variables back to canonical annotations, feeding
  `Interface::from_module`
- Exhaustiveness checking is a later post-solve pass (Elm's
  `Nitpick/PatternMatches.hs`), not part of the solver

**Reference:** `elm/compiler/src/Type/`

---

---

## Roadmap

Ordered. Each milestone is the task list for that phase; check items off
as they land and update the grammar section alongside. `plans/` holds a
detailed plan per remaining milestone (starting state, exit criteria,
settled and open decisions, waves, tests, risks).

### M1: Parser rewrite (`alder-parse`)

New parser for the grammar below (`crates/alder-parse` over
`crates/alder-source`), with snapshot tests per construct. Done; the
design and its decisions are in `docs/parser-internals.md`. `alder-ast`,
`alder-can`, `alder-constrain`, `alder-solve`, `alder-driver`, the CLI
and the language server still consume the old AST and are red until M2
adapts them; the workspace build and CI are red on the branch until
then, by design.

- [x] Lexer: `//` comments, template literals, `:tag` tokens, `#[`, `::`, `=>`, `->`, `|>`, `??`, `?`, `^`, `@if`/`@for`/`@match`
- [x] Items: `pub`, path-first `import` with `.{ }`/`.*`/`as`, re-exports (`pub import`)
- [x] `fn` declarations and lambdas, optional `-> Type` after params
- [x] Statements: `let`/`let mut`, assignment and compound assignment, `for`, `while`, `loop`, `break`/`continue` with values, `return`, `assert`
- [x] Expressions: blocks, `if`/`else if`, `match` with `=>` and guards, `|>`, `.await`, `?`, `??`, calls, `_` placeholders, field/tuple access, paths (`Option::Some`)
- [x] Literals: numbers (JS semantics), template literals, arrays, tuples, records with spread and optional fields
- [x] Types: `Name[a, b]`, `fn(A) -> B`, tuples, records with `?` fields and rows, error rows `[:tag(A) | r]`, `Result[a]` shorthand, `where` clauses
- [x] `type` aliases, `enum` with tuple and record variants
- [x] `trait` and `impl` (Haskell-style, HKT params, associated types, default bodies)
- [x] `error` groups
- [x] `#[attr]` attributes on items
- [x] `tests { }` blocks and `test "name" { }`
- [x] `#[extern(...)]` bodiless functions and `#[extern] type` (the parser accepts them anywhere; the attribute check is canonicalization's, §10.26)
- [x] Typed markup expressions with `{expr}` holes and `@if`/`@else`/`@for`/`@empty`/`@match` directives in child position
- [x] `component`, `table`, `schema`, `style`, `query`, `macro`, `comptime` — every body is parsed; `macro` bodies and `name!(…)` arguments are raw balanced text until M5 (§10.29)
- [x] Error hierarchy in `crates/alder-parse/src/error.rs` for the new constructs (nested like `Reporting/Error/Syntax.hs`; rendering is the M2 `alder fmt` / driver work)

### M2: Core language to JavaScript

- [x] Adapt `alder-can` to namespaced constructors, `pub` visibility, statements, `mut`
- [x] `alder-codegen`: JS emission for the core language; decide enum/record representation
- [x] Prelude and stdlib skeleton: `Option`, `Result`, `Array`, `String`, `Number`, `BigInt`, `Map`
- [x] JS kernel skeleton and `extern` binding
- [x] Embed `deno_core` plus the web-standard extension crates in `alder-cli`; `alder run` for the `standalone` target
- [x] `Cli` module (raw `args()`; the `Args`/`Subcommand` derives land with M5)
- [x] rolldown integration for `alder build`
- [x] `alder fmt` (comment side table in the parser)
- [x] Minimal `alder test` (pass/fail; power-assert and property tests are M9)
- [x] `provide ... { }` becomes an expression

### M3: Traits

- [x] Type-class constraints in `alder-constrain`/`alder-solve` (argument zero is the coherence subject; colon bounds are unary)
- [x] Higher-kinded type parameters
- [x] Dictionary-passing codegen with static resolution where possible
- [x] Compiler-backed derive surface (`Show`, `Eq`, `Ord`, `Hash`, `Json`; macro implementation replaces it in M5)
- [x] Orphan rule checking in `alder-can`

### M4: Errors and async

- [ ] Row-typed `:tag` errors in `Result`'s error position, `?` row merging, inferred rows for `Result[a]`
- [ ] `error` groups and their unification with open rows
- [ ] Exhaustiveness on closed groups, `_` requirement on open rows
- [ ] Inferred `Task` from `.await`; generator codegen; fiber scheduler in the kernel
- [ ] `provide`/`use` context resolution and compile-time provider checking

### M5: Macros and comptime

- [ ] Syntax API (`TokenStream`/AST) exposed to Alder
- [ ] Compile macros to JS and execute in embedded V8 during the build
- [ ] `quote`/`unquote`, attribute/derive/function-like forms, `comptime` blocks
- [ ] Hygiene, caching, sandboxing

### M6: Web vertical slice

- [ ] Typed markup checking against an HTML schema
- [ ] `component` and `state`, compile-time dependency tracking, DOM codegen
- [ ] SSR renderer and hydration in the kernel
- [ ] Folder routing: `+page.ald`, `+page.server.ald`, `+layout.ald`, `+layout.server.ald`, `+server.ald`, `+error.ald`, typed `Routes` and generated `PageData`
- [ ] `*.remote.ald` modules and `+page.server.ald` as server boundaries, reachability analysis, typed HTTP stubs
- [ ] `hooks.server.ald` / `hooks.client.ald` with `handle` providing typed request context
- [ ] Module stores, request-scoped on the server
- [ ] SvelteKit page options (`prerender`, `ssr`, `csr`) inherited down the route tree
- [ ] `alder dev` on vendored miniflare; `alder deploy` generating `wrangler.jsonc`
- [ ] Cloudflare bindings via traits and attributes

### M7: Data layer

- [ ] `table` declarations with dialect modules (`@alder/sqlite`, `@alder/postgres`, `@alder/mysql`)
- [ ] SQL-shaped query expressions with `^` pinned parameters, type-checked projections, desugar to chain API
- [ ] `alder db generate`/`migrate`/`push` with diff-generated SQL
- [ ] D1 and Hyperdrive drivers; embedded SQLite for `standalone`
- [ ] `schema` declarations with `from table` and validation rules

### M8: Styles, forms, API

- [ ] `style` blocks to atomic CSS with typed properties
- [ ] `Form`/`Field` components typed from `schema`
- [ ] Typed client generation from `+server.ald`; `.d.ts` emission
- [ ] Router builder for API-only packages

### M9: Tooling and ecosystem

- [ ] `test`/`tests` with power-assert, per-target runners
- [ ] Registry protocol, Rust client, fs and GitHub-releases adapters, `alder publish` with semver enforcement by API diff (the hosted registry service is deferred past M9)
- [ ] Language server features on the new grammar; WASM playground
- [ ] Documentation generator

### M10: TUI

- [ ] TUI element vocabulary and Rust-side layout/input in deno_core
- [ ] Terminal renderer over the shared signal graph

---

## Grammar

EBNF for the new syntax, as implemented by `alder-parse` in M1. Each
departure from the first draft is a numbered decision in
`docs/parser-internals.md` §10 (cited as §10.n); `?`-marked productions
are still open. `alder-can` decides what names denote (`Array.map` is an
access on a path, §10.15), whether a bodiless `fn` carries `#[extern]`
(§10.26) and how a flat operator chain nests (§10.1).

### Layout

There is no `;` and no indentation rule; line breaks are the only
separator (§10.3, §10.38). Whitespace (spaces, tabs, `\r`, `\n`) and
`//` comments are insignificant between tokens except inside template
literals and markup text, and except where a line break carries one of
these rules:

- **Items and statements are separated by line breaks.** After an item
  (in a module, a `tests { }`, `trait { }` or `impl { }` body) or a
  statement (in a block) the next one must be `}` / EOF or start on a
  later line; two on one line is an error. `;` is never a separator and
  is an error everywhere except `@for … ; key …`. Comma-separated members
  (variants, arms, fields, params, arguments, style entries) are
  separated by their commas and may share a line.
- **Postfix.** After a line break only `.` (`.field`, `.0`, `.await`)
  continues a postfix chain; `(`, `[`, a backtick, `?`, `{` and `!(` on a
  new line start a new statement.
- **Binary operators** on a new line continue the expression (leading
  `|>` style), except `-` not followed by whitespace (unary minus) and
  `<` followed by a letter or `>` (markup), which start a new statement.
- `return` and `break` take a value only when it starts on the same line
  and is not `}`.
- A **record constructor** `Path {` needs the `{` on the same line; a
  **tagged template** `` tag`…` `` and a **macro call** `name!(…)` need
  the opener adjacent to the name. Inside the head of `if` / `else if` /
  `while` / `for … in` / `match` / `provide … =` / `@if` / `@for` /
  `@match` a `{` after a path is the body, never a record constructor
  (Rust's rule, §10.5); `( )`, `[ ]`, `{ }`, `${ }` and markup holes lift
  the restriction.
- **`{` is a record or a block** (§10.4): positions that demand a block
  (`fn` / `component` / `test` / `comptime` / `loop` / `for` / `while` /
  `provide` bodies, `if` / `else` branches, lambda and match-arm bodies
  starting with `{`, `child_block`) always parse a block. Elsewhere `{`
  is a record iff, after whitespace, the next token is `}`, `..`, or a
  `lower_ident` followed by `:`, `,` or `}`; otherwise a block.
- **Trailing commas** are accepted in every comma-separated list
  (§10.8); the productions below write `[ ',' ]` everywhere they apply.

### Lexical

```ebnf
lower_ident   = lower { ident_char } ;                  (* not a reserved word; not a SQL word inside query { } *)
raw_lower     = lower { ident_char } ;                  (* keyword-insensitive: module-path segments, markup names (§10.36, §10.37) *)
dashed_name   = raw_lower { '-' raw_lower } ;           (* element, attribute and close-tag names *)
upper_ident   = upper { ident_char } ;
ident_char    = lower | upper | digit | '_' ;
tag           = ':' lower_ident ;                       (* error tag; ':' adjacent to the name *)
number        = decimal | hex | float ;                 (* JS Number semantics; value and spelling kept (§10.10) *)
bigint        = ( decimal | hex ) 'n' ;
digits        = digit { digit } ;                       (* tuple index after '.' *)
string        = '"' { string_char | escape } '"' ;      (* single line; no interpolation (§10.11) *)
template      = '`' { template_char | '${' expression '}' } '`' ;   (* multi-line; escapes add \` and \$ *)
path          = upper_ident { '::' upper_ident } ;
```

`_name` is not an identifier (`_` alone is the wildcard / placeholder).
Only `//` line comments exist; `///` and `//!` are skipped in M1 (doc
attachment is deferred to the documentation generator, §10.9).

### Module and items

```ebnf
module        = { item } ;                                (* flat, ordered, line-break separated (§10.30) *)
item          = { attribute } [ 'pub' ] item_body ;
item_body     = import | fn_decl | let_decl | type_alias | opaque_type | enum_decl
              | trait_decl | impl_decl | error_decl
              | component_decl | table_decl | schema_decl
              | macro_decl | comptime_block | test_decl | tests_block ;

attribute     = '#[' lower_ident [ '(' [ expression { ',' expression } [ ',' ] ] ')' ] ']' ;

import        = 'import' module_path [ 'as' lower_ident | '.' import_names ] ;
import_names  = '{' import_name { ',' import_name } [ ',' ] '}' | '*' ;
import_name   = ( lower_ident | upper_ident ) [ 'as' ( lower_ident | upper_ident ) ] ;
module_path   = '@' raw_lower '/' raw_lower { '/' raw_lower }         (* package *)
              | '~' { '/' raw_lower } ;                               (* this package *)

fn_decl       = 'fn' lower_ident '(' [ params ] ')' [ '->' type ] [ where_clause ] [ block ] ;
type_params   = '[' lower_ident { ',' lower_ident } [ ',' ] ']' ;     (* only on definitions with arity *)
where_clause  = 'where' [ constraint { ',' constraint } [ ',' ] ] ;
constraint    = lower_ident ':' bound { '+' bound } | lower_ident '.' upper_ident '==' type ;
bound         = path ;
params        = param { ',' param } [ ',' ] ;
param         = [ 'mut' ] pattern [ ':' type ] ;

let_decl      = 'let' [ 'mut' ] pattern [ ':' type ] '=' expression ;

type_alias    = 'type' upper_ident [ type_params ] '=' type ;
opaque_type   = 'type' upper_ident ;                                  (* requires #[extern] (§10.26) *)
enum_decl     = 'enum' upper_ident [ type_params ] '{' [ variant { ',' variant } [ ',' ] ] '}' ;
variant       = upper_ident [ '(' type { ',' type } [ ',' ] ')' | variant_record ] ;   (* '(' / '{' on the name's line *)
variant_record = '{' field_type { ',' field_type } [ ',' ] '}' ;      (* no row extension (§10.39) *)

trait_decl    = 'trait' upper_ident type_params [ where_clause ] '{' { trait_item } '}' ;
trait_item    = 'type' upper_ident | fn_decl ;                        (* line-break separated; default bodies allowed *)
impl_decl     = 'impl' path '[' type { ',' type } [ ',' ] ']' [ where_clause ] '{' { impl_item } '}' ;
impl_item     = 'type' upper_ident '=' type | fn_decl ;              (* line-break separated *)

error_decl    = 'error' upper_ident '{' [ tag_variant { ',' tag_variant } [ ',' ] ] '}' ;
tag_variant   = tag [ '(' type { ',' type } [ ',' ] ')' ] ;           (* '(' on the tag's line *)

component_decl = 'component' ( upper_ident | lower_ident ) '(' [ params ] ')' block ;   (* §10.16 *)
table_decl    = 'table' lower_ident '{' { column } '}' ;
column        = lower_ident ':' expression { modifier } ;             (* next column starts at `name :` (§10.28) *)   (* ? *)
modifier      = lower_ident [ '(' [ expression { ',' expression } [ ',' ] ] ')' ] ;
schema_decl   = 'schema' upper_ident [ 'from' lower_ident ] '{' { schema_item } '}' ;   (* ? *)
schema_item   = 'pick' lower_ident { ',' lower_ident } [ ',' ]
              | lower_ident ':' ( rule { ',' rule } | type [ ',' rule { ',' rule } ] ) [ ',' ] ;
rule          = modifier ;                                            (* a lowercase word after ':' is a rule, not a type variable (§10.28) *)

macro_decl    = 'macro' lower_ident '(' [ lower_ident { ',' lower_ident } [ ',' ] ] ')' '{' raw_tokens '}' ;   (* body is balanced raw text (§10.29) *)
comptime_block = 'comptime' block ;
test_decl     = 'test' string block ;
tests_block   = 'tests' '{' { item } '}' ;                            (* line-break separated *)
```

- A bare `import module_path` binds its last segment, which must be
  present and not a reserved word (`import ~` and `import @alder/test`
  are errors; `import @alder/test.{ fakeDb }` and `as` are fine, §10.37).
- `pub import` requires `.{ … }` or `.*` (§10.25); it is the re-export
  form.
- A bodiless `fn_decl` is an extern function (with `#[extern("module",
"name")]`) or a trait signature; `opaque_type` requires `#[extern]`.
  The parser accepts both anywhere and canonicalization checks the
  attribute (§10.26). A trait `type Item` takes no `= type`.
- `style` is an expression (`let card = style { … }`), not an item form.

### Statements and blocks

```ebnf
block         = '{' { statement } [ expression ] '}' ;   (* statements line-break separated; a trailing expression is the value *)
statement     = let_decl
              | 'use' path
              | assign
              | 'for' pattern 'in' expression block
              | 'while' expression block
              | 'return' [ expression ]                  (* value on the same line *)
              | 'break' [ expression ]                   (* value on the same line *)
              | 'continue'
              | 'assert' expression                      (* §10.6 *)
              | expression ;
assign        = place ( '=' | '+=' | '-=' | '*=' | '/=' ) expression ;   (* operator on the target's line *)
place         = lower_ident { '.' lower_ident | '.' digits | '[' expression ']' } ;
```

### Expressions

```ebnf
expression    = unary { operator unary } ;               (* flat chain; nesting resolved in canonicalization (§10.1) *)
operator      = '|>' | '??' | '||' | '&&' | '==' | '!=' | '<' | '<=' | '>' | '>=' | 'in'
              | '+' | '-' | '*' | '/' | '%' ;            (* 'in' only inside query { } *)
unary         = [ '-' | '!' ] postfix ;
postfix       = primary { call | '.' lower_ident | '.' digits | '.await' | '?'
                        | '[' expression ']' | template | record } ;
                                                         (* template adjacent (tagged, §10.12); record only after a path, '{' on the same line *)
call          = '(' [ call_arg { ',' call_arg } [ ',' ] ] ')' ;
call_arg      = '_' | expression ;                       (* '_' placeholder only as a whole argument (§10.18) *)
primary       = number | bigint | string | template | 'true' | 'false'
              | lower_ident | path [ '::' lower_ident ] | tag [ call ]
              | '(' ')' | '(' expression [ ',' ] ')' | '(' expression ',' expression { ',' expression } [ ',' ] ')'   (* '(' e ',' ')' is e (§10.8) *)
              | '[' [ expression { ',' expression } [ ',' ] ] ']'
              | record | block | lambda | if_expr | match_expr | loop_expr
              | provide_expr
              | 'state' '(' expression ')'
              | 'style' style_block
              | 'query' '{' query_expr '}' | markup
              | macro_call ;
lambda        = 'fn' '(' [ params ] ')' [ '->' type ] ( block | assign | expression ) ;   (* §10.13 *)
if_expr       = 'if' expression block { 'else' 'if' expression block } [ 'else' block ] ;
match_expr    = 'match' expression '{' { match_arm } '}' ;
match_arm     = pattern { '|' pattern } [ 'if' expression ] '=>' ( block | expression ) [ ',' ] ;
                                                         (* a '{' after '=>' is always a block; arms separated by ',' or a line break *)
loop_expr     = 'loop' block ;
provide_expr  = 'provide' path '=' expression block ;
record        = '{' [ record_field { ',' record_field } [ ',' ] ] '}' ;
record_field  = lower_ident [ ':' expression ] | '..' expression ;
macro_call    = lower_ident '!' '(' raw_tokens ')' ;     (* '!(' adjacent; balanced raw text (§10.29) *)
style_block   = '{' { style_key ':' style_value [ ',' ] } '}' ;   (* entries may also be line-break separated *)
style_key     = lower_ident | string ;
style_value   = style_block | dimension | expression ;   (* '{' is always a nested style (§10.27) *)
dimension     = [ '-' ] number ( letter { letter } | '%' ) ;   (* unit adjacent to the number *)
```

Operator precedence, resolved in canonicalization (§10.1):

| level | operators                        | associativity |
| ----- | -------------------------------- | ------------- |
| 0     | `\|>`                            | left          |
| 1     | `??`                             | right         |
| 2     | `\|\|`                           | left          |
| 3     | `&&`                             | left          |
| 4     | `==` `!=` `<` `<=` `>` `>=` `in` | none          |
| 6     | `+` `-`                          | left          |
| 7     | `*` `/` `%`                      | left          |

Operators are matched longest-first from this fixed table (§10.2), so
`a==-1` is `a == -1` and `x<-1` is `x < -1`. `=`, `=>`, `+=`, `-=`, `*=`
and `/=` end an expression and are never operators. The Elm-habit tokens
`->`, `|`, `++`, `::`, `..`, `<|`, `>>`, `<<` and `^` (outside patterns
and queries) are recognized only to report an error with a hint. `?`
applies whenever the next byte is not `?`, so `x? ?? y` works (§10.19).
`^` outside `query { }` and patterns is an error (§10.20).

### Markup

```ebnf
markup        = element | fragment ;                     (* primary position: '<' followed by a letter or '>' *)
element       = '<' element_name { attr } ( '/>' | '>' { child } '</' element_name '>' ) ;
element_name  = dashed_name | path ;                     (* html / custom element | component; not subject to reserved words (§10.36) *)
fragment      = '<>' { child } '</>' ;
attr          = attr_name [ '=' ( string | '{' expression '}' ) ] ;
attr_name     = dashed_name ;                            (* not subject to reserved words (§10.36) *)
child         = element | fragment | text
              | '{' expression '}'
              | '@if' expression child_block { '@else' 'if' expression child_block } [ '@else' child_block ]
              | '@for' pattern 'in' expression [ ';' 'key' expression ] child_block [ '@empty' child_block ]
              | '@match' expression '{' { pattern { '|' pattern } [ 'if' expression ] '=>' match_child [ ',' ] } '}' ;
match_child   = element | fragment | child_block | '@if' … | '@for' … | '@match' … ;   (* no bare text (§10.24) *)
child_block   = '{' { let_decl | 'use' path | child } '}' ;   (* §10.23 *)
text          = (* any run of characters not containing '<', '{' or '}'; a '@' ends it only before if/for/match/else/empty followed by a non-identifier byte *) ;
```

Inside a `child_block`, `let` / `let mut` / `use` are setup and do not
render; markup and `{expr}` holes become children; any other statement
form is written as `{expr}`. Whitespace-only text runs containing a
newline are dropped; all other text is kept verbatim (§10.22). `@else`
and `@empty` may start on the line after the previous block.

### Queries

```ebnf
query_expr    = select_expr | insert_expr | update_expr | delete_expr ;
query_value   = '^' postfix ;                          (* pinned host value; the operand is parsed outside query mode (§10.20) *)
select_expr   = 'select' ( '{' expression { ',' expression } [ ',' ] '}' | '*' )
                'from' table_ref { join } [ 'where' expression ]
                [ 'groupBy' expression { ',' expression } ] [ 'orderBy' order { ',' order } ]
                [ 'limit' expression ] [ 'offset' expression ] ;
table_ref     = lower_ident [ 'as' lower_ident ] ;
join          = [ 'left' | 'inner' ] 'join' table_ref 'on' expression ;
order         = expression [ 'asc' | 'desc' ] ;
insert_expr   = 'insert' 'into' lower_ident 'values' query_value ;
update_expr   = 'update' lower_ident 'set' record [ 'where' expression ] ;
delete_expr   = 'delete' 'from' lower_ident [ 'where' expression ] ;
```

Inside `query { }` the SQL words are keywords, `in` is an operator, and
`query_value` may appear wherever an expression may (`^user.id` pins
`user.id`; `^(a + b)` pins the sum; `^{ email, name }` pins a record).
Clauses must appear in the order written above and, except `join`, at
most once (§10.21).

### Patterns

```ebnf
pattern       = pattern_atom [ 'as' lower_ident ] ;
pattern_atom  = '_' | lower_ident | '^' postfix
              | [ '-' ] number | [ '-' ] bigint | string | 'true' | 'false'   (* §10.7, §10.17 *)
              | path [ '(' pattern { ',' pattern } [ ',' ] ')' | pattern_record ]   (* '(' / '{' on the path's line *)
              | tag [ '(' pattern { ',' pattern } [ ',' ] ')' ]
              | '(' ')' | '(' pattern [ ',' ] ')' | '(' pattern ',' pattern { ',' pattern } [ ',' ] ')'   (* unit pattern for Ok(()) (§10.17); '(' p ',' ')' is p *)
              | '[' { pattern ',' } [ pattern | '..' [ lower_ident ] [ ',' ] ] ']'   (* a comma before '..' is required *)
              | pattern_record ;
pattern_record = '{' { field_pattern ',' } [ field_pattern | '..' [ ',' ] ] '}' ;   (* a comma before '..' is required *)
field_pattern = lower_ident [ ':' pattern ] ;
```

### Types

```ebnf
type          = fn_type | type_app ;
fn_type       = 'fn' '(' [ type { ',' type } [ ',' ] ] ')' '->' type ;
type_app      = path [ type_args ]
              | lower_ident [ type_args ]                              (* type variable; applied for HKT (§10.14) *)
              | '_'                                                    (* only a direct named-constructor argument in an impl head, e.g. Result[_, e] *)
              | '(' ')' | '(' type [ ',' ] ')' | '(' type ',' type { ',' type } [ ',' ] ')'   (* '(' T ',' ')' is T *)
              | record_type
              | error_row ;
type_args     = '[' type { ',' type } [ ',' ] ']' ;                    (* '[' on the name's line *)
record_type   = '{' [ lower_ident '|' ] [ field_type { ',' field_type } [ ',' ] ] '}' ;
field_type    = lower_ident [ '?' ] ':' type ;                         (* '?' adjacent to the name *)
error_row     = '[' [ tag_variant { '|' tag_variant } [ '|' lower_ident ] | lower_ident ] ']' ;   (* '[r]' is open and empty; '[| r]' is an error *)
```

### Reserved words

```
as assert await break comptime component continue else enum error false
fn for if impl import in let loop macro match mut pub provide query
return schema state style table test tests trait true type use where
while
```

`assert` is a statement and `await` keeps `.await` from colliding with a
field (§10.6). All SQL words (`select`, `insert`, `update`, `delete`,
`from`, `join`, `on`, `set`, `into`, `values`, `orderBy`, `groupBy`,
`limit`, `offset`, `asc`, `desc`, `left`, `inner`) are contextual
keywords inside a `query { }` block only; `where` is reserved
everywhere. Module-path segments, element names and attribute names are
never subject to either list (§10.36, §10.37).
