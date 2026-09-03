# Code generation, runtime, and tooling internals

This is the M2b implementation contract. It fixes the runtime ABI before SSR,
hydration, macros, and specialized framework lowering depend on it. The design
panel compared emitter correctness, embedded-runtime integration, and formatter
architecture; the decisions below are the synthesis.

## 1. Phase and crate boundaries

Code generation runs while a solved canonical module and its arena are alive.
It builds Oxc nodes in the allocator owned by Rolldown's self-referential
`EcmaAst` container and returns that owned AST:

```rust
pub struct SolvedModule<'a> {
    pub module: &'a alder_ast::Module<'a>,
    pub annotations: &'a alder_can::Annotations<'a>,
}

pub struct EmittedModule {
    pub module_id: String,
    pub ast: rolldown_ecmascript::EcmaAst,
    pub dependencies: Vec<String>,
}
```

The M2 solver exposes top-level annotations. A later typed-AST side table adds
per-expression types when representation-sensitive specialization needs them;
the generic M2 emitter must not guess types it does not have.

- `alder-codegen`: solved canonical AST directly to Oxc's `Program` inside a
  Rolldown-compatible owned `EcmaAst`. It has no filesystem or runtime
  dependency and pins the exact `rolldown_ecmascript`/Oxc versions used by the
  bundler.
- `alder-bundle`: the only crate allowed to expose Rolldown Rust APIs internally.
  Its public API consists only of Alder-owned request/output structs.
- `alder-kernel`: target-neutral TypeScript helpers and entry adapters, bundled
  at build time and embedded from `OUT_DIR`.
- `alder-runtime`: `deno_core`, web extensions, host ops, module execution, and
  runtime tests. The driver and language server never link V8.
- `alder-fmt`: source-AST formatter and document printer; independent of the
  canonical/type/runtime pipeline.
- `alder-driver`: orchestration with `Check`, `Emit`, and `Test` modes. Emitted
  modules are owned before the module arena can be dropped.

Rolldown's Rust crates are explicitly unstable. `EcmaAst` crosses only the
private compiler-to-bundler boundary; user-facing crates never manipulate it.
The codegen and bundler pins must be upgraded together.

## 2. JavaScript IR and lifting

The emitter constructs Oxc's JavaScript AST directly rather than concatenating
JavaScript text or maintaining a parallel codegen-owned syntax tree:

```rust
struct Value<'js> {
    prefix: oxc_allocator::Vec<'js, oxc_ast::ast::Statement<'js>>,
    expr: oxc_ast::ast::Expression<'js>,
}
```

Every emitter receives fresh-name state, loop labels, return/async mode,
deduplicated imports, and requested kernel helpers. `Value::prefix` is moved
only to a point with identical execution timing and ordering. Earlier operands
are materialized before a later operand's prefix. Prefixes never escape a
short-circuit RHS, unselected branch, match arm/guard, loop body, or lambda.

Expression blocks use a temporary, never an IIFE:

```js
let $t0;
// block statements
$t0 = tail;
```

Value-position `if` assigns the same temporary inside each selected branch.
Later `else if` condition prefixes stay inside the preceding `else`. A loop in
value position uses a labeled `for (;;)`, and `break value` assigns its result
temporary before breaking. A bare break produces `undefined`.

At a function boundary, emit the block directly and `return` its tail. Scan for
`Await` outside nested functions and emit that function as `async`.

## 3. Stable runtime ABI

| Alder value | JavaScript representation |
| --- | --- |
| `Number`, `BigInt`, `String`, `Bool` | native JS primitive |
| `()` | `undefined` |
| `Array[a]`, tuple | mutable JS array |
| record | ordinary plain object |
| `Map`, `Set` | native JS `Map`, `Set` |
| function | native n-ary function |
| `Task[a]` in M2 | `Promise<a>` |
| enum / `Result` / error tag | tagged object |
| `Option[a]` | nullable encoding with dynamic boxing |
| extern opaque type | unchanged JS value |
| `Style`, `Query`, `Html` | kernel-owned opaque object |
| provider identity | canonical module-and-name string |

Functions are never implicitly curried. Tuples are arrays in positional order.
Record keys keep source spellings; dangerous literal keys such as `__proto__`
use computed properties so they cannot mutate an object's prototype.

Enum tuple payloads use `{ $: "Some", _0: value }`; record payloads use
`{ $: "Rect", width, height }`. Unit variants are shared frozen objects such
as `Object.freeze({ $: "Red" })`. Anonymous error tags use the same shape with
the colon retained, such as `{ $: ":io", _0: message }`. String tags remain
the ABI in production as well as development.

### Option boxing

`None` is `null`. `Some(x)` uses a kernel helper that returns unboxed `x` when
safe and boxes null or an existing option box:

```js
const $OPTION_BOX = "$alder$Some";
function optionSome(value) {
  return value === null || value?.$ === $OPTION_BOX
    ? { $: $OPTION_BOX, _0: value }
    : value;
}
function optionPayload(value) {
  return value?.$ === $OPTION_BOX ? value._0 : value;
}
```

This distinguishes arbitrarily deep `Some(Some(None))`, including through
polymorphic code. Matching `None` checks `=== null`; matching `Some` checks
`!== null` then calls `optionPayload`.

Structural equality is a kernel helper. It recursively compares arrays, plain
records, tagged values, options, and results, tracks visited object pairs, and
uses identity for functions and opaque values.

## 4. Names, modules, and emission order

Bindings use collision-proof prefixes and byte escaping:

```text
local        $l<LocalId>_<escaped-name>
top level    $v_<escaped-name>
constructor $c_<escaped-enum>_<escaped-variant>
temporary    $t<id>
extern       $x<id>
loop         $loop<id>
```

ASCII identifier bytes remain; every other UTF-8 byte becomes `_HH`. Property
names are not mangled and are represented as literal property-key nodes, so
escaping and precedence are Oxc's responsibility. Virtual module IDs are stable:

```text
Application [foo]       alder://app/foo.mjs
Named a/p [x,y]         alder://pkg/a/p/x/y.mjs
Builtin [Array]         alder://std/Array.mjs
kernel                  alder://kernel/index.mjs
```

Imports are derived from resolved foreign references, deduplicated, and sorted
by canonical module ID and symbol. A public value is exported under its source
name; public constructors additionally expose stable compiler-linkage exports.
`ValueRef::Module` is a namespace marker and must not reach ordinary value
emission.

Emission order is dependency imports, kernel imports, extern imports, unit
constructor singletons/functions, named functions/components, eager values,
test registrations, then the public export list. Named functions use JS
function declarations for mutual recursion. A top-level destructuring let
evaluates once into a temporary. A recursive SCC containing an eager non-function
value is rejected rather than observing JavaScript TDZ/`undefined` behavior.

## 5. Expressions, control flow, and providers

Primitive arithmetic/comparison/boolean operators emit native JS. Structural
`==`/`!=` uses the kernel unless a solved primitive type proves strict equality
is enough. A pipe into a bare function calls it with the left value; a pipe
into an existing call inserts the left value as the first source argument,
after any hidden dictionary arguments. Calls containing `_` have already been
canonicalized into lambdas, so a placeholder explicitly selects another pipe
position. The left value is evaluated before the callee and existing arguments.
`??` preserves short-circuit RHS lifting.

`Try` evaluates its `Result` once, returns an `Err` unchanged from the current
function, and otherwise yields `_0`. `Await` emits native `await`. `state(x)` is
identity in M2.

`provide` pushes the value under its canonical provider key, executes the body
inside `try/finally`, and pops in `finally`, which remains correct across await.
`use` reads that key. M4 may change provider validation/storage without changing
generated keys.

Markup, styles, and queries call kernel constructors in M2. Their canonical
structure remains intact for later specialized lowering. Macro calls and
`comptime` are rejected before codegen.

## 6. Match decisions

M2 lowers patterns to an allocation-free ordered decision chain using Alder
access paths (tuple/array index, enum payload, and record field) and tests (tag,
primitive literal, and array exact/minimum length). The algorithm:

1. evaluates the scrutinee once;
2. expands alternative arm patterns into consecutive decisions sharing one arm;
3. records local/alias bindings as extraction paths;
4. evaluates pin operands once in source order;
5. installs bindings before a guard;
6. continues to the next decision when a pattern or guard fails;
7. assigns/emits the selected arm body.

An Elm-style pattern-matrix optimizer may later group decisions into switches;
that is a performance optimization rather than part of match semantics.

Array rest bindings use `slice(prefix_len)`. Pins use structural equality unless
known primitive. Runtime fallthrough calls `matchFailure(module, region, value)`;
later exhaustiveness analysis should make it unreachable. Guard failure must
retain a continuation, not merely select an arm index.

## 7. Externs

`#[extern("module", "symbol")]` emits a deduplicated named ESM import. Reserve
module string `globalThis` for validated dotted global paths emitted as bracket
access. Empty/unsafe segments are canonicalization errors. `node:` imports are
rejected for Cloudflare and for standalone while `deno_node` is excluded.

Direct and `Task` extern results pass through. `Result` results use synchronous
or async kernel try/catch wrappers; argument evaluation remains outside the
catch. Until M4 specifies exception-to-row mapping, a catching extern may only
use one closed unary error tag.

## 8. Kernel, stdlib, bundling, and runtime

Kernel TypeScript exports a versioned ABI: enum/option helpers, structural
equality, interpolation, provider stack, result wrappers, `runMain`, match
failure, and test registration. Generated code imports `alder:kernel`, never a
physical filename.

The standalone bootstrap installs one frozen, non-enumerable
`globalThis.__alderHost` with `args`, stdout/stderr writes, exit status, and test
events. Generated code never touches `Deno`, `Deno.core`, or raw op names. A
Cloudflare entry supplies the same target-neutral kernel contract with a worker
adapter.

Pin one coherent Deno release family with exact Cargo versions and document the
matrix in `docs/runtime.md`. The implementation baseline is Deno 2.8.1
(`deno_core` 0.402.0); in this family URL and console support live in
`deno_web`, so obsolete separate `deno_url`/`deno_console` dependencies are not
added. Extension order is copied from that release and frozen in one function.
Start without a V8 startup snapshot: construct with `try_new`, load the entry
ESM, evaluate it, and drive the event loop. Do not use `MainWorker` or
`deno_node`.

The authoritative stdlib is Alder source under `std/`, embedded with a content
fingerprint and compiled as `PackageId::Builtin` before application modules.
The prelude injects capitalized module bindings. The cache key includes compiler
version, stdlib fingerprint, target, and build mode.

Rolldown receives application modules as owned `EcmaAst` values through a
virtual-module plugin. Its public loader currently insists on parsing before
the experimental `transform_ast` hook, so the plugin supplies an empty source
and replaces that empty AST; generated Alder JavaScript is never serialized or
reparsed. Oxc printing occurs only for final artifacts, snapshots, and debug
views. The dynamically generated entry module is also built directly as an Oxc
AST. Handwritten kernel TypeScript and small stdlib bridge modules remain normal
auditable source strings and are parsed by Rolldown. Unresolved imports are
rejected except an explicit extern allowlist. `node:` is always rejected.
Standalone synthesizes an entry that calls `main`; Cloudflare synthesizes a
module-worker default export.

## 9. Tests and commands

`runMain` accepts sync/promise main, maps unit/`Ok` to exit 0, `Err` to a stable
stderr rendering and exit 1, and maps thrown/rejected JS exceptions to a panic
exit. Rust reads host state rather than scraping console output.

Test mode retains `test`/`tests` declarations and emits lazy registry entries.
The registry reports structured events to the host; only the CLI formats plain
pass/fail. M2 tests execute in the embedded standalone runtime.

Commands share the driver pipeline:

```text
alder check [PATH]
alder build [PATH] [--out-dir DIR]
alder run [PATH] [-- ARGS...]
alder test [PATH]
alder fmt [PATH...] [--check]
```

Only `main` selects the process exit code; command implementations return typed
outcomes.

## 10. Comments and formatter

Comments are a source-AST side table:

```rust
pub enum CommentKind { Line, OuterDoc, InnerDoc }
pub struct Comment<'a> {
    pub region: Region,
    pub kind: CommentKind,
    pub text: &'a str, // exact lexeme including //, excluding newline
}
```

`source::Module` owns the ordered slice. The parser records comments while
chomping whitespace. Crucially, parser checkpoints store `comments_len` and
restore truncates the vector so lookahead/backtracking cannot duplicate comments.
Comments inside raw macro/comptime bodies stay only in their preserved raw text.

Formatter attachment uses an ordered cursor: same-line comments trail the
completed node; otherwise comments lead the nearest following node; contained
unconsumed comments precede a closing delimiter; `//!` is module-leading,
`///` attaches to the next item, and leftovers trail the module. Columns may
classify placement but never slice UTF-8 source.

`alder-fmt` uses a Wadler-style document algebra with groups, soft/hard lines,
and nesting. It formats the source AST at width 100, four spaces, LF, and one
terminal newline. Imports retain order, multiline comma lists gain trailing
commas, block statements occupy lines, and markup text/raw macro bodies are
never reflowed. The side table preserves comments, not arbitrary blank-line
choices.

`alder fmt` reads and parses every target before writing any, sorts paths,
writes only changed files, and makes `--check` non-mutating. Required invariants
are idempotence, parse/format structural equivalence (ignoring regions/comments),
comment-order preservation, and corpus coverage over all `.ald` and full-module
docs examples.

## 11. Required implementation gates

1. Fix source-fetch ordering so dependency order is deterministic.
2. Land codegen IR, expression lifting, enum ABI, and JS snapshots.
3. Land kernel helpers and execute codegen fixtures in a minimal runtime.
4. Add embedded stdlib/prelude.
5. Add comment capture and formatter, then corpus invariants.
6. Add driver emit mode and isolated bundling.
7. Add `build`, embedded `run`, and the test registry/command.
8. Run runtime/e2e tests on supported platforms; update `SPEC.md` and changesets.
