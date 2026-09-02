# Alder Language Specification

**Status: current direction, everything provisional.** The language was
redesigned on 2026-09-01. The pipeline below is a working Elm port; the
surface syntax it parses today is Elm's and will be replaced by the
grammar at the end of this document. Design rationale lives in `docs/`:

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
│   ├── alder-parse/            # Parser (to be rewritten for the new grammar)
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
- **Field naming:** camelCase (`sourceDirectories`, `exposedModules`, `testDependencies`)
- **Lock file:** Single shared `alder.lock` at workspace root (TBD in driver)
- **Dependencies:** Runtime-agnostic (no mandatory core deps), `author/project` naming
- **Dependency syntax:** Constraint string or object (`{ "workspace": true }`, `{ "path": "..." }`, `{ "git": "..." }`)
- **Source discovery:** Convention-based (`src/` default)
- **Error messages:** Line/column accurate via AST-based parsing

**Config Types:**

```jsonc
// Application - compiles to a JavaScript app
{
    "type": "application",
    "sourceDirectories": ["src"],
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
    "exposedModules": ["Module.Name"],  // or { "Category": ["Module"] }
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
as they land and update the grammar section alongside.

### M1: Parser rewrite (`alder-parse`)

New parser for the grammar below, keeping `alder-ast`, `alder-can`, and
the solver. Snapshot tests per construct as today.

- [ ] Lexer: `//` comments, template literals, `:tag` tokens, `#[`, `::`, `=>`, `->`, `|>`, `??`, `?`
- [ ] Items: `pub`, path-first `import` with `.{ }`/`.*`/`as`, re-exports (`pub import`)
- [ ] `fn` declarations and lambdas, optional `-> Type` after params
- [ ] Statements: `let`/`let mut`, assignment and compound assignment, `for`, `while`, `loop`, `break`/`continue` with values, `return`
- [ ] Expressions: blocks, `if`/`else if`, `match` with `=>` and guards, `|>`, `.await`, `?`, `??`, calls, `_` placeholders, field/tuple access, paths (`Option::Some`)
- [ ] Literals: numbers (JS semantics), template literals, arrays, tuples, records with spread and optional fields
- [ ] Types: `Name[a, b]`, `fn(A) -> B`, tuples, records with `?` fields and rows, error rows `[:tag(A) | r]`, `Result[a]` shorthand, `where` clauses
- [ ] `type` aliases, `enum` with tuple and record variants
- [ ] `trait` and `impl` (Haskell-style, HKT params, associated types, default bodies)
- [ ] `error` groups
- [ ] `#[attr]` attributes on items
- [ ] `tests { }` blocks and `test "name" { }`
- [ ] `#[extern(...)]` bodiless functions and `#[extern] type`
- [ ] Typed markup expressions with `{expr}`, `{if}`, `{for}`, `{match}` blocks
- [ ] `component`, `table`, `schema`, `style`, `query`, `macro`, `comptime` (grammar reserved; bodies may be parsed later)
- [ ] Port/rewrite `Reporting/Error/Syntax.hs`-style error hierarchy for the new constructs

### M2: Core language to JavaScript

- [ ] Adapt `alder-can` to namespaced constructors, `pub` visibility, statements, `mut`
- [ ] `alder-codegen`: JS emission for the core language; decide enum/record representation
- [ ] Prelude and stdlib skeleton: `Option`, `Result`, `Array`, `String`, `Number`, `BigInt`, `Map`
- [ ] JS kernel skeleton and `extern` binding
- [ ] Embed `deno_core` plus the web-standard extension crates in `alder-cli`; `alder run` for `server` and `cli` targets
- [ ] `Cli` module with `Args`/`Subcommand` derives
- [ ] rolldown integration for `alder build`
- [ ] `alder fmt`

### M3: Traits

- [ ] Type-class constraints in `alder-constrain`/`alder-solve` (single param)
- [ ] Higher-kinded type parameters
- [ ] Dictionary-passing codegen with static resolution where possible
- [ ] Derives via macros (`Show`, `Eq`, `Json`)
- [ ] Orphan rule checking in `alder-can`

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
- [ ] SQL-shaped query expressions, type-checked projections, desugar to chain API
- [ ] `alder db generate`/`migrate`/`push` with diff-generated SQL
- [ ] D1 and Hyperdrive drivers; embedded SQLite for `server`/`tui`
- [ ] `schema` declarations with `from table` and validation rules

### M8: Styles, forms, API

- [ ] `style` blocks to atomic CSS with typed properties
- [ ] `Form`/`Field` components typed from `schema`
- [ ] Typed client generation from `+server.ald`; `.d.ts` emission
- [ ] Router builder for API-only packages

### M9: Tooling and ecosystem

- [ ] `test`/`tests` with power-assert, per-target runners
- [ ] Package registry, `alder publish`, semver enforcement by API diff
- [ ] Language server features on the new grammar; WASM playground
- [ ] Documentation generator

### M10: TUI

- [ ] TUI element vocabulary and Rust-side layout/input in deno_core
- [ ] Terminal renderer over the shared signal graph

---

## Grammar (draft)

EBNF for the new syntax. Provisional; `?`-marked productions are open
questions. Whitespace and `//` comments are insignificant except inside
template literals and markup text.

### Lexical

```ebnf
lower_ident   = lower { ident_char } ;
upper_ident   = upper { ident_char } ;
ident_char    = lower | upper | digit | '_' ;
tag           = ':' lower_ident ;                       (* error tag *)
number        = decimal | hex | float ;                 (* JS Number semantics *)
bigint        = decimal 'n' ;
string        = '"' { string_char | escape } '"' ;      (* no interpolation *)
template      = '`' { template_char | '${' expression '}' } '`' ;
path          = upper_ident { '::' upper_ident } ;
```

### Module and items

```ebnf
module        = { item } ;
item          = { attribute } [ 'pub' ] item_body ;
item_body     = import | reexport | fn_decl | let_decl | type_alias | enum_decl
              | trait_decl | impl_decl | error_decl | extern_fn | extern_type
              | component_decl | table_decl | schema_decl | style_decl
              | macro_decl | comptime_block | test_decl | tests_block ;

attribute     = '#[' lower_ident [ '(' [ expression { ',' expression } ] ')' ] ']' ;

import        = 'import' module_path [ 'as' lower_ident | '.' import_names ] ;
import_names  = '{' import_name { ',' import_name } [ ',' ] '}' | '*' ;
import_name   = ( lower_ident | upper_ident ) [ 'as' ( lower_ident | upper_ident ) ] ;
reexport      = 'import' module_path '.' import_names ;                          (* after 'pub' *)
module_path   = '@' lower_ident '/' lower_ident { '/' lower_ident }             (* package *)
              | '~' { '/' lower_ident } ;                                       (* this package *)

fn_decl       = 'fn' lower_ident '(' [ params ] ')' [ '->' type ] [ where_clause ] block ;
type_params   = '[' lower_ident { ',' lower_ident } ']' ;           (* only on definitions with arity *)
where_clause  = 'where' constraint { ',' constraint } [ ',' ] ;
constraint    = lower_ident ':' bound { '+' bound } | lower_ident '.' upper_ident '==' type ;
bound         = path ;
params        = param { ',' param } ;
param         = [ 'mut' ] pattern [ ':' type ] ;

let_decl      = 'let' [ 'mut' ] pattern [ ':' type ] '=' expression ;

type_alias    = 'type' upper_ident [ type_params ] '=' type ;
enum_decl     = 'enum' upper_ident [ type_params ] '{' [ variant { ',' variant } [ ',' ] ] '}' ;
variant       = upper_ident [ '(' type { ',' type } ')' | record_type ] ;

trait_decl    = 'trait' upper_ident type_params [ where_clause ] '{' { trait_item } '}' ;
trait_item    = 'type' upper_ident
              | 'fn' lower_ident '(' [ params ] ')' [ '->' type ] [ where_clause ] [ block ] ;
impl_decl     = 'impl' path '[' type { ',' type } ']' [ where_clause ] '{' { impl_item } '}' ;
impl_item     = 'type' upper_ident '=' type | fn_decl ;

error_decl    = 'error' upper_ident '{' [ tag_variant { ',' tag_variant } [ ',' ] ] '}' ;
tag_variant   = tag [ '(' type { ',' type } ')' ] ;

extern_fn     = 'fn' lower_ident '(' [ params ] ')' '->' type [ where_clause ] ;   (* requires #[extern("module", "name")] *)
extern_type   = 'type' upper_ident ;                                           (* requires #[extern] *)

component_decl = 'component' upper_ident '(' [ params ] ')' block ;
table_decl    = 'table' lower_ident '{' { column } '}' ;
column        = lower_ident ':' expression { lower_ident [ '(' [ expression { ',' expression } ] ')' ] } ;   (* ? *)
schema_decl   = 'schema' upper_ident [ 'from' lower_ident ] '{' { schema_item } '}' ;   (* ? *)
schema_item   = 'pick' lower_ident { ',' lower_ident }
              | lower_ident ':' [ type ',' ] rule { ',' rule } ;
rule          = lower_ident [ '(' [ expression { ',' expression } ] ')' ] ;
style_decl    = 'let' lower_ident '=' 'style' style_block ;                 (* style is an expression *)

macro_decl    = 'macro' lower_ident '(' [ lower_ident { ',' lower_ident } ] ')' block ;
comptime_block = 'comptime' block ;
test_decl     = 'test' string block ;
tests_block   = 'tests' '{' { item } '}' ;
```

### Statements and blocks

```ebnf
block         = '{' { statement } [ expression ] '}' ;
statement     = let_decl
              | 'use' path
              | 'provide' path '=' expression block
              | assign
              | 'for' pattern 'in' expression block
              | 'while' expression block
              | 'return' [ expression ]
              | 'break' [ expression ]
              | 'continue'
              | expression ;
assign        = place ( '=' | '+=' | '-=' | '*=' | '/=' ) expression ;
place         = lower_ident { '.' ( lower_ident | digit ) | '[' expression ']' } ;
```

### Expressions

```ebnf
expression    = pipe ;
pipe          = binop { '|>' binop } ;
binop         = unary { operator unary } ;                (* precedence table TBD; '??' lowest *)
unary         = [ '-' | '!' ] postfix ;
postfix       = primary { call | '.' lower_ident | '.' digit | '.await' | '?' | '[' expression ']' } ;
call          = '(' [ expression { ',' expression } ] ')' ;
primary       = number | bigint | string | template | '_'            (* placeholder in call args *)
              | lower_ident | path [ '::' lower_ident ] | tag [ call ]
              | '(' ')' | '(' expression ')' | '(' expression ',' expression { ',' expression } ')'
              | '[' [ expression { ',' expression } ] ']'
              | record | block | lambda | if_expr | match_expr | loop_expr
              | 'state' '(' expression ')'
              | 'style' style_block
              | 'query' '{' query_expr '}' | markup
              | macro_call ;
lambda        = 'fn' '(' [ params ] ')' [ '->' type ] ( block | expression ) ;
if_expr       = 'if' expression block { 'else' 'if' expression block } [ 'else' block ] ;
match_expr    = 'match' expression '{' { match_arm } '}' ;
match_arm     = pattern { '|' pattern } [ 'if' expression ] '=>' ( block | expression ) [ ',' ] ;
loop_expr     = 'loop' block ;
record        = '{' [ record_field { ',' record_field } [ ',' ] ] '}' ;
record_field  = lower_ident [ ':' expression ] | '..' expression ;
macro_call    = lower_ident '!' '(' { token } ')' ;
style_block   = '{' { ( lower_ident | string ) ':' ( expression | style_block ) [ ',' ] } '}' ;
```

### Markup

```ebnf
markup        = element | fragment ;
element       = '<' element_name { attr } ( '/>' | '>' { child } '</' element_name '>' ) ;
element_name  = lower_ident | path ;                     (* html element | component *)
fragment      = '<>' { child } '</>' ;
attr          = attr_name [ '=' ( string | '{' expression '}' ) ] ;
attr_name     = lower_ident { '-' lower_ident } ;
child         = element | fragment | text
              | '{' expression '}'
              | '{' 'if' expression block { 'else' 'if' expression block } [ 'else' block ] '}'
              | '{' 'for' pattern 'in' expression block '}'
              | '{' 'match' expression '{' { match_arm } '}' '}' ;
text          = (* any run of characters not containing '<', '{', or '}' *) ;
```

Inside markup blocks, `block` bodies produce children rather than values.

### Queries

```ebnf
query_expr    = select_expr | insert_expr | update_expr | delete_expr ;
select_expr   = 'select' ( '{' expression { ',' expression } '}' | '*' )
                'from' table_ref { join } [ 'where' expression ]
                [ 'groupBy' expression { ',' expression } ] [ 'orderBy' order { ',' order } ]
                [ 'limit' expression ] [ 'offset' expression ] ;
table_ref     = lower_ident [ 'as' lower_ident ] ;
join          = [ 'left' | 'inner' ] 'join' table_ref 'on' expression ;
order         = expression [ 'asc' | 'desc' ] ;
insert_expr   = 'insert' 'into' lower_ident 'values' ( record | expression ) ;
update_expr   = 'update' lower_ident 'set' record [ 'where' expression ] ;
delete_expr   = 'delete' 'from' lower_ident [ 'where' expression ] ;
```

### Patterns

```ebnf
pattern       = pattern_atom [ 'as' lower_ident ] ;
pattern_atom  = '_' | lower_ident | number | bigint | string
              | path [ '(' pattern { ',' pattern } ')' | pattern_record ]
              | tag [ '(' pattern { ',' pattern } ')' ]
              | '(' pattern { ',' pattern } ')'
              | '[' [ pattern { ',' pattern } [ ',' '..' [ lower_ident ] ] ] ']'
              | pattern_record ;
pattern_record = '{' [ lower_ident [ ':' pattern ] { ',' lower_ident [ ':' pattern ] } [ ',' '..' ] ] '}' ;
```

### Types

```ebnf
type          = fn_type | type_app ;
fn_type       = 'fn' '(' [ type { ',' type } ] ')' '->' type ;
type_app      = path [ '[' type { ',' type } ']' ]
              | lower_ident                                            (* type variable *)
              | '(' ')' | '(' type ',' type { ',' type } ')'
              | record_type
              | error_row ;
record_type   = '{' [ lower_ident '|' ] [ field_type { ',' field_type } [ ',' ] ] '}' ;
field_type    = lower_ident [ '?' ] ':' type ;
error_row     = '[' [ tag_variant { '|' tag_variant } ] [ '|' lower_ident ] ']' ;
```

### Reserved words

```
as break comptime component continue else enum error false fn for if
impl import in let loop macro match mut pub provide query return schema
state style table test tests trait true type use where while
```

All SQL words (`select`, `insert`, `update`, `delete`, `from`, `where`,
`join`, `on`, `set`, `into`, `values`, `orderBy`, `groupBy`, `limit`,
`offset`, `asc`, `desc`, `left`, `inner`) are contextual keywords inside a
`query { }` block only.
