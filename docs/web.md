# Alder Web Framework

The M6 implementation includes typed components, reactive directives, SSR and
hydration, filesystem routes, server loads/actions, remote functions, hooks,
request-isolated stores, prerendering, and both runtime adapters. Its full
acceptance status is tracked in [m6-acceptance.md](../plans/m6-acceptance.md);
implementation and local verification do not imply a completed live deployment.

Start with [web-development.md](web-development.md) and the source-only
[full example](../examples/web-full/README.md). No handwritten JavaScript
bootstrap is required. Framework modules use explicit imports such as
`import html` and `import http.{Request, Response}`; there are no global `Html`
or `Http` namespaces. See [web-internals.md](web-internals.md) for exact compiler,
ownership, transport, and error contracts.

Interactive pages require JavaScript; there is no progressive-enhancement form
mode. `csr = false` explicitly produces noninteractive server HTML. Typed styles,
schema-generated Form/Field components, API client generation, and TUI rendering
remain later-milestone designs, identified separately below. Static DI guarantees
remain deferred to [dependency-injection.md](dependency-injection.md).

## Routing

SvelteKit's model, copied deliberately. The folder is the route; the file
name says what the file is.

```
src/
├── hooks.server.ald           # app-wide server hooks
├── hooks.client.ald           # browser init/error hooks
├── lib/users.remote.ald       # typed remote functions
└── routes/
    ├── +layout.ald            # layout component + universal load
    ├── +layout.server.ald     # server-only load for the subtree
    ├── +page.ald              # / page component + universal load
    ├── +error.ald             # error boundary for the subtree
    ├── users/
    │   ├── +page.ald          # /users
    │   ├── +page.server.ald   # server load + actions scoped to /users
    │   └── [id]/
    │       ├── +page.ald      # /users/:id
    │       ├── +page.server.ald
    │       └── +server.ald    # HTTP handlers for /users/:id
    └── api/health/+server.ald # endpoint with no page
```

- `+page.ald` exports a `page` component and may export a universal
  `load` that resolves initial data on the server and runs in the browser on
  navigation. If a downstream server load depends on ancestor universal data,
  that ancestor is also evaluated on the server to preserve trusted inputs;
  the browser's universal result determines its final PageData.
- `+page.server.ald` exports a server-only `load` and `actions`, both
  scoped to that page. Anything here may `use Db`; nothing here ships to
  the browser.
- `+layout.ald` and `+layout.server.ald` are the same pair for a subtree;
  the layout component renders `children`. Layout `load` data is available
  to every page beneath it.
- `+server.ald` exports `get`, `post`, ... returning typed responses. A
  route may return pure JSON this way with no page at all.
- `+error.ald` renders when a `load` or page in the subtree fails.
- `[id]`, `[[optional]]`, and `[...rest]` params are typed from folder names.
  Route groups `(group)` do not add URL segments. The generated `~/routes`
  module exports typed link functions and a `Routes` record. Names reversibly
  encode the route ID (`route_2f` names `/`); links take exactly that route's
  typed parameter record. Embedded parameters and parameter matchers are
  rejected explicitly.
- A code-defined API router builder remains deferred. Filesystem endpoints
  work without pages.

## Server hooks

`src/hooks.server.ald` holds app-wide server hooks, as in SvelteKit. This
is where authentication, request-scoped context, and error reporting are
handled centrally instead of in every `load`.

```alder
// src/hooks.server.ald
import http.{RequestEvent, Response}
import ~/session.{Session}

pub async fn handle(event: RequestEvent[p], resolve: fn(RequestEvent[p]) Task[Response]) Response {
    provide Session = { user: "Alder visitor" } {
        resolve(event).await
    }
}

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

The example's `Session` is a shared record alias `{ user: String }`; its fixed
viewer is a demonstration, not authentication. Existing runtime context follows
Task fibers and render owners. This does not promise the deferred static DI
availability checks. Exact `handleError`, `handleFetch`, client-hook, and typed
error-boundary signatures are in [web-hooks.md](web-hooks.md).

## Page options

Exactly SvelteKit's, exported as values from `+page.ald`,
`+page.server.ald`, `+layout.ald`, or `+layout.server.ald`, and inherited
down the tree. There is no `alder.jsonc` default; the root `+layout.ald`
is where app-wide choices go.

```alder
pub let prerender = true      // build-time render (SSG); default false
pub let ssr = false           // skip server render for this subtree; default true
pub let csr = false           // ship no JS for this subtree; default true
pub let trailingSlash = TrailingSlash::Never // Never | Always | Ignore
```

- `prerender = true` on a dynamic route requires `entries` to enumerate
  params, as in SvelteKit.
- `ssr = false` makes the page render only in the browser; `csr = false`
  makes it static HTML. Both false is a compile error.
- Initial data still resolves on the server when SSR markup is disabled.
  Entries may be an `Array[Params]` or a zero-argument function/Task returning
  that array. Builds emit HTML and navigation data for the enumerated paths;
  unlisted paths continue through the dynamic server route. Prerendering uses
  local target providers, not live Cloudflare credentials or production data.

## Public assets and build output

Files under the project's `public/` directory are served at their exact root
paths on both targets: `public/images/logo.png` becomes `/images/logo.png`.
Public files take precedence over application routes at the same exact path;
they bypass application hooks. GET and HEAD are supported, including URLs with
query strings. Common file extensions receive their MIME type; unknown formats
use `application/octet-stream`. Binary bytes are preserved.

Builds copy these files into `dist/client/` and embed them in the server artifact
so standalone execution does not require an external asset server. Development
watches public files alongside source files. The `_alder/` namespace is reserved;
symlinks and public/prerender output collisions are rejected. Put only public
content in `public/`, never credentials or private application files.

Web builds stage and validate all artifacts before replacing compiler-owned
`dist/client/`, `server.mjs`, `worker.mjs`, and `wrangler.jsonc` entries. Rebuilding
removes stale public files, prerender HTML/data, and obsolete target artifacts.
Unrelated files directly under `dist/` are preserved. Publication uses
same-filesystem renames with rollback on failure, not a single atomic directory
swap across all entries. Development never falls back to stale build assets.

## Loading data

```alder
// users/[id]/+page.server.ald
pub fn load(event: LoadEvent) Result[{ name: String }, [:missing(String)]] {
    if event.params.id == "missing" {
        Err(:missing("That user does not exist"))
    } else {
        Ok({ name: event.params.id })
    }
}

// users/[id]/+page.ald
pub component page(props: { data: PageData }) {
    <h1>{props.data.name}</h1>
}
```

- `PageData` for a route is generated from the return types of its own
  `load` functions merged with every parent layout's, so `props.data` is
  fully typed through the generated alias.
- `event.params` is typed from the folder names on the way down.
- Expected load errors use tagged Result payloads; `+error.ald` receives a
  generated `PageError` enum alias, distinct from unexpected failures. All
  explicitly returned load data is public transport data, never a place for
  credentials or private handles.

## Remote functions

SvelteKit's remote functions, which are the same idea as server functions
in Solid and TanStack. Any module named `*.remote.ald` is server-only:
every `pub` function in it can be called from anywhere, including
components, and the compiler replaces the call with a typed stub over
HTTP when the caller runs in the browser. The `Result` type crosses the
wire intact.

Remote functions are Task-producing on both targets, including declarations
written with `fn` rather than `async fn`. Use `.await` or pass the Task to
`html.resource`. `#[command]` selects mutation/invalidation semantics;
`#[query]` and an absent annotation select queries. Function names do not infer
effects. See the working [greeting module](../examples/web-full/src/greetings.remote.ald).

- Browser query results are cached by module, function, and encoded arguments
  for at most 30 seconds, with a 256-entry bound. Navigation clears
  the page cache and prevents old in-flight queries from repopulating it.
  Commands invalidate their module's cache and live resources; actions invalidate
  all query modules. Explicit `html.refresh(resource)` bypasses query caches.
  Server calls execute directly in their request scope without cross-request
  caching. Cached transport values are decoded afresh for callers.
- Typed M6 actions and text forms use [web-actions.md](web-actions.md), not the
  deferred schema-generated Form/Field language.
- Remote modules, `+page.server.ald`, `+layout.server.ald`, `+server.ald`,
  and `hooks.server.ald` are server-only boundaries; there is no per-function
  boundary attribute. The compiler checks reachability from browser roots and
  rejects server modules and Cloudflare bindings with a path explaining how
  they became reachable. Remote imports are replaced with typed client stubs.
  Components are isomorphic by default.

## Reactivity

Fine-grained signals with compile-time dependency tracking (Svelte 5
runes style). Components run once.

- `state(x)` creates a signal bound to a `let`. Reads inside
  expressions and markup are tracked; derived `let` bindings that read
  state are memoized.
- Markup compiles to direct DOM operations bound to signals; `if`, `for`,
  and `match` blocks become reactive regions.
- Hydration reuses server-rendered DOM.
- `html.resource` owns a cancellable Task, suspends SSR, and hydrates its result
  without an initial refetch. Reactive `@if`, `@match`, and keyed `@for` own and
  dispose their regions. General effect syntax and transitions remain deferred.

## Stores (out-of-tree state)

Module-level stores with plain syntax that the compiler makes
request-scoped during SSR, so state never leaks between requests.

```alder
// src/counter.ald
pub let count: Number = state(0)
pub fn increment() { count = count + 1 }
```

- In the browser this is a singleton signal graph.
- On the server each request gets its own instance (AsyncLocalStorage
  style through the fiber scheduler).
- Components subscribe by importing.
- Replace aggregate state values rather than mutating nested fields. Compiler
  dependency metadata includes private helpers; server-only stores are excluded
  from hydration by a browser-reachable store allowlist.

## Styles (M8 design, not implemented)

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

## Schema forms and validation (M7/M8 design, not implemented)

The implemented M6 text-form/action contract is documented separately in
[web-actions.md](web-actions.md). The schema-driven API below is future work.

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
pub fn signUp(input: SignUp) Result[User] { ... }

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

- `+server.ald` handlers return native typed `http.Response` values, including
  `http.json` for Json-serializable Alder values. Automatic typed endpoint
  clients and `.d.ts` emission remain M8 work.
- Router builder for API-only packages: explicitly `import http/router`, then
  `router.new().get("/users/:id", handler)` (deferred).
- **Open:** middleware model, OpenAPI export, streaming responses.

## TUI (M10 design, not implemented)

Terminals reuse signals, stores, and the markup grammar, but with their
own element vocabulary and layout (flexbox via Rust-side layout in the
embedded runtime).

```alder
component App() {
    let selected = state(0)
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
