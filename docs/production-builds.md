# Production web builds

Design decisions and verification evidence are recorded in
[production-bundling.md](../plans/production-bundling.md).

`alder build` selects production optimization for routed web applications.
`alder dev` keeps eager, readable bundles and revision-addressed HMR. No manually
configured vendor chunks are required. Server and Worker execution stay in a
single ESM artifact; only the production browser graph is split.

## Output contract

- `dist/client/_alder/entry-<hash>.mjs` starts the browser application.
- `dist/client/_alder/chunk-<hash>.mjs` contains shared or lazy route modules.
- `dist/manifest.json` records build identity, files, imports/dynamic imports,
  module membership, route preloads, map names, and raw/gzip/Brotli sizes.
- `dist/maps/client/` contains hidden source maps. They map optimized output to
  generated JavaScript, **not original Alder source**. They include generated
  source content, are not copied to the public client tree, and have no public
  `sourceMappingURL`. Uploading maps to a debugger/error service is a separate,
  explicit operation. Original-Alder mapping is not implemented.
- `dist/server.mjs` or `dist/worker.mjs` contains the server application.

The current web defaults are production minification, automatic splitting,
hashed names, and hidden maps. The low-level Rust bundle API exposes those
options independently; the web CLI currently applies these fixed defaults.
No unsafe property mangling is enabled. gzip level 9 and Brotli quality 11 are
used for size estimates, not precompressed files or a guarantee about CDN wire
sizes. Per-chunk compression totals include each independent compression stream.

SSR/prerender documents preload the entry's static dependency graph and the
selected page/layout graph. Error documents select the applicable error/layout
graph instead. Navigation data carries only the destination route's preload
graphs. After checking build identity, the browser preloads the selected static
closure before importing its page/layout modules; existing preload links are
reused. Error modules and their dependencies load when needed. ESM caching
preserves shared runtime and store identity.

## Cache and deployment behavior

Generated hashed assets receive `public, max-age=31536000, immutable`.
HTML and navigation data use `no-store`. Ordinary public assets use `no-cache`.
Missing generated paths receive 404/no-store, never a fallback HTML response.
Cloudflare serves known generated files through ASSETS; standalone embeds their
bytes in its executable server artifact. Source maps and the manifest are not
exposed by either server.

Documents/navigation data carry build identity; startup also verifies the entry
URL before hydration. Identity includes the resolved server graph (including
server-only JavaScript dependencies) before client metadata is embedded, plus
the browser outputs. A build mismatch or missing route chunk requests a full
document navigation, at most once for that URL until startup succeeds. If loading
still fails, the page shows a retry control instead of looping. When session
storage is unavailable, recovery does not automatically reload. Network failures
during ordinary navigation preserve the existing page and show retry feedback.
Reload recovery can discard unsaved local state, as an ordinary full navigation
would; it does not serialize arbitrary application state across releases.

Build publication stages all managed output before replacing it and removes old
generated files. The compatibility policy is version detection and bounded
recovery, not permanent retention of old chunks. Do not independently upload HTML
from one build and JavaScript from another. Deployment remains an explicit action;
this hardening pass does not authorize an external deployment.

## Measured full-example output

The baseline is the unsplit build at `0ba08ad`. Browser route totals below sum
the selected route's files, each compressed independently (gzip 9, Brotli 11).

| Browser graph | Raw bytes | gzip | Brotli |
| --- | ---: | ---: | ---: |
| Baseline, every route | 88,294 | 20,555 | 18,093 |
| Production home | 44,681 | 15,408 | 13,757 |
| Production about | 42,054 | 14,622 | 13,026 |
| Production user page | 44,305 | 15,549 | 13,898 |

The standalone server changes from 213,245 / 46,512 / 32,675 bytes to
254,567 / 42,398 / 36,687 (raw/gzip/Brotli). Its raw and Brotli size increase:
embedding the complete browser graph as byte arrays has overhead. This pass
optimizes browser loading, not all server artifact sizes, and makes no measured
latency claim. The separate catalog fixture demonstrates a 159 KB route-only
dependency absent on the light route and fetched on navigation on both targets.

## Reproducing local checks

From the repository root:

```sh
cargo build -p alder-cli
target/debug/alder build examples/web-full
target/debug/alder run examples/web-full/dist/server.mjs -- --port 3006
```

Open `http://127.0.0.1:3006/` in Chrome. Increment the shared count, run the remote
command, navigate between Ada/Grace, submit the form, and follow the typed-error
link. For dependency isolation and controlled failure injection, follow
[the chunk fixture instructions](../examples/web-chunks/README.md).

To check Cloudflare without deployment, use a temporary project copy with the
full example's `alder.cloudflare.jsonc` as `alder.jsonc`, build that copy, then run
`node tools/serve-built-worker.mjs /absolute/path/to/copy 3007`. The repository's
installed CLI support dependencies provide local Miniflare/workerd. Do not run
`alder deploy` for this check. For each target, run `alder dev` on a separate port,
increment the count, edit a heading, and verify the update retains state.

Automated regression gates are `cargo test`,
`cargo clippy --all-targets --all-features -- -D warnings`,
`cargo fmt --all -- --check`, and Alder format checks on `examples` and `tests/e2e`.
Release-package verification uses `cargo package --workspace --allow-dirty
--target-dir /absolute/fresh/temporary/directory`; a fresh target prevents stale
unreleased dependency artifacts from masking package failures.
