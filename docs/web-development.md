# Develop, build, and run an Alder web application

M6 implementation and verification are tracked in
[the acceptance ledger](../plans/m6-acceptance.md). A real Cloudflare preview
deployment remains a separate gate requiring an exact account/Worker and
authorization; local Miniflare success is not a deployment.

## Smallest application

Create `app/alder.jsonc`:

```json
{ "type": "application", "target": "standalone" }
```

Create `app/src/routes/+page.ald`:

```alder
pub component page() {
    let count = state(0)
    <button onClick={() -> { count = count + 1 }}>Count: {count}</button>
}
```

Then run:

```sh
alder dev app --port 3000
```

Open the printed localhost URL. The server renders the button, the generated
browser entry hydrates its existing nodes, and clicks update its text. Edit the
component and save. No `main`, HTML document, JavaScript bootstrap, npm project,
or application-side bundler configuration is needed. `alder init` is not yet an
implemented command; create the two files directly.

## Full example from a source checkout

From the repository root:

```sh
cargo build -p alder-cli
target/debug/alder dev examples/web-full --port 3000
```

The [example guide](../examples/web-full/README.md) covers layouts, typed loads,
resources, remote commands, forms, hooks, errors, and prerender entries. Its
fixed viewer is a context demonstration, not authentication; request stores
are not a database. Visit `/health` repeatedly to see fresh request-local state.

Compatible HMR snapshots live writable cells before derived values are rebuilt.
Module stores retain compatible values. Changing component/state signatures
resets local state and prints the reason; it does not promise to preserve
arbitrary closures. A failed compile leaves the current page and handlers alive
and displays source diagnostics. Fixing the source resumes updates without a
server restart. Replacement setup failure also retains the last good tree.

Routes are rediscovered after file additions/deletions. Development pins each
document to its matching client revision; reconnecting event streams catch
missed builds. The last eight browser revisions are retained. An older evicted
document, a removed active route, or an incompatible document/render mode may
require a full navigation. Runtime target changes require stopping and
restarting dev. Ctrl-C closes the watcher, listener, and local runtime.

## Standalone production artifact

Stop dev, then:

```sh
target/debug/alder build examples/web-full
target/debug/alder run examples/web-full/dist/server.mjs -- --port 3000
```

This runs the built artifact directly in Alder's embedded V8/deno_http runtime;
it does not recompile source and needs no external Node application runtime.
`dist/server.mjs` contains the application and client/public assets.
`dist/client/` also contains the emitted browser bundle, public files, prerender
HTML, and navigation data. Prerender entries run during the build; their console
output is separate from the structured build-result channel.

## Cloudflare development and build

Cloudflare commands need user-provided Node.js 22+ and explicitly installed tooling:

```sh
target/debug/alder cloudflare setup
cp examples/web-full/alder.cloudflare.jsonc examples/web-full/alder.jsonc
target/debug/alder dev examples/web-full --port 3000
```

Stop dev before switching targets. The same Alder sources run in a directly
controlled local Miniflare/workerd instance; neither `wrangler dev` nor Vite is
used. Local binding state persists under the project's `.alder` directory.
Source checkouts and binary releases use the same shared tooling cache; release
archives do not contain an npm tree. See [release packaging](release-packaging.md).

Build the Worker and validate deployment packaging without uploading:

```sh
target/debug/alder build examples/web-full
target/debug/alder deploy examples/web-full --dry-run
```

Output is `dist/worker.mjs`, `dist/client/`, and `dist/wrangler.jsonc`.
Prerendering runs in the local target runtime, including declared local binding
providers. Production data is not fetched from an inferred account. The
source-checkout helper below serves the binding-free full example's built
artifact directly in Miniflare for a production-browser check:

```sh
node tools/serve-built-worker.mjs examples/web-full 3000
```

Restore standalone configuration afterward:

```sh
cp examples/web-full/alder.standalone.jsonc examples/web-full/alder.jsonc
```

Public assets and stale-output/collision behavior are documented in
[web.md](web.md#public-assets-and-build-output). Binding types, queue/workflow
adapters and explicit Durable Object migration semantics are documented in
[cloudflare-web.md](cloudflare-web.md).

## Real deployment

After choosing and authorizing an exact preview destination and configuring
Wrangler authentication, the command is:

```sh
alder deploy app --name EXACT_PREVIEW_WORKER --account-id EXACT_ACCOUNT_ID
```

These are required explicit inputs for a real deployment, not names inferred
from a project directory. Authentication stays in Wrangler's environment/auth
store. Existing remote variables/secrets are preserved with `--keep-vars`;
credentials are not copied into browser modules or hydration stores. Bindings
must identify existing/provisioned resources. Existing Durable Object migration
history can be passed with `--migrations PATH_TO_JSON_ARRAY`; no destructive
history is synthesized. Database schema migrations and standalone image
packaging belong to later work.

## Explicit browser checks

These opt-in checks use an installed Chrome and the source checkout's
`playwright-core`. They are not run by Cargo and do not use a signed-in browser
profile:

```sh
node tools/web-full-browser-check.mjs http://127.0.0.1:3000
```

This checks SSR node reuse, no initial resource refetch, shared stores, repeated
refresh, commands, navigation, typed form results, and error boundaries. For a
disposable copy of the example running under dev:

```sh
node tools/web-hmr-browser-check.mjs http://127.0.0.1:3000
node tools/web-reconnect-browser-check.mjs http://127.0.0.1:3000
```

Follow each script's printed source-edit prompts. They verify stateful edits,
compile-error recovery, route addition/removal, history, document/client races,
and missed-build recovery. These scripts never edit source files themselves.
Direct deterministic compiler/kernel/DOM/routing/runtime tests remain the
ordinary automated acceptance path.
