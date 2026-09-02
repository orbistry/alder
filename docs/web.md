# Alder Web Framework

**Status: current direction, everything provisional.**

Alder ships a full metaframework: routing, SSR/SSG/CSR, components with
fine-grained reactivity, stores, typed styles, forms, and JSON APIs.
JavaScript is required in the browser; there is no progressive
enhancement mode.

## Routing

SvelteKit's model. The folder is the route; the file name says what the
file is.

```
src/routes/
├── +layout.ald            # wraps the whole subtree
├── +page.ald              # /
├── users/
│   ├── +page.ald          # /users
│   └── [id]/
│       ├── +page.ald      # /users/:id
│       └── +server.ald    # JSON handlers for /users/:id
└── api/
    └── health/+server.ald
```

- `+page.ald` exports `load` and a `page` component.
- `+layout.ald` exports `load` and a `layout` component that renders
  `children`; it controls the whole subtree (auth, data, render mode).
- `+server.ald` exports `get`, `post`, ... returning typed responses. A
  route may return pure JSON this way with no page at all.
- `[id]` params are typed from the folder name. The compiler generates a
  typed `Routes` module so `href(Routes.users.show, { id })` is checked
  and links to unknown routes fail at compile time.
- API-only packages can use a code-defined router builder (hono-like)
  with typed path params parsed from the string literal. Both systems
  share handler and middleware types.

## Render modes

Per route, with an app default in `alder.jsonc`:

```alder
#[static]           // prerendered at build time
#[server]           // SSR per request (default)
#[client]           // CSR only
```

A `+layout.ald` attribute applies to its subtree.

## Server and client code

One web package holds both. The split is per function.

```alder
#[server]
fn loadUser(id: Id) Result<User, [:not_found(Id) | r]> {
    use Db
    Db.get(users, id).await
}

pub component UserCard(props: { id: Id }) {
    let user = resource(fn() loadUser(props.id))
    ...
}
```

- `#[server]` functions run only on the worker. Calls from
  client-reachable code are replaced with typed RPC stubs that carry the
  same `Result` type.
- The compiler performs whole-program reachability from each entry point
  and rejects server-only stdlib (Db, Kv) in client code with a path
  explaining how it got there.
- Components are isomorphic by default.

## Reactivity

Fine-grained signals with compile-time dependency tracking (Svelte 5
runes style). Components run once.

- `state(x)` creates a signal bound to a `let mut`. Reads inside
  expressions and markup are tracked; derived `let` bindings that read
  state are memoized.
- Markup compiles to direct DOM operations bound to signals; `if`, `for`,
  and `match` blocks become reactive regions.
- Hydration reuses server-rendered DOM.
- **Open:** effects (`effect { ... }`), resources/async data, and
  transitions.

## Stores (out-of-tree state)

Module-level stores with plain syntax that the compiler makes
request-scoped during SSR, so state never leaks between requests.

```alder
// src/stores/cart.ald
pub let mut items = state([])
pub fn add(item: Item) { items.push(item) }
```

- In the browser this is a singleton signal graph.
- On the server each request gets its own instance (AsyncLocalStorage
  style through the fiber scheduler).
- Components subscribe by importing.

## Styles

`style` blocks are typed and compile to atomic CSS (StyleX model).

```alder
let card = style {
    padding: 16px,
    color: theme.text,
    ":hover": { color: theme.accent },
    "@media (max-width: 600px)": { padding: 8px },
}

<div class={card}>...</div>
```

- Property names and value types are checked; unknown properties and
  wrong units are compile errors.
- Merging is deterministic (last style wins per property).
- **Open:** `theme` declaration and tokens, keyframes, and dynamic values.

## Forms and validation

Storage shape and input shape are separate. A `schema` declaration
mirrors `table` syntax, can start from a table, and holds validation
rules that do not belong in the database.

```alder
schema SignUp from users {
    pick email, name
    name: min(3)
    password: String, min(12)
    confirm: String, equals(password)
}

#[server]
fn signUp(input: SignUp) Result<User, [:taken(Email) | r]> { ... }

<Form action={signUp}>
    <Field name="email" />
    <Field name="password" type="password" />
</Form>
```

- `SignUp` is a record type plus a parser. Form components are typed from
  it; server actions receive the parsed value; errors map back to fields.
- Validation errors are open `:tag` errors so custom rules compose.

## API

- `+server.ald` handlers with typed request and response bodies. The
  compiler emits a typed client for Alder frontends and `.d.ts` for
  TypeScript consumers.
- Router builder for API-only packages: `Router.new().get("/users/:id", handler)`.
- **Open:** middleware model, OpenAPI export, streaming responses.

## TUI

Terminals reuse signals, stores, and the markup grammar, but with their
own element vocabulary and layout (flexbox via Rust-side layout in the
embedded runtime).

```alder
component App() {
    let mut selected = state(0)
    <box direction="column" border="round">
        <text bold>Tasks</text>
        {for (task, i) in tasks { <text inverse={i == selected}>{task}</text> }}
    </box>
}
```

- Input events, raw mode, and rendering come from the Rust side of
  deno_core.
- **Open:** element set, focus model, and whether TUIs can also render to
  the web for previews.
