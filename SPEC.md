# Alder Language Specification

## Current State

**Parser complete!** Next phase: project configuration and driver infrastructure.

---

## Compilation Target

Alder compiles to **JavaScript**, with a special focus on targeting **Cloudflare** (Workers and the surrounding platform).

**Key concepts:**
- **Fork of Elm:** The compiler is a Rust port of the Elm compiler, but the language diverges deliberately.
- **No TEA:** The runtime model is React-like rather than The Elm Architecture.
- **SSR:** Server-side rendering is a first-class target.
- **Built-in data layer:** A Drizzle-like query layer ships with the language.
- **Curly-brace syntax:** Surface syntax uses braces instead of Elm's layout-sensitive syntax.

Source files use the `.ald` extension.

---

## Next Milestones

### Phase 1: Project Configuration (`alder-config`) ✅

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

### Phase 2: Driver & Build System (`alder-driver`) ✅

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

### Phase 3: Canonicalization (`alder-can`)

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

### Phase 4: Type Inference (`alder-constrain` + `alder-solve`)

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

### Phase 5: JavaScript Code Generation

**Goal:** Compile typed AST to JavaScript suitable for Cloudflare Workers, including SSR output.

**Crate:** TBD (alder-codegen? alder-js?)

---

### Phase 6: Runtime

**Goal:** React-like runtime (no TEA), SSR support, and the built-in Drizzle-like data layer.

---

### Phase 7: CLI (`alder-cli`)

**Goal:** Full developer toolkit CLI — a single `alder` binary.

**Commands:**
- `alder check` - Type-check without generating JavaScript
- `alder build` - Compile to JavaScript
- `alder test` - Run tests
- `alder lsp` - Start language server
- `alder init` - Initialize new project
- `alder repl` - True stateful REPL
- `alder fmt` - Format source files
- `alder docs` - Generate documentation

**Compiler version management:**
- Projects specify `"compiler": "X.Y.Z"` in `alder.jsonc`
- If the running `alder` binary matches, it compiles directly
- If not, checks `~/.alder/versions/<version>/alder` for a cached copy and exec's it

**Architecture:**
- Uses clap for argument parsing
- Entry point picks tokio runtime
- Thin wrapper around driver/compiler

**Reference:** `polarity/app/src/cli/`

---

### Phase 8: Language Server (`alder-language-server`)

**Goal:** LSP implementation that works native and in WASM.

**Architecture Decisions:**
- **Library:** tower-lsp with `runtime-agnostic` feature
- **Features:** Diagnostics, hover, goto-definition, formatting, code-actions
- **Invalidation:** Reverse dependency tracking
- **Unsaved buffers:** InMemorySource overlays FileSystemSource

**Reference:** `polarity/lang/lsp/`

---

### Phase 9: Playground (`web/`)

**Goal:** Browser-based Alder playground with LSP support.

**Architecture Decisions:**
- **Editor:** Monaco + wasm-bindgen
- **File loading:** HTTP fetch from server
- **LSP transport:** JSON-RPC over streams

**Structure:**
- `web/crates/lsp-wasm/` - WASM LSP entry point
- `web/packages/web-editor/` - Monaco editor UI

**Reference:** `polarity/web/`

---

### Phase 10: Package Registry & Dependency Resolution

**Goal:** Publish packages, resolve dependencies.

**Architecture Decisions:**
- **Solver:** pubgrub crate
- **Package naming:** author/project format
- **Registry:** HTTP API (design TBD)

---

## Crate Organization

```
orbistry/alder/
├── crates/
│   ├── alder-region/           # Source spans/positions
│   ├── alder-source/           # Parsed AST types
│   ├── alder-parse/            # Parser
│   ├── alder-ast/              # Canonical/typed AST types
│   ├── alder-can/              # Canonicalization
│   ├── alder-constrain/        # Type constraint generation
│   ├── alder-solve/            # Constraint solving (type inference)
│   ├── alder-config/           # Project configuration (JSONC)
│   ├── alder-driver/           # Build orchestration, FileSource
│   ├── alder-codegen/          # JavaScript code generation (TBD)
│   ├── alder-language-server/  # LSP implementation
│   └── alder-cli/              # CLI binary (`alder`)
├── web/                       # Playground (TBD)
│   ├── crates/
│   │   └── lsp-wasm/
│   └── packages/
│       └── web-editor/
├── .alder/                     # Build artifacts (gitignored)
└── alder.json                  # Project config
```

---

## Error Reporting

**Library:** miette

**Features:**
- Rich terminal diagnostics with source snippets
- JSON output via miette's serialization
- Suggestion system (port Elm's Levenshtein-based Suggest.hs)

**Reference:** `elm/compiler/src/Reporting/`, `polarity` (uses miette)

---

## Current Working Files (Parser - Complete)

Working files:
- `crates/alder-parse/src/lib.rs` - Parser struct, combinators (`one_of`, `in_context`, `specialize`, `word1`, `word2`)
- `crates/alder-parse/src/number.rs` - `number_literal` primitive
- `crates/alder-parse/src/string.rs` - `string_literal` primitive
- `crates/alder-parse/src/keyword.rs` - reserved words, keyword parsers (`keyword_if`, `keyword_then`, etc.)
- `crates/alder-parse/src/space.rs` - whitespace, comments, indentation
- `crates/alder-parse/src/expression/mod.rs` - `expression`, `term`, `possibly_negative_term`, `chomp_expr_end`
- `crates/alder-parse/src/expression/accessor.rs` - `.field` accessor, `foo.bar` field access chains
- `crates/alder-parse/src/expression/lambda.rs` - `\args -> body` lambda expressions
- `crates/alder-parse/src/expression/if_.rs` - `if/then/else` expressions
- `crates/alder-parse/src/expression/case.rs` - `case/of` expressions
- `crates/alder-parse/src/expression/let_.rs` - `let/in` expressions
- `crates/alder-parse/src/expression/number.rs` - `number` expression + tests
- `crates/alder-parse/src/expression/string.rs` - `string` expression + tests
- `crates/alder-parse/src/expression/variable.rs` - `variable`, `lower_name`, `upper_name`, `foreign_alpha` + tests
- `crates/alder-parse/src/expression/list.rs` - `list` expression + tests
- `crates/alder-parse/src/expression/tuple.rs` - `tuple`, unit, parens + tests
- `crates/alder-parse/src/expression/record.rs` - `record`, record update + tests
- `crates/alder-parse/src/pattern/mod.rs` - `pattern_term`, `pattern_expr` (cons, as, ctor args)
- `crates/alder-parse/src/pattern/term.rs` - wildcard, var, ctor, number, string
- `crates/alder-parse/src/pattern/record.rs` - `{ x, y, z }`
- `crates/alder-parse/src/pattern/tuple.rs` - `()`, `(a, b)`
- `crates/alder-parse/src/pattern/list.rs` - `[]`, `[a, b, c]`
- `crates/alder-parse/src/type_.rs` - `type_term`, `type_expr` (variables, named, function, tuple, record)
- `crates/alder-parse/src/declaration/mod.rs` - `declaration`, `Decl` enum, orchestration
- `crates/alder-parse/src/declaration/value.rs` - value definitions with type annotations
- `crates/alder-parse/src/declaration/type_alias.rs` - `type alias Name a = Type`
- `crates/alder-parse/src/declaration/union.rs` - `type Name a = Ctor1 | Ctor2`
- `crates/alder-parse/src/declaration/infix.rs` - `infix left 6 (|>) = apR`
- `crates/alder-parse/src/exposing.rs` - `(..)`, `(foo, Bar(..), (+))`
- `crates/alder-parse/src/import.rs` - `import Foo as F exposing (bar)`
- `crates/alder-parse/src/module.rs` - full module parsing (header, imports, declarations)
- `crates/alder-parse/src/error.rs` - Error hierarchy

## File Mappings

Elm parser modules → Alder parser modules:

| Elm (`elm/compiler/src/Parse/`) | Alder (`crates/alder-parse/src/`) |
|---------------------------------|---------------------------------|
| `Primitives.hs`                 | `lib.rs` (Parser struct)        |
| `Module.hs`                     | `module.rs`                     |
| `Declaration.hs`                | `declaration.rs`                |
| `Expression.hs`                 | `expression/`                   |
| `Pattern.hs`                    | `pattern/`                      |
| `Type.hs`                       | `type_.rs`                      |
| `Number.hs`                     | `number.rs`                     |
| `String.hs`                     | `string.rs`                     |
| `Variable.hs`                   | `expression/variable.rs`        |
| `Symbol.hs`                     | `symbol.rs`                     |
| `Keyword.hs`                    | `keyword.rs`                    |
| `Space.hs`                      | `space.rs`                      |
| `Reporting/Error/Syntax.hs`     | `error.rs`                      |

AST types: `crates/alder-source/src/lib.rs`

## Implementation Progress

### Parser Infrastructure
- [x] Parser struct with state tracking (`Parser<'a>` in lib.rs)
- [x] Basic methods (peek, advance, position)
- [x] Snapshot test infrastructure (insta macros)
- [x] Error type hierarchy (from Elm's Syntax.hs) - `error.rs`
- [x] `one_of` / `one_of_with_fallback` combinators
- [x] `in_context` / `specialize` for error wrapping
- [x] `word1` / `word2` for byte matching
- [x] Whitespace and comment handling (`space.rs`)
- [x] Line comments (`--`)
- [x] Multi-line comments (`{- -}`) with nesting
- [x] Indentation checking

### Literals
- [x] Integer literals (`number.rs`)
- [x] String literals (single-line) (`string.rs`)
- [x] String literals (multi-line) (`string.rs`)

### Identifiers
- [x] Lowercase variables (`expression/variable.rs`)
- [x] Uppercase variables (constructors)
- [x] Qualified names (Module.name)
- [x] Operators (`symbol.rs`)

### Basic Expressions
- [x] Unit `()` (`expression/tuple.rs`)
- [x] Tuples (`expression/tuple.rs`)
- [x] Lists (`expression/list.rs`)
- [x] Records (`expression/record.rs`)
- [x] Record update (`expression/record.rs`)

### Patterns
- [x] Wildcard `_` (`pattern/term.rs`)
- [x] Variable binding (`pattern/term.rs`)
- [x] Constructor patterns (`pattern/term.rs`, `pattern/mod.rs`)
- [x] Tuple patterns (`pattern/tuple.rs`)
- [x] List patterns (`pattern/list.rs`)
- [x] Record patterns (`pattern/record.rs`)
- [x] As-patterns (`pattern/mod.rs`)
- [x] Cons patterns (`pattern/mod.rs`)

### Types
- [x] Type variables (`type_.rs`)
- [x] Named types (`type_.rs`)
- [x] Function types (`type_.rs`)
- [x] Tuple types (`type_.rs`)
- [x] Record types (`type_.rs`)

### Expressions
- [x] term / number (`expression/number.rs`)
- [x] term / string (`expression/string.rs`)
- [x] term / variable (`expression/variable.rs`)
- [x] Accessor `.field` (`expression/accessor.rs`)
- [x] Field access `foo.bar.baz` (`expression/accessor.rs`)
- [x] Negation `-expr` (`expression/mod.rs`)
- [x] Function application (`expression/mod.rs`)
- [x] Lambda expressions (`expression/lambda.rs`)
- [x] If expressions (`expression/if_.rs`)
- [x] Case expressions (`expression/case.rs`)
- [x] Let expressions (`expression/let_.rs`)
- [x] Binary operators (`expression/mod.rs`, `symbol.rs`)
- [x] Operator sections (`expression/tuple.rs`)

### Declarations
- [x] Value definitions (`declaration/value.rs`)
- [x] Type annotations (`declaration/value.rs`)
- [x] Type aliases (`declaration/type_alias.rs`)
- [x] Custom types (unions) (`declaration/union.rs`)
- [x] Infix declarations (`declaration/infix.rs`)

### Module Structure
- [x] Module header (`module.rs`)
- [x] Exposing list (`exposing.rs`)
- [x] Imports (`import.rs`)
- [x] Full module parsing (`module.rs`)

### Canonicalization
- [x] Module header canonicalization (`crates/alder-can/src/module.rs`)
- [x] Import environment with privacy (`crates/alder-can/src/environment/foreign.rs`)
- [x] Local env: types, ctors, vars, binops, dup detection (`crates/alder-can/src/environment/local.rs`)
- [x] Union/alias canonicalization with free-var checks and alias cycles (`crates/alder-can/src/module.rs`)
- [x] Pattern canonicalization (`crates/alder-can/src/pattern.rs`)
- [x] Type/annotation canonicalization with alias dealiasing (`crates/alder-can/src/types.rs`)
- [x] Expression canonicalization with operator desugaring (`crates/alder-can/src/expression.rs`)
- [x] Two-phase SCC cycle detection, exact `Data.Graph.stronglyConnComp` port (`crates/alder-can/src/scc.rs`)
- [x] Export canonicalization and interface extraction (`crates/alder-can/src/interface.rs`)
- [x] Foreign value/binop annotations flow from solved interfaces (`crates/alder-can/src/environment/foreign.rs`)
- [ ] Prelude / default imports (deferred to prelude design)

### Type Inference
- [x] Union-find with weight balancing and path compression (`crates/alder-constrain/src/union_find.rs`)
- [x] `Type.Type` data half: descriptors, inference types, constraints (`crates/alder-constrain/src/type_.rs`)
- [x] Type error data (`Type/Error.hs` + `Reporting/Error/Type.hs`, rendering deferred) (`crates/alder-constrain/src/{error,error_type}.rs`)
- [x] Canonical type instantiation (`crates/alder-constrain/src/instantiate.rs`)
- [x] Pattern constraint generation (`crates/alder-constrain/src/pattern.rs`)
- [x] Expression constraint generation incl. recursive defs (`crates/alder-constrain/src/expression.rs`)
- [x] Module constraint entry point (`crates/alder-constrain/src/module.rs`)
- [x] Unification incl. supertypes, records, aliases (`crates/alder-solve/src/unify.rs`)
- [x] Occurs check (`crates/alder-solve/src/occurs.rs`)
- [x] Solver with rank-based generalization and pools (`crates/alder-solve/src/solve.rs`)
- [x] `toAnnotation`/`toErrorType` with fresh-name generation (`crates/alder-solve/src/annotation.rs`)
- [x] Driver pipeline: dependency-ordered builds against solved interfaces (`crates/alder-driver/src/compile.rs`)
- [ ] Exhaustiveness checking (Elm's `Nitpick/PatternMatches.hs`, a post-solve pass)
- [ ] Type error rendering (deferred with the rest of error reporting)

---

## Grammar (EBNF)

The grammar is built up incrementally as features are implemented.

### Notation
```
rule      = definition ;
( ... )   = grouping
[ ... ]   = optional
{ ... }   = zero or more
|         = alternation
"..."     = terminal string
'...'     = terminal char
```

### Lexical

```ebnf
digit     = '0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' ;
lower     = 'a' | ... | 'z' ;
upper     = 'A' | ... | 'Z' ;
```

### Literals

```ebnf
number_literal = decimal_int | hex_int ;
decimal_int    = nonzero_digit { digit } | '0' ;
hex_int        = '0' ( 'x' | 'X' ) hex_digit { hex_digit } ;
nonzero_digit  = '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' ;
hex_digit      = digit | 'a' | 'b' | 'c' | 'd' | 'e' | 'f'
                       | 'A' | 'B' | 'C' | 'D' | 'E' | 'F' ;

string_literal = single_string | multi_string ;
single_string  = '"' { string_char | escape } '"' ;
multi_string   = '"""' { any_char | escape } '"""' ;
string_char    = (* any char except '"', '\', newline *) ;
escape         = '\' ( 'n' | 'r' | 't' | '"' | '\'' | '\' | unicode_escape ) ;
unicode_escape = 'u' '{' hex_digit hex_digit hex_digit hex_digit [ hex_digit [ hex_digit ] ] '}' ;
```

### Expressions

```ebnf
expression     = let_expr | case_expr | if_expr | lambda | binop_expr ;
binop_expr     = possibly_neg_term { term } { operator binop_rhs } ;
binop_rhs      = possibly_neg_term { term } | let_expr | case_expr | if_expr | lambda ;
let_expr       = 'let' let_def { let_def } 'in' expression ;
let_def        = definition | destructure ;
definition     = lower_var [ ':' type_expr ] { pattern } '=' expression ;
destructure    = pattern '=' expression ;
case_expr      = 'case' expression 'of' case_branch { case_branch } ;
case_branch    = pattern '->' expression ;
if_expr        = 'if' expression 'then' expression 'else' expression ;
lambda         = '\' pattern { pattern } '->' expression ;
possibly_neg_term = '-' term | term ;
term           = ( variable | record | tuple ) { '.' lower_var }
               | string | number | list | accessor ;
accessor       = '.' lower_var ;
variable       = lower_var | upper_var | qualified_var ;
lower_var      = lower { inner_char } ;
upper_var      = upper { inner_char } ;
qualified_var  = upper { inner_char } '.' ( lower_var | upper_var | qualified_var ) ;
inner_char     = lower | upper | digit | '_' ;
string         = string_literal ;
number         = number_literal ;  (* no floats in Alder *)
list           = '[' [ expr { ',' expr } ] ']' ;
tuple          = '(' ')' | '(' operator ')' | '(' expr ')' | '(' expr ',' expr { ',' expr } ')' ;
record         = '{' '}' | '{' lower_var '=' expr { ',' field } '}'
               | '{' lower_var '|' field { ',' field } '}' ;
field          = lower_var '=' expr ;
operator       = op_char { op_char } ;  (* except reserved: . | -> = : *)
op_char        = '+' | '-' | '*' | '/' | '=' | '.' | '<' | '>' | ':' | '&' | '|' | '^' | '?' | '%' | '!' ;
```

### Patterns

```ebnf
pattern_expr   = pattern_part { '::' pattern_part } [ 'as' lower_var ] ;
pattern_part   = ctor_pattern | pattern_term ;
ctor_pattern   = ( upper_var | qualified_upper ) { pattern_term } ;
pattern_term   = wildcard | lower_var | ctor_no_args | number | string
               | pattern_record | pattern_tuple | pattern_list ;
wildcard       = '_' ;
ctor_no_args   = upper_var | qualified_upper ;
pattern_record = '{' '}' | '{' lower_var { ',' lower_var } '}' ;
pattern_tuple  = '(' ')' | '(' pattern_expr ')'
               | '(' pattern_expr ',' pattern_expr { ',' pattern_expr } ')' ;
pattern_list   = '[' ']' | '[' pattern_expr { ',' pattern_expr } ']' ;
```

### Types

```ebnf
type_expr      = type_app [ '->' type_expr ] ;
type_app       = upper_var { type_term } ;
type_term      = type_var | type_named | type_tuple | type_record ;
type_var       = lower_var ;
type_named     = upper_var | qualified_upper ;
type_tuple     = '(' ')' | '(' type_expr ')' | '(' type_expr ',' type_expr { ',' type_expr } ')' ;
type_record    = '{' '}' | '{' lower_var ':' type_expr { ',' type_field } '}'
               | '{' lower_var '|' type_field { ',' type_field } '}' ;
type_field     = lower_var ':' type_expr ;
```

### Declarations

```ebnf
declaration    = [ doc_comment ] ( type_decl | value_decl ) ;

value_decl     = lower_var [ ':' type_expr ] { pattern_term } '=' expression ;

type_decl      = 'type' ( alias_decl | union_decl ) ;

alias_decl     = 'alias' upper_var { lower_var } '=' type_expr ;

union_decl     = upper_var { lower_var } '=' variant { '|' variant } ;

variant        = upper_var { type_term } ;

infix_decl     = 'infix' associativity digit '(' operator ')' '=' lower_var ;

associativity  = 'left' | 'right' | 'non' ;
```

### Module

```ebnf
module         = [ module_header ] { import } { infix_decl } { declaration } ;
module_header  = 'module' module_name 'exposing' exposing_list ;
import         = 'import' module_name [ 'as' upper_var ] [ 'exposing' exposing_list ] ;
module_name    = upper_var { '.' upper_var } ;

exposing       = '(' ( '..' | exposed { ',' exposed } ) ')' ;
exposed        = lower_var                        (* value *)
               | '(' operator ')'                 (* operator *)
               | upper_var [ '(' '..' ')' ]       (* type, optionally with constructors *)
               ;
```
