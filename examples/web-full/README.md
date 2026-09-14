# Full M6 web example

This application exercises the implemented web compiler/runtime with Alder
source only: nested layouts, dynamic typed route params and server loads,
request-local context, shared reactive stores, remote queries/commands and
resources, typed page actions and text forms, JSON endpoints, typed error
boundaries, and static/dynamic prerender entries. It uses no custom JavaScript
bootstrap, user extern helpers, database, or M8 schema language.

From the repository root:

```sh
cargo run -p alder-cli -- dev examples/web-full
cargo run -p alder-cli -- build examples/web-full
cargo run -p alder-cli -- run examples/web-full/dist/server.mjs -- --port 3000
```

Standalone is the default target. To use the local Cloudflare runtime, stop the
dev server, install the pinned platform support dependencies once, and switch
the example's configuration:

```sh
npm ci --prefix crates/alder-cli/support
cp examples/web-full/alder.cloudflare.jsonc examples/web-full/alder.jsonc
cargo run -p alder-cli -- dev examples/web-full
```

Restore the default with
`cp examples/web-full/alder.standalone.jsonc examples/web-full/alder.jsonc`.
Both targets run the same source. Cloudflare development uses local Miniflare,
not production services; this example needs no account credentials or bindings.
Deployment is separate and requires an explicitly chosen Worker/account.
For production artifacts, HMR behavior, browser checks, and exact deploy
commands, see [web development](../../docs/web-development.md).

## Things to try

- `/`: increment the count and watch both the root layout and page update from
  the same browser store. Navigate between pages; the browser application
  retains its store. The initial hydrated count is 1 because the server hook
  increments the request-local store before its snapshot reaches the browser.
- The home resource is server-rendered, hydrated, and refreshed by a real remote
  query. Run the command: its response reports request count 2, while the next
  fresh query reports 1. The command mutates request-local demonstration state,
  not persistent storage.
- `/users/ada` and `/users/grace`: nested layout, generated PageData/Params,
  request context supplied by the server hook, and a working text form. Submit
  one character to see the typed action validation error; submit a longer name
  to see the hook-provided viewer in the successful action result.
- `/users/missing`: the load returns `:missing(String)`. The typed error boundary
  handles the expected error; it does not receive a partial PageData record.
- `/health`: a `#[derive(Json)]` Health enum response includes `requestCount: 1`.
  The JSON shape is `{ "tag": "Health", "value": { ... } }`. Repeated fresh
  requests each see 1; server stores are isolated rather than shared across
  visitors.
- `/about`, `/users/ada`, and `/users/grace` declare prerendering. The dynamic
  user page supplies its two explicit entries.

Useful manual checks while development is running:

```sh
curl http://127.0.0.1:3000/health
curl http://127.0.0.1:3000/health
curl -i http://127.0.0.1:3000/users/missing
```

The shared server hook supplies a fixed demonstration viewer, not authentication.
Form reads preserve repeated text fields; the example accepts exactly one name
and validates it on the server. File uploads and schema-generated Form/Field
components are intentionally outside this example's scope.
