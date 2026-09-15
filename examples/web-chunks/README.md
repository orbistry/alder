# Route chunk isolation fixture

The light route has no dependency on the catalog. The catalog route imports a
synthetic 2,048-record lookup table (over 150 KB of literal JavaScript source)
and selects records interactively, preventing dead-code elimination of the data.
The deterministic data is deliberately checked in rather than generated at runtime.

Build with `alder build examples/web-chunks`, then serve the generated standalone
artifact with `alder run examples/web-chunks/dist/server.mjs -- --port 3002`.

In Chrome, inspect the network while loading `/` and then selecting **Large
catalog**. Identify the catalog chunk through `dist/manifest.json`: it must be
absent on the light route and requested on catalog navigation. **Next record**
checks that the downloaded module executes. A direct `/catalog` load must include
its dependencies in the document's modulepreload links.

For a terminal record of actual Chrome requests, serve the artifact on port 3004,
run `node tools/trace-web-requests.mjs 3004 3005`, and open
`http://127.0.0.1:3005/` in a fresh Chrome tab. The local-only proxy preserves
response bodies and cache headers. It logs requests, not response bodies.

To check bounded bootstrap recovery, append the exact manifest entry URL to the
proxy command, for example `node tools/trace-web-requests.mjs 3004 3005
/_alder/entry-HASH.mjs`. This deliberately returns 503 for that asset only. A fresh
document should retry once, then show **Reload page**. Restart the proxy without
the blocked path and use that button to recover. Use a fresh local port if the
entry is already cached by Chrome.
