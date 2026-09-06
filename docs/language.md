# Alder Language

**Status: current direction, everything provisional.** This records the
design decisions from the 2026-09-01 design session. The parser
implements this syntax (M1); nothing past parsing is implemented yet.
Where a decision is open it is marked **Open**.

Alder is a fork of the Elm compiler (ported to Rust) that compiles to
JavaScript. The language is a deliberate mix of OCaml, JavaScript, and
Rust: Elm's type inference and rows underneath, Rust-flavored surface
syntax, JavaScript runtime semantics.

## Principles

- **Honest about JavaScript.** `Number` and `BigInt` instead of `Int`/`Float`,
  arrays are JS arrays, `Option` compiles to `null`, mutation is real.
- **Shared-reference mutation.** Lets and parameters are writable. There is no
  mutation permission or borrow checker; aliasing has JS semantics.
- **Errors are values.** `Result` everywhere, no exceptions, open error
  tags so nobody writes wrapper types.
- **Explicit lazy async.** `async fn` and `async { ... }` construct reusable
  tasks; postfix `.await` runs them on a fiber scheduler.
- **Effects are untracked.** Any function may perform I/O. Purity is not
  enforced by the type system; the compiler tracks only what it needs for
  reactivity and the server/client split.
- **The framework is in the compiler.** Reactivity, server/client split,
  typed markup, routing, and the `table`/`schema`/`style`/`error`
  declaration forms are grammar and compiler passes, not libraries.
  Everything else should be expressible with traits and macros.

## Modules

A file is a module. There is no module header. Items are private unless
marked `pub`. Every module in a package is importable; the `pub` items
are the API.

Imports are path-first: the module path, then optionally `.{ names }` or
`.*`.

```alder
import @alder/http                    // binds `http` (last segment, lowercase)
import @alder/http as h               // binds `h`
import @alder/http.{ get, Request }   // names into scope
import @alder/http.*                  // every pub name into scope
import ~/db/users                     // this package: binds `users`
import ~/db/users.{ find }

http.get(url)
users.find(id)
```

- `@author/package` is a package; `import @author/package` is its root
  module (`mod.ald` at the package source root, the `lib.rs` of the
  package), bound as `package`. Further segments are modules inside it:
  `@author/package/tree` is `tree.ald` or `tree/mod.ald` under that root.
  The root `mod.ald` typically curates the public surface with
  `pub import ~/x.*` re-exports, but any module remains reachable by path.
- `~/` is the root of the current package. There is no `@/` alias and no
  special `~name.ald` index files. A directory's index is `dir/mod.ald` or
  a sibling `dir.ald`, Rust-style. Relative paths (`./`) do not exist.
- User modules are namespaces bound to lowercase names; members are reached
  with `.`. They are not first-class record values. Prelude modules are the
  exception to the spelling convention: built-ins such as `Array`, `Http`,
  and `Fiber` are bound with capitalized names (`Array.map`, `Http.get`,
  `Fiber.all`). Qualified type and trait paths and enum constructors use `::`
  (for example, the builtin configuration type `Fiber::MapOptions`).
- Re-exports are public imports:

```alder
pub import ~/leaf.{ someFunc }
pub import ~/leaf.*                   // typical for mod.ald
```

- Enum constructors are always qualified (`Shape::Circle(1)`) except in
  `match` arms, where the scrutinee type is known and `Circle(r) =>` is
  allowed, and except for the prelude's `Some`/`None`/`Ok`/`Err`.
- **Open:** a convention or attribute for package-internal modules.

## Bindings and mutability

```alder
let x = 1
let count = 0
count += 1

let items = [1, 2]
let alias = items      // same array, JS reference semantics
Array.push(items, 3)   // alias observes the push
```

- Ordinary lets and parameters permit reassignment and field/index writes;
  assignments must preserve their inferred or declared types.
- Local `for` and `match` pattern bindings are writable too. Imported names and
  named function declarations are not replaceable local bindings.
- Rebinding a parameter changes that local binding. Mutating an array or record
  through it is visible to aliases, including the caller.
- Shared replaceable bindings remain subject to the value restriction below.

## Functions

Uncurried, JS-style call syntax. The pipe operator passes the value as the
first argument.

```alder
pub fn add(a: Number, b: Number) Number {
    a + b
}

fn greet(name: String) String {
    `Hello ${name}`
}

let inc = x -> x + 1
let block = x -> {
    let y = x * 2
    y + 1
}

[1, 2, 3]
    |> Array.map(x -> x * 2)
    |> Array.filter(x -> x > 2)
```

- A return type is juxtaposed after the parameter list, begins on that same
  line, and may be omitted when it is inferred (including inferred `Result`
  errors and `Task`). A record return type is parenthesized so its opening
  `{` is not mistaken for the function body.
- The last expression of a block is its value. `return` exits early.
- `name?: T` on a parameter is shorthand for `name: Option[T]`. A trailing
  consecutive suffix of Option parameters may be omitted; omitted arguments
  become `None`. Earlier Option parameters remain required when followed by a
  non-Option parameter. Function-type annotations use ordinary Option types.
- Supplied arguments prefer a direct type match, otherwise adding the fewest
  `Some` layers needed. The same rule applies to newly written record fields,
  including fields around spreads. Expected field types flow through explicit
  `Some` construction, fresh arrays, blocks, and branches. This does not convert existing
  mutable payloads or implicitly wrap an ordinary function's returned value.
- `value |> function(args...)` inserts `value` as the first argument.
  Partial application uses `_` placeholders: `add(1, _)` and
  `Array.map(_, double)` each become a lambda with one parameter per `_`,
  in order. That also selects another pipe position:
  `value |> function(first, _, third)`. Lambdas remain for anything more
  involved.
- Functions have no generic parameter list. Lowercase names in type
  positions are type variables, generalized per declaration:
  `fn first(xs: Array[a]) Option[a]`. Bounds go in a `where` clause.

Top-level `let` bindings obey a value restriction. Function and lambda values,
references to existing values, constructor functions, and scalar literals can generalize
only type variables not tied to shared state. Calls and newly constructed
arrays, records, tuples, maps, sets, or tasks are not generalized at that
binding. This prevents one shared object from being used at incompatible types;
it does not change JavaScript-style aliasing. Top-level bindings assigned anywhere
in the module, including inside nested functions, remain monomorphic. This is
based on resolved binding identity: a never-assigned
function binding can generalize. The analysis conservatively includes writes in
unreachable code. Local block lets remain monomorphic.

For example, `let shared = []` has one element type inferred from its uses,
whereas `fn fresh() { [] }` can be called independently for `Array[Number]` and
`Array[String]`. Exported shared values must have a determined type by the end
of their defining module: use `pub let shared: Array[Number] = []`, or export
a factory. A function capturing shared state cannot re-generalize that state's
type variables.

## Statements and control flow

Function bodies are statement blocks. `if`, `match`, and `loop` are
expressions.

```alder
fn classify(n: Number) String {
    if n < 0 {
        "negative"
    } else if n == 0 {
        "zero"
    } else {
        "positive"
    }
}

for item in items {
    if item.skip { continue }
    total += item.price
}

while pending.length > 0 {
    process(pending.pop())
}

let found = loop {
    let next = iter.next()
    if matches(next) { break next }
}
```

### Layout rules

Items and statements are separated by line breaks. `;` is never a
separator, and two items or two statements on one line is an error.
Comma-separated members (enum variants, match arms, record fields,
parameters) are separated by their commas and may share a line. A record
constructor needs its `{` on the same line as the path
(`Shape::Rect { width: 1 }`); a `{` on the next line starts a block.

### Pinning

`^` means "use the existing value here" wherever a position would
otherwise bind or resolve a name. In `match` patterns it compares against
a binding instead of introducing one (Elixir's pin); in `query { }` blocks
it injects a host value as a bound parameter (see `data.md`).
Pins resolve names in the scope before the arm's pattern introduces bindings.
For `(value, ^value)`, the pin uses the enclosing `value`, while the arm's guard
and body see the newly matched `value`. With no enclosing binding, the pin is
an unknown-name error; reordering pattern fields does not change this rule.
Match-pattern permission does not extend to lets or function parameters inside
the arm's guard or body. A nested match has its own match-pattern permission.

```alder
match input {
    ^expected => "matched the existing value",
    other => `got ${other}`,
}
```

## Types

### Type application and variables

Type arguments use square brackets with commas: `Array[User]`,
`Map[String, Array[User]]`, `Result[User, AuthError]`. Lowercase names are
type variables and never need declaring on functions; only definitions
that fix an arity name them in their head (`enum Result[a, e]`,
`type Cache[k, v] = ...`, `trait Functor[f]`).

```alder
fn zip(xs: Array[a], ys: Array[b]) Array[(a, b)]

fn lookup(cache: Cache[k, v], key: k) Option[v]
    where k: Eq + Hash

fn traverse(xs: t[f[a]], g: fn(a) f[b]) f[t[b]]
    where
        t: Traversable,
        f: Applicative,
```

- `where` takes any number of comma-separated clauses; `+` joins several
  bounds on one variable; `i.Item == Number` constrains an associated type.
- Higher-kinded variables (`f` above) are applied like any other type; their
  kind is inferred from use.
- A type variable named in a nested lambda or local `let` annotation refers
  to the enclosing callable's variable of the same name; otherwise it is
  fresh for that annotation (or lambda signature). Fresh local annotation
  names do not bind names in sibling declarations. Local lets remain
  monomorphic; an annotation does not grant polymorphism to shared values.
- There are no explicit type arguments at call sites. Annotate the binding
  instead: `let users: Array[User] = parse(body)?`.
- A type that starts with `[` is an error row (`[:not_found(Id) | r]`), so
  `Result[User, [:timeout | r]]` is unambiguous.

### Enums

Constructors are namespaced under the type, as in Rust.

```alder
pub enum Option[a] {
    Some(a),
    None,
}

pub enum Shape {
    Circle(Number),
    Rect { width: Number, height: Number },
}

let s = Shape::Rect { width: 1, height: 2 }
let o = Option::Some(3)
```

- `Option::Some` and `Option::None` are in the prelude as `Some`/`None`.
  Other constructors are qualified except inside `match` arms.

### Records and rows

Anonymous records with Elm's row polymorphism stay. Optional fields are new.

```alder
type User = {
    id: Id,
    name: String,
    nickname?: String,        // read as Option[String]
}

fn rename(user: { r | name: String }, name: String) ({ r | name: String }) {
    { ..user, name }
}

let u: User = { id, name: "Ada" }          // nickname omitted
match u.nickname {
    Some(n) => n,
    None => u.name,
}
```

- `field?: T` is shorthand for `field: Option[T]`. Fresh contextual construction
  may omit it, supplying `None`, or supply a `T`, which is wrapped in `Some`.
  An already matching `Option[T]` is stored directly. Reads and assignments use
  the actual `Option[T]` type; assignment does not implicitly wrap a payload.
  Access through an optional parent requires handling that parent's `Option` first.
  Destructuring reads fields by the same rule: `let { field } = record`
  binds `Option[T]` for `field?: T`, including record-shaped enum payloads.
  Omission and explicit outer `None` are equal; `Some(())` and `Some(None)` remain
  distinct from `None`. Structural operations visit every declared field using
  its Option capability. JSON decoding supplies `None` for a missing Option
  field; encoding uses that field's Option codec.
- `{ ..r, x: 1 }` is record update. `r.x` is access, `t.0` tuple index.
  Spreads copy properties in source order. A later field replaces an earlier
  value even when it contains `None`; there is no absence-based fallback.
- `type Name[a] = ...` declares an alias.

### Traits

Haskell-style type classes with Rust spelling and higher-kinded type
parameters. No `self`; trait functions are ordinary functions called by
name or through the pipe.

```alder
enum User { User(String) }

pub trait Show[a] {
    fn show(value: a) String
}

impl Show[User] {
    fn show(user: User) String {
        match user { User::User(name) => name }
    }
}

pub trait Functor[f] {
    fn map(fa: f[a], g: fn(a) b) f[b]
}

enum Box[a] { Box(a) }

impl Functor[Box] {
    fn map(fa: Box[a], g: fn(a) b) Box[b] {
        match fa { Box::Box(value) => Box::Box(g(value)) }
    }
}

trait SequenceIterator[i] {
    type Item
    fn next(it: i) Option[Item]
}

fn describe(value: a) String where a: Show {
    show(value)
}

pub fn main() {
    assert(describe(User::User("Ada")) == "Ada")

    let boxed: Box[Number] = map(Box::Box(1), value -> value + 1)
    assert(boxed == Box::Box(2))
}
```

The builtin `Iterator` trait is implemented by `ArrayIterator[a]`, created with
`Array.iter(values)`. `next(iterator)` returns `Some(value)` and advances that
cursor, or returns `None` permanently after exhaustion. Separate iterators have
independent cursors; aliases of one iterator share progress. Advancing does not
remove source elements. Iteration is live: unread replacements and appends are
visible until exhaustion, and returned objects retain their shared identity.

- Bounds live in `where` clauses: `where a: Show + Eq, k: Hash`. Traits
  may constrain their own parameters the same way
  (`trait Ord[a] where a: Eq`), and impls too
  (`impl Show[Cache[k, v]] where k: Show, v: Show`).
- Associated types are declared one item per line, like `type Item` in the
  `SequenceIterator` example above.

- Default method bodies are allowed in the trait.
- Rust's orphan rule applies: an `impl` must live in the package that
  defines the trait or the type.
- There is no method-call sugar. `show(user)` or `user |> show`, never
  `user.show()`. `.` is for modules, record fields, and tuple indices.
- Enums receive automatic `Eq` when their payloads support it;
  enum `Show`, `Ord`, `Hash`, and `Json` use compiler-backed `#[derive(...)]`
  attributes in M3, replaced by macros in M5 without changing user code.
  Arithmetic is the `Num` trait (`Number`, `BigInt`); comparisons use
  `Ord.compare(left, right) Ordering`, where `Ordering` has `Less`, `Equal`,
  and `Greater` variants. Primitive comparisons lower directly to JavaScript
  relational operators, while generic comparisons inspect that result.

### Errors

`Result[a, e]` is the only failure mechanism. The error position accepts
open tagged constructors written `:tag(payload)`; their type is a row that
grows as errors flow through `?`. The error is inferred: writing
`Result[User]` in a signature leaves the row to the compiler, which
collects every tag the body can produce. Spell the row out only to close
it or to document it.

```alder
fn find(id: Id) Result[User] {                 // error inferred: [:not_found(Id) | r]
    match db.get(id) {
        Some(u) => Ok(u),
        None => Err(:not_found(id)),
    }
}

async fn load(id: Id) Result[Profile] {        // inferred: [:not_found(Id) | :timeout | r]
    let user = find(id)?          // rows merge through ?
    let prefs = fetchPrefs(user).await?
    Ok({ user, prefs })
}

async fn loadStrict(id: Id) Result[Profile, [:not_found(Id) | :timeout]] {
    load(id).await                 // explicit, closed row
}

match load(id) {
    Ok(p) => render(p),
    Err(:not_found(id)) => notFound(id),
    Err(:timeout) => retry(),
    Err(_) => fail(),             // open row needs a catch-all
}
```

Tags can be packaged into a named group, which closes the row:

```alder
pub error AuthError {
    :invalid_token,
    :expired(Timestamp),
}

fn check(token: String) Result[Session, AuthError]
```

- `Result[a]` with one argument means an inferred error row. Hover, docs,
  and the generated `.d.ts` show the inferred row, so `pub` functions still
  have a readable error surface. **Open:** whether `pub` items should be
  required to spell the row for API stability (semver diffing needs it).
- `:tag` outside a `Result` error position is a type error. Tags are not a
  general polymorphic-variant feature.
- A closed `error` group is matched exhaustively. An open row requires `_`.
- Closed error rows support `Show` when every payload supports `Show`, without
  a derive annotation on a named group. Equivalent groups and literal rows
  format identically and respect custom payload implementations. This is a
  selected structural capability, not automatic support for every trait.
- Closed error rows similarly support `Json` when every payload has a codec.
  Equivalent named groups and literal rows share the same tagged JSON format;
  custom payload codecs are honored for both encoding and decoding.
- Closed error rows support `Eq` and `Hash` conditional on payload capabilities.
  Hashing uses the active tag and payloads, not group names, declaration order,
  or other possible tags in the row. Widening a row preserves a value's hash.
  Named groups cannot own custom implementations or derive annotations;
  error rows do not have an automatic `Ord` instance.
- A named group is only a name for a closed row. `?` on a
  `Result[a, AuthError]` inside a function with an open error row flattens
  the group's tags into that row; callers can match `:expired` directly.
  Groups never become wrappers.
- Panics exist for programmer errors and are not catchable by user code;
  the framework installs error boundaries per request/component.

## Async and fibers

An `async fn` returns a lazy, reusable `Task[a]`; its annotation describes the
completed value `a`. Constructing the task does not execute its body, even when
there are no awaits. Tasks compile to generator-based fibers (`yield*`) on a
scheduler in the JS kernel, giving
structured concurrency, interruption, and scopes without an `Effect` type
in user code.

```alder
async fn profile(id: Id) Result[Profile] {
    let user = Http.get(`/users/${id}`).await?
    let posts = Http.get(`/users/${id}/posts`).await?
    Ok({ user, posts })
}

async fn profiles() {
    Fiber.all([profile(1), profile(2)]).await
}
```

For bounded traversal, use `Fiber.map(values, callback, options?)` with a
task-returning callback. Omitting options runs sequentially; a record such as
`{ concurrency: 8 }` bounds active callbacks, including their scope cleanup.
The configuration type is `Fiber::MapOptions`. Configuration and a shallow copy
of input membership are read at each execution; payload objects remain shared.
Collected results retain input order.

```alder
async fn profiles(ids: Array[Id]) Array[Result[Profile]] {
    ids |> Fiber.map(id -> async { profile(id).await }, { concurrency: 8 }).await
}
```

`map` collects returned `Result` values without cancelling successful siblings.
Use `tryMap` to stop on the first observed `Err`, cancel active work, and wait
for cleanup before returning `Result[Array[a], e]`. `forEach` instead requires
unit-returning tasks and allocates no result array; `tryForEach` requires
`Result[(), e]` task results and returns `Result[(), e]`. Invalid concurrency
bounds fail at execution as defects, not recoverable Alder errors.

- `Task` is a visible type. A plain function may return an existing task
  (`fn load(id: Id) Task[Result[User]]`); that annotation does not authorize
  await inside its body. An un-awaited async call is a `Task` value you can pass
  to `Fiber.fork`, `Fiber.all`, or `Fiber.race`.
- `async` adds exactly one Task layer, without flattening. For example,
  `async fn nested() Task[Number] { async { 42 } }` returns
  `Task[Task[Number]]`, requiring two awaits to obtain the number.
- `async { ... }` constructs a task with its own await, return, and `?`
  boundary. Break and continue cannot escape that boundary. Lambdas use ordinary
  syntax: `x -> async { x + 1 }`, or
  `(x: Number) Task[Number] -> async { x + 1 }`. There is no async-lambda prefix.
- Async blocks capture lexical bindings, not value snapshots. Rebinding and
  writes through shared aliases remain visible; repeated task runs may observe
  different state. Function call arguments evaluate at call time, before the
  async body runs. Capture does not provide synchronization.
- `.await?` means “await, then propagate the resolved `Result` error.” It is
  ordinary postfix composition, not a special fused operation.
- Pipe forwarding happens before postfix operations on a destination. Thus
  `request |> send(client).await?` means
  `send(request, client).await?`.
- The last task-producing stage can be awaited directly, without wrapping the
  whole pipeline:

  ```alder
  value
      |> prepare
      |> startOperation().await?
  ```

  To sequence multiple asynchronous stages, await each stage before its value
  is forwarded: `value |> start().await? |> transform().await?`. Omitting
  `.await` passes the `Task` itself.
- `Fiber.fork` returns a fiber handle, `join` awaits it, `interrupt` requests
  cooperative interruption, `all` preserves input order, and `race` returns
  the first exit after interrupting and cleaning up its losers.
- Every fiber owns its child scope. Leaving that scope interrupts and joins
  remaining children, then runs registered finalizers once in LIFO order.
  `Fiber.scope`, `Fiber.addFinalizer`, and `Fiber.uninterruptible` expose the
  minimal structured-cleanup surface.
- `main` and test declarations may produce tasks. Generated entries recognize
  and run them on the kernel scheduler automatically.
  Use `pub async fn main() { ... }`, or return a task from a plain main.
  Tests may return an async block: `test "wait" { async { ... } }`.
  The root scheduler does not authorize await in a plain function or test body.

## Context (dependency injection)

Services are requested by type with `use` and supplied by `provide` in an
enclosing scope. Missing providers are compile errors at entry points.

```alder
async fn saveUser(user: User) Result[()] {
    use Db
    Db.insert(users, user).await
}

async fn main() {
    provide Db = Sqlite.open("app.db") {
        saveUser(u).await
    }
}
```

- Providers are resolved lexically through the call graph and, in the web
  runtime, through the render tree, so SSR gets per-request isolation.
- Tests swap providers with `provide Db = FakeDb.new() { ... }`.
- `provide … { }` is currently a statement, not a value-producing expression.
  To return a value from its body, use an explicit `return` belonging to the
  enclosing function or async block. Await still requires an explicit async
  boundary. Provider requirement checking and the web integration described
  above remain future milestone work; the current runtime context mechanism
  does not establish those compile-time guarantees.

## Numbers, strings, collections

- `Number` is the JS double. `BigInt` maps to JS BigInt. There is no `Int`;
  `/` is always float division. Stdlib functions that need integers check
  at runtime.
- Strings are JS strings. Interpolation uses template literals:
  `` `Hello ${name}` ``. Tagged templates exist for escape hatches such as
  `sql` and `css`. Double-quoted strings do not interpolate.
- `Array[a]` is a mutable JS array. Literals `[1, 2, 3]`. There is no
  linked `List`.
- `Option[a]` compiles to `a | null`; nested `Option[Option[a]]` boxes the
  inner value. FFI values that may be null are typed `Option`.
- `==` and `!=` are the `Eq` trait (see Traits): structural for records,
  enums, tuples, arrays, `Option`, and `Result`, derived automatically, a
  compile error on functions. Known primitives compile to `===`.
  `Ref.same(a, b)` compares identity.
- `Map[k, v]` and `Set[a]` are JS Map/Set with identity keys. Record keys
  compare by reference; the docs warn about it. There is no structural
  dictionary in the first version.

## Shared cells

`Ref[a]` is an opaque shared cell. Its operations return lazy reusable tasks:
`Ref.make(value)`, `Ref.get(cell)`, `Ref.set(cell, value)`,
`Ref.update(cell, value -> replacement)`, and
`Ref.modify(cell, value -> (result, replacement))`.
`update` and `set` complete with unit; `modify` completes with `result`.

```alder
pub async fn main() {
    let count = Ref.make(0).await
    Ref.update(count, value -> value + 1).await
    let previous = Ref.modify(count, value -> (value, value + 1)).await
    assert previous == 1
    assert Ref.get(count).await == 2
}
```

An update's synchronous read–callback–write cannot interleave with another
fiber. This is not a cross-worker atomic or protection for writes through
escaped payload aliases. Callbacks cannot await; returning a Task as the cell's
payload stores it without executing it. A thrown defect leaves the cell's
binding unchanged, but mutations through aliases are not rolled back.

Each execution of a make task allocates a fresh cell, not a fresh copy of its
argument. For example, repeated execution of `Ref.make([])` shares the array
evaluated when that task was constructed. Allocate inside an async body when
each run needs a fresh payload. Type inference keeps shared payloads from being
instantiated at incompatible types. `Ref.same` remains an immediate identity
comparison, not a task operation.

`SynchronizedRef[a]` supports transformations that may suspend. Its `make`,
`get`, and `set` have the same task-shaped API as Ref; `update` takes
`fn(a) Task[a]`, and `modify` takes `fn(a) Task[(b, a)]` and returns `Task[b]`.

```alder
pub async fn main() {
    let count = SynchronizedRef.make(0).await
    let increment = SynchronizedRef.update(count, value -> async {
        Task.sleep(0).await
        value + 1
    })
    Fiber.all([increment, increment]).await
    assert SynchronizedRef.get(count).await == 2
}
```

Writes serialize, and callbacks read the latest value only after acquiring the
cell's lock. Reads do not wait for the lock: they see the last committed binding
while an update is suspended. A defect or interruption before commit leaves
that binding unchanged; neither later cleanup failures nor alias mutations are
rolled back. The lock remains held through protected cleanup. Updating the same
cell again from its own update is non-reentrant and can deadlock.

Results remain ordinary values. A modify callback can return
`(Err(error), oldValue)` to report a typed error without replacing state; no
implicit error channel is added to these operations.

## Semaphores

`Semaphore.make(capacity).await` creates a fixed-capacity semaphore.
`Semaphore.withPermits(gate, count, task).await` waits for permits, runs the
task in an owned scope, and releases permits after its children and finalizers
finish. The wrapper is lazy and reusable; it preserves the task's result,
including ordinary `Err` values, and releases on defects or interruption too.

```alder
pub async fn main() {
    let gate = Semaphore.make(1).await
    let protected = Semaphore.withPermits(gate, 1, async { 42 })
    assert Fiber.all([protected, protected]).await == [42, 42]
}
```

Capacity and requested count must be positive safe integers, and the count
cannot exceed capacity. Invalid values produce a runtime RangeError defect
when the task executes. Waiters are FIFO: a smaller request does not bypass
an older larger request. Waiting is interruptible, and cancellation does not
leak permits. Acquisition is not reentrant; reacquiring permits already held
by the same operation can deadlock. There is no manual release or resize API.

## Typed markup

Markup looks like JSX but is a typed HTML DSL: elements, attributes, and
children are checked against a schema, not stringly typed. Expressions
are embedded with `{expr}`. Control flow in child position uses `@`
directives with no wrapping braces, following Octane's TSRX.

```alder
<ul class={styles.list}>
    @for item in items; key item.id {
        <li>{item.name}</li>
    } @empty {
        <li>Nothing here</li>
    }
    @if status.loading {
        <Spinner />
    } @else if status.failed {
        <p>Something went wrong</p>
    } @else {
        <p>{count} items</p>
    }
    @match status {
        Loading => <Spinner />,
        Ready(n) => <span>{n}</span>,
    }
</ul>
```

- `@if` without `@else` renders nothing when false. `@for` takes an
  optional `; key expr` for keyed reconciliation and an optional `@empty`
  branch.
- A directive body is a list of children. Statements (`let x = ...`) are
  allowed inside for setup and do not render; only markup and `{expr}`
  holes produce output.
- `@` is unambiguous in child position because text never starts with
  `@if`, `@for`, or `@match` followed by a space. Literal `@` in text is
  written `{"@"}`.
- Components are used as capitalized elements; props are a record type,
  so optional fields make optional props natural.
- The element vocabulary is per target: HTML for web, a separate set for
  TUI. See `web.md`.

## Components and state

```alder
pub component Counter(props: { start?: Number, label: String }) {
    let count = state(props.start ?? 0)
    let double = count * 2                     // memoized automatically

    <button onClick={() -> count += 1}>
        {props.label}: {count} ({double})
    </button>
}
```

- `component` bodies run once. `state(...)` bindings are signals; the
  compiler tracks reads of them in expressions and markup (Svelte 5 rune
  style) and memoizes derived values. Plain `let` bindings that do not
  read state are not reactive.
- `??` unwraps exactly one `Option` layer with a default: `Option[a] ?? a`
  produces `a`. The left operand is evaluated once; the default is evaluated
  only for `None`. `Some(())` and `Some(None)` are present values.

## Attributes and macros

Attributes use Rust syntax and are how the compiler and packages mark
items. Macros are real: Alder functions from syntax to syntax, executed at
build time in the compiler's embedded V8, with Elixir-style
`quote`/`unquote` and Jai-style `comptime` blocks.

```alder
#[derive(Show, Eq, Json)]
enum Shape {
    Point { x: Number, y: Number },
    Circle(Number),
}

macro assert_eq(left, right) {
    quote {
        let l = unquote(left)
        let r = unquote(right)
        if l != r { Test.fail(unquote(stringify(left)), l, r) }
    }
}

comptime {
    let routes = Fs.readDir("routes")
    ...
}
```

- Attribute, derive, and function-like (`name!(...)`) macro forms.
- **Open:** hygiene rules, the public AST/`TokenStream` API and its
  stability, sandboxing of compile-time code, and caching of macro output.

## Tests

`test` is a declaration. `assert` is compiler-known so failures show both
sides of a comparison (power-assert style). A module-level `tests` block
only exists under `alder test`; its imports and helpers are pruned from
every other build. A test body has an implicit open `Result` failure context,
so it may use `?`; returning an `Err` fails that test and reports its value.

```alder
tests {
    import @alder/test.{ fakeDb }

    test "adds numbers" {
        assert add(1, 2) == 3
    }

    test "finds a user" {
        async {
            provide Db = fakeDb() {
                assert find(1).await == Ok(ada)
            }
        }
    }
}
```

## FFI

JavaScript is reached through bodiless functions carrying an `extern`
attribute. The compiler trusts the declared types. First-party packages
wrap important libraries. Alder also emits `.d.ts` for its `pub` items so
TypeScript can consume Alder modules.

```alder
#[extern("globalThis", "crypto.randomUUID")]
fn randomUUID() String

#[extern("./files.js", "readFile")]
fn readFile(path: String, encoding: String) Task[Result[String, [:io(String)]]]

#[extern("./client.js", "request", "abort")]
fn request(url: String) Task[Response]

#[extern("globalThis", "JSON.parse")]
fn parseJson(s: String) Result[Json, [:syntax(String)]]
```

- Relative extern modules such as `"./client.js"` resolve beside the declaring
  Alder source file, not the shell's working directory. A JS wrapper's own
  relative imports resolve beside that wrapper normally.
- A JS function returning a Promise must be declared `Task[...]`. The Promise
  producer is lazy: calling the Alder extern constructs a task, and the JS
  function is invoked only when that task runs.
- Promise fulfillment becomes task success. A synchronous foreign throw or raw
  Promise rejection is a contextual foreign-runtime defect; Alder does not
  invent a typed error row from an arbitrary JavaScript value.
- `Task[Result[a, e]]` means the Promise fulfills with Alder `Result` data.
  `.await?` propagates that declared `e` normally.
- The optional `"abort"` convention requires a `Task` return and appends an
  `AbortSignal` to the JS call. Interruption aborts it once. Without the
  convention, interruption detaches the waiter but cannot cancel the foreign
  operation; late settlement is ignored and late rejection remains observed.
- If a synchronous extern declares `Result`, the kernel wraps its call in
  try/catch. Otherwise a synchronous throw is a panic/defect.
- Plain-data JS objects are typed as records and used directly at zero
  cost. Class instances are opaque types declared with
  `#[extern] type Response` and accessed through extern functions.
- No automatic consumption of `.d.ts` files.

## Open questions (collected)

- Convention or attribute for package-internal modules.
- Macro hygiene and the compile-time API surface.
- Enum and record runtime representation (see `runtime.md`).
- Whether `provide … { }` is a statement or an expression (M2).
