# Alder Web Framework

**Status: current direction, everything provisional.**

Alder ships a full metaframework: routing, SSR/SSG/CSR, components with
fine-grained reactivity, stores, typed styles, forms, and JSON APIs.
JavaScript is required in the browser; there is no progressive
enhancement mode.

## Routing

SvelteKit's model, copied deliberately. The folder is the route; the file
name says what the file is.

```
src/routes/
├── +layout.ald                # layout component + universal load for the subtree
├── +layout.server.ald         # server-only load for the subtree
├── +page.ald                  # /            page component + universal load
├── +error.ald                 # error boundary for the subtree
├── users/
│   ├── +page.ald              # /users
│   ├── +page.server.ald       # server load + actions scoped to /users
│   └── [id]/
│       ├── +page.ald          # /users/:id
│       ├── +page.server.ald
│       └── +server.ald        # HTTP handlers for /users/:id
├── api/health/+server.ald     # endpoint with no page
├── lib/
│   └── users.remote.ald       # remote functions callable from anywhere
└── hooks.server.ald           # app-wide server hooks (auth, request setup)
```

- `+page.ald` exports a `page` component and may export a universal
  `load` that runs on the server for the first render and in the browser
  on navigation.
- `+page.server.ald` exports a server-only `load` and `actions`, both
  scoped to that page. Anything here may `use Db`; nothing here ships to
  the browser.
- `+layout.ald` and `+layout.server.ald` are the same pair for a subtree;
  the layout component renders `children`. Layout `load` data is available
  to every page beneath it.
- `+server.ald` exports `get`, `post`, ... returning typed responses. A
  route may return pure JSON this way with no page at all.
- `+error.ald` renders when a `load` or page in the subtree fails.
- `[id]` params are typed from the folder name. The compiler generates a
  typed `Routes` module so `href(Routes.users.show, { id })` is checked
  and links to unknown routes fail at compile time.
- API-only packages can use a code-defined router builder (hono-like)
  with typed path params parsed from the string literal. Both systems
  share handler and middleware types.

## Server hooks

`src/hooks.server.ald` holds app-wide server hooks, as in SvelteKit. This
is where authentication, request-scoped context, and error reporting are
handled centrally instead of in every `load`.

```alder
// src/hooks.server.ald
pub fn handle(event: RequestEvent, resolve: fn(RequestEvent) -> Task[Response]) -> Task[Response] {
    let session = Auth.fromCookie(event.cookies).await
    provide Session = session {
        resolve(event).await
    }
}

pub fn handleError(err: Error, event: RequestEvent) -> ErrorResponse { ... }
pub fn handleFetch(event: RequestEvent, request: Request, fetch: Fetch) -> Task[Response] { ... }
```

- `handle` wraps every request: pages, endpoints, remote functions, and
  form actions. Values provided here (`provide Session = ...`) are
  available through `use Session` in every `load`, action, and remote
  function for that request, which replaces SvelteKit's untyped
  `event.locals` with typed context.
- `handleError` centralizes unexpected-error reporting; expected errors
  stay `Result` values and never reach it.
- `handleFetch` intercepts server-side `fetch` calls made during `load`.
- `src/hooks.client.ald` mirrors this for the browser (`handleError`,
  `init`).
- **Open:** a `sequence` helper for composing several `handle` hooks, and
  per-subtree hooks (SvelteKit does not have them either).
- **Open (M2):** `provide … { }` is a statement in the M1 parser, so the
  `handle` body above has no value. M2 either promotes `provide` to an
  expression whose value is its body's value, or this example writes an
  explicit `return` / tail (see `language.md`, Context).

## Page options

Exactly SvelteKit's, exported as values from `+page.ald`,
`+page.server.ald`, `+layout.ald`, or `+layout.server.ald`, and inherited
down the tree. There is no `alder.jsonc` default; the root `+layout.ald`
is where app-wide choices go.

```alder
pub let prerender = true      // build-time render (SSG); default false
pub let ssr = false           // skip server render for this subtree; default true
pub let csr = false           // ship no JS for this subtree; default true
pub let trailingSlash = Never // Never | Always | Ignore
```

- `prerender = true` on a dynamic route requires `entries` to enumerate
  params, as in SvelteKit.
- `ssr = false` makes the page render only in the browser; `csr = false`
  makes it static HTML. Both false is a compile error.

## Loading data

```alder
// users/[id]/+page.server.ald
pub fn load(event: LoadEvent) -> Result[{ user: User, posts: Array[Post] }] {
    use Db
    let user = db.run(query { select * from users where users.id == ^event.params.id }).await?
    let posts = loadPosts(user.id).await?
    Ok({ user, posts })
}

// users/[id]/+page.ald
pub component page(props: { data: PageData }) {
    <h1>{props.data.user.name}</h1>
}
```

- `PageData` for a route is generated from the return types of its own
  `load` functions merged with every parent layout's, so `props.data` is
  fully typed with no annotation.
- `event.params` is typed from the folder names on the way down.
- Errors from `load` are open `:tag` errors; `+error.ald` matches on them.

## Remote functions

SvelteKit's remote functions, which are the same idea as server functions
in Solid and TanStack. Any module named `*.remote.ald` is server-only:
every `pub` function in it can be called from anywhere, including
components, and the compiler replaces the call with a typed stub over
HTTP when the caller runs in the browser. The `Result` type crosses the
wire intact.

```alder
// lib/users.remote.ald
pub fn getUser(id: Id) -> Result[User] { ... }              // query
pub fn deleteUser(id: Id) -> Result[()] { ... }             // command
pub fn signUp(input: SignUp) -> Result[User] { ... }        // form action, typed by schema

// any component
component UserCard(props: { id: Id }) {
    let user = resource(fn() getUser(props.id))
    <button onClick={fn() deleteUser(props.id)}>Delete</button>
}
```

- Queries and commands are just functions; the framework caches queries
  by arguments and invalidates them when a command in the same module
  runs, following SvelteKit's `query`/`command` semantics. **Open:** the
  exact cache and invalidation API.
- A remote function whose argument is a `schema` type is usable as a
  `Form` action.
- Remote modules and `+page.server.ald` are the only server-only
  boundaries; there is no per-function attribute. The compiler performs
  whole-program reachability from each entry point and rejects
  server-only stdlib (Db, Kv) in client code with a path explaining how it
  got there. Components are isomorphic by default.

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

// lib/auth.remote.ald
pub fn signUp(input: SignUp) -> Result[User] { ... }

<Form action={signUp}>
    <Field name="email" />
    <Field name="password" type="password" />
</Form>
```

- `SignUp` is a record type plus a parser. Form components are typed from
  it; the remote function (or a `+page.server.ald` action) receives the
  parsed value; errors map back to fields.
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
        @for (task, i) in tasks; key task {
            <text inverse={i == selected}>{task}</text>
        }
    </box>
}
```

- Input events, raw mode, and rendering come from the Rust side of
  deno_core.
- **Open:** element set, focus model, and whether TUIs can also render to
  the web for previews.
