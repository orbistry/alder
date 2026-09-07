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

At an ordinary function boundary, emit the block directly and `return` its tail.
An explicit `async fn` instead returns a lazy `$task(function* () { ... })`,
including when its body has no awaits. An `async { ... }` expression constructs
the same task form with its own control-flow boundary. Neither form is emitted
as a native JavaScript `async` function. A plain function returning an existing
Task does not gain another wrapper or permission to await.

## 3. Stable runtime ABI

| Alder value | JavaScript representation |
| --- | --- |
| `Number`, `BigInt`, `String`, `Bool` | native JS primitive |
| `()` | `undefined` |
| `Array[a]`, tuple | mutable JS array |
| record | ordinary plain object |
| `Map`, `Set` | native JS `Map`, `Set` |
| function | native n-ary function |
| `Task[a]` | frozen reusable generator factory owned by the kernel |
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
const optionBoxes = new WeakSet();
function optionSome(value) {
  if (value === null || optionBoxes.has(value)) {
    const box = { $: "Some", _0: value };
    optionBoxes.add(box);
    return box;
  }
  return value;
}
function optionPayload(value) {
  return optionBoxes.has(value) ? value._0 : value;
}
```

This distinguishes arbitrarily deep `Some(Some(None))`, including through
polymorphic code. Matching `None` checks `=== null`; matching `Some` checks
`!== null` then calls `optionPayload`.
Box identity comes from the WeakSet, not a user-visible tag string, so ordinary
enum/record payloads resembling a Some box are not accidentally unwrapped.

Structural equality is a kernel helper. It recursively compares arrays, plain
records, tagged values, options, and results, tracks visited object pairs, and
uses identity for functions and opaque values.

Dictionary-based derived equality carries a canonical nominal type name and
tracks active object pairs within that nominal domain for one synchronous
comparison. Revisiting a pair closes a cycle; payload dictionaries still run,
so even comparing an object to itself does not bypass NaN or custom equality.
Erased Option layers are unwrapped normally, not pair-memoized. Active entries
are removed on return or throw; later mutations never reuse a cached result.
This cycle guard does not make arbitrarily deep acyclic comparisons stack-safe.

Trait dictionary construction precedes source value initializers. Local
implementations are emitted in a deterministic dependency order derived from
their solved superclass evidence, including dictionary factory arguments.
Factory superclass dependencies participate as well, since initialization can
invoke a factory. Method bodies remain lazy closures and do not impose eager
initialization edges. Source value initializers retain their relative order;
this does not resolve arbitrary forward references between source values.

Closed error-row Show evidence carries canonical tag names and one checked Show
dictionary per payload. The backend constructs variant descriptors directly as
Oxc AST nodes and calls `$showDerived`; it does not choose a dictionary by the
name or declaration order of an error group. Missing payload Show capabilities
are solver errors, not a fallback to generic runtime formatting.
The shared structural evidence path also supports Json, emitting encode/decode
methods with per-payload codecs for `$jsonEncodeDerived`/`$jsonDecodeDerived`.
It does not select a nominal group codec or infer capabilities from runtime data.
Container and structural Json methods share a descriptor-producing closure, so
nested payload evidence is emitted once rather than duplicated into both methods.
The closure remains lazy to preserve recursive dictionary initialization; this
shares generated code, not a cached runtime descriptor.
Container Hash dictionaries use the same lazy-descriptor mechanism. Their Eq
superclass projects each child dictionary's `$super0` after evaluating the shared
thunk once, rather than emitting the entire nested Hash evidence again. This
preserves checked equality dispatch and prevents exponential nesting growth.
Closed error-row Hash uses the same shared payload evidence. `$hashErrorRow`
hashes the active tag, its payload count, and selected child hashes; unlike
nominal enum hashing, it includes neither a type name nor a variant index.
Adding other possible tags therefore cannot change an existing value's hash.
Its Eq superclass projects the same payload dictionaries. The Effect v4 Hash
contract was revisited at the commit pinned in `docs/effects-internals.md`;
no code was copied, and Alder retains its own 64-bit byte-stream protocol.

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
Builtin [array]         alder://std/array.mjs
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

`Try` evaluates its input once. For Result it returns an `Err` unchanged from
the current boundary and otherwise yields `_0`; for Option it returns `None`
or unwraps one Some layer using the centralized Option representation.
`Await` emits `yield*` inside the
enclosing task generator. Task iteration yields a scheduler `Call` operation;
the runtime maintains explicit caller frames, so this syntax does not imply
unbounded native generator delegation. No new codegen helper or child fiber
is needed for sequential awaits. `state(x)` is identity in M2.

All record reads and record-pattern projections use direct member access,
including enum record payloads. Optional declaration shorthand has already
become an ordinary Option type; reads never add another Some layer. Contextual
record construction records omitted Option fields by construction region and
emits their None values explicitly. Enum record constructors use the same
contextual field checking and default metadata. Nested Some(None) stays distinct
from outer None through the normal Option representation, not property presence.

`provide` pushes the value under its canonical provider key, executes the body
inside `try/finally`, and pops in `finally`, which remains correct across await.
`use` reads that key. M4 may change provider validation/storage without changing
generated keys.

Build/Test lowering rejects markup, styles, and queries until their respective
milestones; it does not emit placeholder kernel calls. Their parsed/canonical
structure remains available for checking. Source macro calls and `comptime`
are rejected before codegen.

## 6. Match decisions

M2 lowers patterns to an allocation-free ordered decision chain using Alder
access paths (tuple/array index, enum payload, and record field) and tests (tag,
primitive literal, and array exact/minimum length). The algorithm:

1. evaluates the scrutinee once;
2. expands alternative arm patterns into consecutive decisions sharing one arm;
3. captures each reached extraction path's value, after its parent shape checks;
4. evaluates reached pin operands once in source order, after capturing their
   subject value, short-circuiting after a failed enclosing shape or preceding
   subpattern;
5. installs bindings from captured values before a guard, never rereading a
   path that a later pin may have mutated;
6. continues to the next decision when a pattern or guard fails;
7. assigns/emits the selected arm body.

All alternatives in an arm bind the same names to shared canonical local IDs.
The solver requires corresponding payload types to agree, including aliases
and array-rest bindings, so any successful alternative can safely supply the
shared guard and body. Pins still refer to the enclosing pre-pattern scope.

Captures are ordinary references, not deep copies: a bound object preserves its
identity. Array-rest patterns capture their shallow slice when reached, before
later sibling pins run. Each alternative owns fresh captures; a failed guard
retries the next alternative against the then-current scrutinee. Captures remain
live across suspension inside a pin. No extraction runs before the enclosing
shape permits it. See `pattern-capture-hardening.md` for the mutation regression.

Guard retry is per alternative, not per arm: `(x, _) | (_, x) if check(x)`
tries the first binding, and if its guard is false, tries the second binding
and guard. A failed pattern does not run its guard; a successful guard stops
the chain. The CLI pattern fixture verifies these rules with recorded effects,
including a guard that suspends and resumes before returning false.

An Elm-style pattern-matrix optimizer may later group decisions into switches;
that is a performance optimization rather than part of match semantics.

Array rest bindings use `slice(prefix_len)`. Pins use structural equality unless
known primitive. Runtime fallthrough calls `matchFailure(module, region, value)`;
later exhaustiveness analysis should make it unreachable. Guard failure must
retain a continuation, not merely select an arm index.

Refutable patterns at binding sites (top-level/local lets, named and lambda
parameters, and for bindings) use the same decision logic before extracting
payloads. Failure calls the same source-located matchFailure path; the binding
source is evaluated once. Irrefutable bindings retain direct extraction. Async
parameter checks live inside task execution, preserving lazy task creation and
normal scope cleanup on failure.

## 7. Externs

`#[extern("module", "symbol")]` emits a deduplicated named ESM import. Reserve
module string `globalThis` for validated dotted global paths emitted as bracket
access. Empty/unsafe segments are canonicalization errors. `node:` imports are
rejected for Cloudflare and for standalone while `deno_node` is excluded.

Direct results pass through. For a non-kernel extern, declared `Task[a]` is the
explicit Promise ABI: emit a plain function returning a lazy generator task,
and invoke the foreign symbol inside a `$tryPromise` thunk only when that task
runs. Kernel `Task` externs already return Alder tasks and pass through. The
compiler never probes ordinary extern results to infer async behavior.

`#[extern("module", "symbol", "abort")]` selects the cancellable ABI. It is
valid only with a `Task` return and appends the bridge-created `AbortSignal` to
the foreign call. Generated origin metadata records the Alder module/location
and foreign module/symbol. Raw throws and rejections become foreign defects;
typed errors cross this boundary only as fulfillment values such as
`Task[Result[a, e]]`. Synchronous `Result` externs retain `$tryCatch`.

### Template calls and evaluation order

Ordinary template interpolations evaluate left to right, including each
JavaScript `String` conversion before the next interpolation's effects. Tagged
templates instead evaluate the tag first and pass a string-segment array followed
by the interpolation values, without converting those values to strings. Inference
checks this function signature and preserves its return type, including tasks;
tag references retain any required dictionary evidence.

Lowering may split an expression into setup statements and a final expression.
For ordered operand lists, earlier expressions must be materialized before later
setup executes. Capture references, not copies, except when the operation itself
requires a copy: record spreads snapshot their properties at the spread's source
position. Ordinary templates capture the converted string, not the original
mutable reference. This also preserves effects preceding an early return in a
later operand.

Assignments similarly resolve the target before right-hand setup executes.
Each receiver and computed index is evaluated once; compound assignments also
read the old value before evaluating the right-hand side. Dictionary arithmetic
and intrinsic arithmetic follow the same ordering. Simple targets with an
unlifted RHS may use JavaScript's native assignment evaluation order directly.

## 8. Kernel, stdlib, bundling, and runtime

Kernel TypeScript exports a versioned ABI: enum/option helpers, structural
equality, interpolation, fiber-local provider context, result wrappers,
`$task`, `$tryPromise`, `$runMain`, fiber operations, match failure, and test
registration. Generated code imports `alder:kernel`, never a physical
filename.

The standalone bootstrap installs one frozen, non-enumerable
`globalThis.__alderHost` with `args`, stdout/stderr writes, exit status, and test
events. Generated code never touches `Deno`, `Deno.core`, or raw op names. A
Cloudflare entry supplies the same target-neutral kernel contract with a worker
adapter.

Pin one coherent Deno release family with exact Cargo versions and document the
matrix in `docs/runtime.md`. The implementation baseline uses `deno_core`
0.403.0; in this family URL and console support live in
`deno_web`, so obsolete separate `deno_url`/`deno_console` dependencies are not
added. Extension order is copied from that release and frozen in one function.
Start without a V8 startup snapshot: construct with `try_new`, load the entry
ESM, evaluate it, and drive the event loop. Do not use `MainWorker` or
`deno_node`.

The authoritative stdlib is Alder source under `std/`, with compiler-packaged
copies. Its ordinary interfaces use `PackageId::Builtin` and lowercase paths.
The prelude imports basic operation namespaces through those interfaces;
effectful utilities and JSON operations require explicit imports. Canonical
core type and trait identities stay at the builtin root. Bundled runtime
facades use the same lowercase module IDs. Interface format 8 rejects earlier
serialized contracts rather than translating old module identities.

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

`$runMain` accepts either a synchronous value or an Alder task and runs the
latter through the fiber scheduler. The standalone entry maps unit/`Ok` to exit
0, `Err` to stable stderr rendering and exit 1, and thrown foreign defects or
panics to a failed module evaluation. Rust reads host state rather than
scraping console output.

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
