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
- **Rust-like mutability, not Rust-like ownership.** `mut` is a binding
  permission. There is no borrow checker; aliasing has JS semantics.
- **Errors are values.** `Result` everywhere, no exceptions, open error
  tags so nobody writes wrapper types.
- **Async without ceremony.** Postfix `.await`, asyncness is inferred,
  everything runs on a fiber scheduler.
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
- Modules are values bound to lowercase names; members are reached with
  `.`. `::` is only for enum constructors and trait paths.
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
let mut count = 0
count += 1

let mut items = [1, 2]
items.push(3)          // allowed: binding is mut

let alias = items      // same array, JS reference semantics
```

- `let` bindings are immutable; reassignment or calling a mutating method
  requires `let mut`.
- `mut` does not prevent aliasing. `alias` above observes the push.
- Function parameters may be declared `mut` to mutate in place.

## Functions

Uncurried, JS-style call syntax. The pipe operator passes the value as the
first argument.

```alder
pub fn add(a: Number, b: Number) -> Number {
    a + b
}

fn greet(name: String) -> String {
    `Hello ${name}`
}

let inc = fn(x) x + 1
let block = fn(x) {
    let y = x * 2
    y + 1
}

[1, 2, 3]
    |> Array.map(fn(x) x * 2)
    |> Array.filter(fn(x) x > 2)
```

- Return type follows `->` after the parameter list and may be omitted
  when it is inferred (including inferred `Result` errors and `Task`).
- The last expression of a block is its value. `return` exits early.
- Partial application uses `_` placeholders: `add(1, _)` and
  `Array.map(_, double)` each become a lambda with one parameter per `_`,
  in order. Lambdas remain for anything more involved.
- Functions have no generic parameter list. Lowercase names in type
  positions are type variables, generalized per declaration:
  `fn first(xs: Array[a]) -> Option[a]`. Bounds go in a `where` clause.

## Statements and control flow

Function bodies are statement blocks. `if`, `match`, and `loop` are
expressions.

```alder
fn classify(n: Number) -> String {
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
fn zip(xs: Array[a], ys: Array[b]) -> Array[(a, b)]

fn lookup(cache: Cache[k, v], key: k) -> Option[v]
    where k: Eq + Hash

fn traverse(xs: t[f[a]], g: fn(a) -> f[b]) -> f[t[b]]
    where
        t: Traversable,
        f: Applicative,
```

- `where` takes any number of comma-separated clauses; `+` joins several
  bounds on one variable; `i.Item == Number` constrains an associated type.
- Higher-kinded variables (`f` above) are applied like any other type; their
  kind is inferred from use.
- A type variable named in a nested lambda's annotation refers to the
  enclosing function's variable of the same name; otherwise it is fresh.
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

fn rename(user: { r | name: String }, name: String) -> { r | name: String } {
    { ..user, name }
}

let u: User = { id, name: "Ada" }          // nickname omitted
match u.nickname {
    Some(n) => n,
    None => u.name,
}
```

- `field?: T` declares an optional field. Construction may omit it; reading
  it yields `Option[T]`. Callers never write `Some(...)` for props.
- `{ ..r, x: 1 }` is record update. `r.x` is access, `t.0` tuple index.
- `type Name[a] = ...` declares an alias.

### Traits

Haskell-style type classes with Rust spelling and higher-kinded type
parameters. No `self`; trait functions are ordinary functions called by
name or through the pipe.

```alder
pub trait Show[a] {
    fn show(value: a) -> String
}

impl Show[User] {
    fn show(user: User) -> String { user.name }
}

pub trait Functor[f] {
    fn map(fa: f[a], g: fn(a) -> b) -> f[b]
}

impl Functor[Option] {
    fn map(fa: Option[a], g: fn(a) -> b) -> Option[b] {
        match fa {
            Some(x) => Some(g(x)),
            None => None,
        }
    }
}

fn describe(xs: Array[a]) -> String where a: Show {
    xs |> Array.map(show) |> String.join(", ")
}
```

- Bounds live in `where` clauses: `where a: Show + Eq, k: Hash`. Traits
  may constrain their own parameters the same way
  (`trait Ord[a] where a: Eq`), and impls too
  (`impl Show[Cache[k, v]] where k: Show, v: Show`).
- Associated types are declared one item per line, like every other
  trait item:

  ```alder
  trait Iterator[i] {
      type Item
      fn next(it: i) -> Option[Item]
  }
  ```

- Default method bodies are allowed in the trait.
- Rust's orphan rule applies: an `impl` must live in the package that
  defines the trait or the type.
- There is no method-call sugar. `show(user)` or `user |> show`, never
  `user.show()`. `.` is for modules, record fields, and tuple indices.
- `Eq` is derived automatically for every type whose parts are `Eq`;
  `Show`, `Ord`, `Hash`, and `Json` are `#[derive(...)]` macros. Arithmetic
  is the `Num` trait (`Number`, `BigInt`); comparisons are `Ord`.

### Errors

`Result[a, e]` is the only failure mechanism. The error position accepts
open tagged constructors written `:tag(payload)`; their type is a row that
grows as errors flow through `?`. The error is inferred: writing
`Result[User]` in a signature leaves the row to the compiler, which
collects every tag the body can produce. Spell the row out only to close
it or to document it.

```alder
fn find(id: Id) -> Result[User] {              // error inferred: [:not_found(Id) | r]
    match db.get(id) {
        Some(u) => Ok(u),
        None => Err(:not_found(id)),
    }
}

fn load(id: Id) -> Result[Profile] {           // inferred: [:not_found(Id) | :timeout | r]
    let user = find(id)?          // rows merge through ?
    let prefs = fetchPrefs(user).await?
    Ok({ user, prefs })
}

fn loadStrict(id: Id) -> Result[Profile, [:not_found(Id) | :timeout]] {
    load(id)                       // explicit, closed row
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

fn check(token: String) -> Result[Session, AuthError]
```

- `Result[a]` with one argument means an inferred error row. Hover, docs,
  and the generated `.d.ts` show the inferred row, so `pub` functions still
  have a readable error surface. **Open:** whether `pub` items should be
  required to spell the row for API stability (semver diffing needs it).
- `:tag` outside a `Result` error position is a type error. Tags are not a
  general polymorphic-variant feature.
- A closed `error` group is matched exhaustively. An open row requires `_`.
- A named group is only a name for a closed row. `?` on a
  `Result[a, AuthError]` inside a function with an open error row flattens
  the group's tags into that row; callers can match `:expired` directly.
  Groups never become wrappers.
- Panics exist for programmer errors and are not catchable by user code;
  the framework installs error boundaries per request/component.

## Async and fibers

There is no `async` keyword. A function that uses `.await` is inferred to
return `Task[a]`; callers `.await` it in turn. Everything compiles to
generator-based fibers (`yield*`) on a scheduler in the JS kernel, giving
structured concurrency, interruption, and scopes without an `Effect` type
in user code.

```alder
fn profile(id: Id) -> Result[Profile] {
    let user = Http.get(`/users/${id}`).await?
    let posts = Http.get(`/users/${id}/posts`).await?
    Ok({ user, posts })
}

let (a, b) = Fiber.all(profile(1), profile(2)).await
```

- `Task` is a visible type. Signatures may write it
  (`fn load(id: Id) -> Task[Result[User]]`), hover shows
  it when inferred, and an un-awaited call is a `Task` value you can pass
  to `Fiber.fork`, `Fiber.all`, or `Fiber.race`.
- **Open:** how a top-level entry point runs the scheduler; the exact fiber
  API.

## Context (dependency injection)

Services are requested by type with `use` and supplied by `provide` in an
enclosing scope. Missing providers are compile errors at entry points.

```alder
fn saveUser(user: User) -> Result[()] {
    use Db
    Db.insert(users, user).await
}

fn main() {
    provide Db = Sqlite.open("app.db") {
        saveUser(u).await
    }
}
```

- Providers are resolved lexically through the call graph and, in the web
  runtime, through the render tree, so SSR gets per-request isolation.
- Tests swap providers with `provide Db = FakeDb.new() { ... }`.
- **Open (M2):** `provide … { }` is a statement in the M1 parser, so a
  block ending in it has no value. `web.md`'s `handle` hook ends its body
  with `provide Session = session { resolve(event).await }` and expects
  that to be the function's `Task[Response]`. M2 either promotes
  `provide` to an expression whose value is its body's value, or `handle`
  writes an explicit `return` / tail.

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
    let mut count = state(props.start ?? 0)
    let double = count * 2                     // memoized automatically

    <button onClick={fn() count += 1}>
        {props.label}: {count} ({double})
    </button>
}
```

- `component` bodies run once. `state(...)` bindings are signals; the
  compiler tracks reads of them in expressions and markup (Svelte 5 rune
  style) and memoizes derived values. Plain `let` bindings that do not
  read state are not reactive.
- `??` unwraps an `Option` with a default.

## Attributes and macros

Attributes use Rust syntax and are how the compiler and packages mark
items. Macros are real: Alder functions from syntax to syntax, executed at
build time in the compiler's embedded V8, with Elixir-style
`quote`/`unquote` and Jai-style `comptime` blocks.

```alder
#[derive(Show, Eq, Json)]
type Point = { x: Number, y: Number }

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
every other build.

```alder
tests {
    import @alder/test.{ fakeDb }

    test "adds numbers" {
        assert add(1, 2) == 3
    }

    test "finds a user" {
        provide Db = fakeDb() {
            assert find(1).await == Ok(ada)
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
#[extern("node:crypto", "randomUUID")]
fn randomUUID() -> String

#[extern("node:fs/promises", "readFile")]
fn readFile(path: String, encoding: String) -> Task[Result[String, [:io(String)]]]

#[extern("globalThis", "JSON.parse")]
fn parseJson(s: String) -> Result[Json, [:syntax(String)]]
```

- If the declared return type is `Result`, the kernel wraps the call in
  try/catch and tags the thrown error. Otherwise a throw is a panic.
- A JS function returning a promise must be declared `Task[...]`.
- Plain-data JS objects are typed as records and used directly at zero
  cost. Class instances are opaque types declared with
  `#[extern] type Response` and accessed through extern functions.
- No automatic consumption of `.d.ts` files.

## Open questions (collected)

- Convention or attribute for package-internal modules.
- Entry-point scheduler and the fiber API surface.
- Macro hygiene and the compile-time API surface.
- Enum and record runtime representation (see `runtime.md`).
- Whether `provide … { }` is a statement or an expression (M2).
