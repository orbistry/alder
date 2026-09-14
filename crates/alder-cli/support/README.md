# Alder local Workers support

Official compiler releases include the pinned production dependencies and
platform-native workerd in this support tree. Projects do not run `npm install`
and do not need a `package.json`. Node.js **22 or newer** must be on `PATH` for
Workers development, prerendering, and deployment; Node itself is not bundled.

For a source checkout only, install dependencies with `npm ci` in this directory.
Release CI uses `npm ci --omit=dev --bin-links=false` separately on every native
release platform. Never copy `node_modules` between operating systems or CPU
architectures. See [release packaging](../../../docs/release-packaging.md) for
the archive, installer, and version-cache contracts. The Rust CLI
starts `node cloudflare-dev.mjs --root /absolute/project --host 127.0.0.1 --port 3000`.
The bridge uses the installed Miniflare 5 alpha's `convertV4MiniflareOptions`
adapter; it does not pass legacy option objects directly to the v5 constructor.

Input is newline-delimited JSON:

- `{"type":"build","server":"compiled ESM source","config":{...}}`
- `{"type":"render","server":"compiled prerender Worker","config":{...}}`
- `{"type":"error","message":"compiler diagnostic"}`
- `{"type":"shutdown"}`

Stdout contains only newline JSON `listening` (with `url`), `ready` (with
`revision`), and `error` (with `message`). Logs and Worker console output go to
stderr. No compiler source is evaluated by Node; workerd executes it.

`render` uses the same bindings and reload path, then dispatches a local request
to `http://alder-prerender/` and emits `{"type":"rendered","result":...}` with
the compiler-generated Worker's JSON result. The usual `ready` message precedes
it. The parent sends `shutdown` after consuming the result; no production URL is
contacted.

The stable listener serves `/_alder/events` using the native dev host's SSE
protocol (`kind: ready/build/error`, plus `revision`). It retains the last eight
revision-specific client modules, pins generated HTML to its matching revision,
and marks development responses no-store. Other HTTP requests stream to the
current Worker; HTML is annotated with its served revision before delivery.
Compile errors retain the last good build. Failed Worker evaluation attempts
restore its last good options before subsequent requests are dispatched.

KV, R2, D1, SQLite Durable Objects, Queues, and Workflows use explicit generated
Wrangler bindings. Local state persists beneath the project's
`.alder/miniflare` and survives successful rebuilds and host restarts. This
directory contains development data only; the bridge never accesses production
resources or provisions anything. Assets resolve relative to `project/dist` and
are enabled when the configured directory exists; the Worker can serve its
inline client bundle before a disk asset build exists.

The bridge has been smoke-tested against its pinned Miniflare package with real
KV rebuild persistence, SQLite Durable Object persistence, R2 writes/reads, D1
SQL, queue delivery, Workflow checkpoint results, SSE notifications, and failed
Worker evaluation rollback followed by recovery. These are local runtime checks,
not evidence of a production deployment.

Hyperdrive requires an explicit `localConnectionString` or
`CLOUDFLARE_HYPERDRIVE_LOCAL_CONNECTION_STRING_BINDING`. An HTTP service binding
to another Worker requires `ALDER_SERVICE_LOCAL_URL_BINDING` containing its
local HTTP(S) origin. Self-service bindings work directly. Cross-Worker Durable
Objects, cross-Worker Workflows, and external RPC entrypoints require a separate
multi-Worker setup and fail clearly here. Local secrets are not inferred from
deployment credentials; explicit config `vars` are supported.

Relevant primary references:
[Miniflare](https://developers.cloudflare.com/workers/testing/miniflare/) and
the installed `miniflare/dist/src/index.d.ts` (V4 option converter and v5 API).
