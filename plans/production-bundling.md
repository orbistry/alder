# Production bundling hardening (before M7)

## Scope and inspected baseline

Implement the complete production browser artifact/loading pipeline while keeping
the existing standalone and Worker execution models. No commit, push, or external
deployment is authorized for this pass. All automated Rust tests use direct APIs.

The baseline at `0ba08ad` uses pinned Rust Rolldown `0.1.0`, not the current JS
API. `alder-bundle::bundle_entry` returns the first emitted chunk's code and drops
the rest. Web route descriptors statically import all pages/layouts/errors.
`client.mjs` is embedded in the server and referenced by all documents. Both dev
bridges keep revision-specific copies of that URL for HMR. Production prerender
builds the client twice; staged publication replaces the managed client tree.

## Design

### Artifacts and optimization

- Introduce an owned bundle result containing every file and chunk metadata:
  explicit entry/facade identity, imports, dynamic imports, module membership,
  exports, source-map relationships, and asset provenance. Sort by filename.
- Retain a checked single-file adapter for execution-only callers. It must reject
  unexpected extra chunks/assets rather than silently discard output. Server,
  Worker, and prerender execution stay single-file; inline dynamic imports there.
- Production browser output uses ESM, minification, automatic shared chunks, and
  content-hashed names. Do not use property mangling or arbitrary vendor groups.
  Development stays eager, unminified, and revision-addressed for existing HMR.
- Source maps are explicitly selectable and generated against emitted JavaScript,
  not falsely described as original `.ald` source maps. Hidden maps live outside
  the public client directory by default; publication must be an explicit policy.
- Report raw, gzip, and Brotli sizes per chunk, with entry/shared/route membership.
  Preserve a baseline measurement of the current full example before changes.

### Lazy routes and manifests

- Production browser route descriptors retain only routing metadata and lazy
  module loaders. Pages, universal loads, layouts, and error components load for
  the selected route. Common modules remain shared through Rolldown's graph and
  native ESM module caching; singleton stores/runtime must not be copied.
- Client hooks may remain eager because they govern startup and global errors.
  Error components load when needed, including initial SSR error hydration.
- Route loading precedes universal-load execution and rendering. Check navigation
  generation/abort state after every asynchronous boundary; stale imports may
  finish but must not mount a stale route or alter history. Preserve the last
  good page until the replacement is ready.
- A deterministic manifest records emitted entry, all files/dependencies, route
  module mappings, sizes, and a build identity. Build identity must not create a
  circular content hash or unnecessarily invalidate independent route chunks.
- SSR and prerender use that same manifest to reference the hashed bootstrap and
  preload its static closure plus only the active view's required module closure.
  Navigation payloads carry the destination route's normal/error closures; the
  browser selects and preloads the applicable graph after build-version checking
  and before lazy imports, deduplicating existing modulepreload links. Route data
  is resolved independently of browser module fetching.

### Serving, versioning, and recovery

- Standalone serves the complete emitted asset set. Cloudflare serves only known
  generated paths through ASSETS; unknown/stale chunk paths return a real 404,
  never fallback HTML or another build's entry.
- Hashed assets: immutable long-lived cache headers. HTML and navigation payloads:
  no-store/revalidation policy consistent with existing dynamic responses. Keep
  public non-hashed assets on a conservative separate policy.
- Include build identity in SSR/navigation data and verify it before hydration
  or navigation rendering. An old runtime must never consume new-build payloads.
- Recover from a build mismatch/missing lazy chunk using a bounded full document
  navigation with session-scoped retry protection. Persistent failure must show
  actionable UI, preserve usable existing content when possible, and never loop.
  Network failures must not be silently swallowed or mistaken for typed errors.
- Staged builds still remove obsolete generated files. Old-session compatibility
  relies on explicit version checks and bounded recovery, not indefinite retention
  of stale output. Test two consecutive builds and stale browser sessions.

## Acceptance ledger

Baseline measured from a fresh `alder build examples/web-full` at `0ba08ad`:
browser `client.mjs` 88,294 bytes raw / 20,555 gzip / 18,093 Brotli;
standalone `server.mjs` 213,245 / 46,512 / 32,675 bytes respectively.
Compression used Node zlib gzip level 9 and Brotli quality 11. These are artifact
sizes, not measured latency or browser execution times.

- [x] Structured output preserves every chunk/asset/map and explicitly resolves entries.
  Evidence: `split_output_preserves_graph_and_explicit_entry`, binary asset
  provenance, hidden-map tests, and the direct CLI publication regression.
- [x] Production minification executes correctly; dev output/HMR remain usable.
  Evidence: minified V8 execution preserves exports, serialization and shared
  identity; actual production forms/remotes and both development targets pass.
- [x] Lazy route/page/layout/error/load boundaries and shared singleton identities pass.
  Evidence: lazy startup/navigation loader regressions, nested error/layout tests,
  shared-count browser workflows, and large-route network traces on both targets.
- [x] Deterministic hashes/manifests and independent-route cache stability pass.
  Evidence: reversed-input/repeated-build bundler tests, direct compiler manifest
  regression, and unchanged light/layout/shared hashes in the two-build browser check.
- [x] SSR/prerender preloads and references resolve to the correct active graph.
  Evidence: direct compiler/prerender manifest test, normal/error preload runtime
  tests, and actual Chrome dependency requests match the selected graph.
- [x] Standalone/Cloudflare serving, MIME, HEAD, cache headers, and stale URLs pass.
  Evidence: application asset regression and local workerd byte-for-byte checks;
  stale build chunk removal/404 verified against the running standalone artifact.
- [x] Navigation races, failures, initial hydration, and bounded deployment recovery pass.
  Evidence: direct lazy hydration node-identity and superseded-import regressions,
  mismatched-build/storage-denial tests, and real two-build/bootstrap-failure Chrome
  checks. Node identity is asserted in direct runtime tests, not inferred from AX.
- [x] Source-map policy, bundle size reporting, and before/after measurements documented.
  Evidence: generated-source lookup regression, hidden-map publication checks,
  and final measurements in `docs/production-builds.md`, including server growth.
- [x] Full example and large isolated-dependency fixture verified in Chrome on both
  local production targets, including network loading proof and existing workflows.
  Evidence: the target-specific Chrome workflow and request-log sections below.
- [x] Development/HMR regression checks pass on both local targets.
  Evidence: temporary full-example heading updates retained count 2 in Chrome
  on both standalone and Cloudflare development servers, detailed below.
- [x] Rust fmt, strict Clippy, workspace tests, Alder fmt, and packaging/build checks pass.
  Evidence: final post-identity workspace validation and fresh package exit 0,
  with existing platform-specific installer skips and macOS linker warning noted.
- [x] Docs, SPEC, changeset, configuration/defaults, limitations, and commands updated.
  Evidence: `docs/production-builds.md`, example READMEs, SPEC checkpoint, and
  `.sampo/changesets/production-bundling.md` covering the four changed crates.

Evidence belongs beside each checked item. A passing build or minify-only change
does not satisfy the goal. Deferred CSS language features and original-Alder
source-map generation are not implicitly claimed by this work.

### Incremental verification: navigation preloading

The direct kernel regression
`navigation_preloads_selected_static_graph_before_imports_without_duplicates`
checks that navigation emits the selected static closure before invoking the
lazy loader, repeated visits do not duplicate links, and error navigation selects
its error dependencies. The deployment-mismatch regression rejects new-build
payloads before touching preload links. The application-response regression
checks that navigation data carries the server manifest's destination graph.

After this change, `cargo test --quiet` passed across the workspace (including
149 kernel tests), strict workspace Clippy passed, `cargo fmt --all` completed,
and `git diff --check` passed. This is unit/integration evidence, not browser
network evidence; the browser, adapter, HMR, and remaining release gates above
are still unchecked.

### Incremental verification: large-dependency Chrome fixture

`examples/web-chunks` builds a light route and a catalog route with a deterministic
2,048-record JavaScript dependency used by an interactive lookup. In the tested
standalone artifact, the catalog chunk was `chunk-DHFPi5A-.mjs`: 159,244 bytes raw,
110,618 gzip, 101,844 Brotli. The source is literal data so that this tests download
isolation, not runtime generation of a small dependency.

Chrome tab 1491424756 loaded the production artifact through the local-only
`tools/trace-web-requests.mjs` proxy (port 3005, upstream 3004). The request log for
the initial `/` document contained exactly its four manifest JavaScript URLs:
`chunk-BTnxbAs7.mjs`, `chunk-DLiX1m_W.mjs`, `chunk-HaFzKlHd.mjs`, and
`entry-DSTy8ebZ.mjs`. The catalog chunk was absent. Clicking **Large catalog**
requested `/catalog` navigation data and `chunk-DHFPi5A-.mjs`. Clicking **Next
record** changed record 0 to record 1. Revisiting the light route and catalog
requested navigation data only, with no repeated chunk download. Chrome reported
no warning/error logs. The favicon's unrelated 404 was visible in the proxy log.
The fixture's four Alder files pass `alder fmt --check`.

This proves standalone browser isolation and interactive lazy execution for the
fixture; it does not yet prove local Cloudflare or the full application's browser
acceptance criteria.

A fresh full-example build after navigation preloading measured these active
normal-route closures (sum of independently compressed files):

| Route | Raw | gzip | Brotli |
| --- | ---: | ---: | ---: |
| `/` | 44,681 | 15,408 | 13,757 |
| `/about` | 42,054 | 14,622 | 13,026 |
| `/users/[id]` | 44,305 | 15,549 | 13,898 |

The standalone server measured 254,567 raw / 42,361 gzip / 36,646 Brotli bytes.
Compared with the recorded baseline, browser initial bytes decrease, but server
raw and Brotli bytes increase. These are interim artifact measurements, not a
latency claim; the embedding policy and final measurements still need review.

### Incremental verification: full-example standalone Chrome workflows

Chrome tab 1491424759 exercised the production full example on port 3006:

- The home document bootstrapped with shared/page counts of 1 and its remote
  greeting rendered. Incrementing changed both views to 2.
- The remote command produced `Welcome back, Alder` and command request count 2;
  its refreshed query still reported a fresh server request count of 1.
- Navigating to Ada retained browser count 2 and rendered the nested layout,
  hook-provided viewer, fresh server count 1, and typed form.
- Submitting `Bundle check` returned `Alder visitor submitted: Bundle check`.
- Navigating to Grace retained browser count 2 and reset the form result to
  `No form has been submitted.` (The example's initial input value is `Ada`.)
- Navigating to the missing user rendered the typed loader error boundary with
  the root layout and shared count 2 intact. Chrome warning/error logs were empty.

These checks establish successful standalone interactive workflows after the
split bootstrap. They do not establish DOM node identity during hydration,
direct deep-link loading, or browser history: the attempted page-level history
shortcut did not navigate, and native browser control acquisition stalled.
Those browser gates remain open, along with local Cloudflare and dev/HMR checks.

### Incremental verification: publication and release-support gates

The direct CLI regression
`split_outputs_publish_all_bytes_and_remove_old_chunks_maps_and_manifest` passes:
entry/shared bytes and a nested binary asset are published intact, hidden maps
stay outside the public tree, and rebuilding without the production graph removes
the previous chunks, maps, and manifest. Existing unrelated-dist preservation
and failed-build preservation tests remain in the suite.

- `node --test .github/scripts/prepare-alder-support.test.mjs`: 2 passed.
- `python3 -m unittest discover -s .github/scripts -p '*_test.py'`: 8 tests,
  6 passed, 2 skipped (PowerShell unavailable and optional live cargo-dist
  installer output not supplied). These skipped environments are not claimed.
- `python3 tools/generate-html-schema.py --check`: pinned schema matches.
- `alder fmt examples --check`: 18 correct after formatting the existing
  `examples/web/src/routes/+page.ald` button; formatting-only change.
- `alder fmt tests/e2e --check`: 63 correct.
- `cargo fmt --all -- --check` and strict workspace Clippy: passed after the new
  publication test.
- `cargo test --quiet`: full workspace passed again, including 17 CLI tests and
  149 kernel tests (the two existing ignored doctests remain ignored).

Fresh-directory `cargo package --workspace --allow-dirty --target-dir
/tmp/alder-package-verify.a60gwo` completed with exit 0, including verification of
the CLI. It recovered from registry transport warnings. macOS's debug linker
warned that `__eh_frame` exceeded the compact-unwind table limit; this was not a
package failure and is not a browser build warning. The new publication test was
added while verification was running; executable production sources were unchanged
after packaging started. This is a local host package check, not a claim that
Windows/Linux release matrices or optional live installer verification ran.

### Incremental verification: local Cloudflare production artifact

Copied the full example's source and Cloudflare configuration into
`/tmp/alder-cloudflare-check.hDZAn9`, built it, and ran the existing
`tools/serve-built-worker.mjs` helper on port 3007. This uses local Miniflare/workerd
and the real ASSETS binding; no deployment was performed. The browser chunk names
matched the standalone build. The binding-based serving policy was checked
against [Cloudflare's asset binding documentation](https://developers.cloudflare.com/workers/static-assets/binding/).

A local HTTP verification compared all 10 manifest assets byte-for-byte with
`dist/client`, checked immutable cache headers and empty HEAD bodies, and checked
404/no-store for stale chunks, maps, and the obsolete fixed entry URL. Documents
for `/`, `/about`, `/users/ada`, and `/users/grace` returned 200/no-store and
referenced the manifest entry with modulepreloads.

Chrome tab 1491424772 opened `/users/ada` directly. The typed form returned
`Alder visitor submitted: Worker check`. Navigation to home loaded the remote
greeting; incrementing updated both shared count views to 2, and the command
returned `Welcome back, Alder` with request count 2. Browser back restored Ada
while retaining shared count 2. Although the browser action's observation timed
out, reconnecting to the same tab confirmed the resulting Ada URL and rendered
state. The missing-user link rendered the typed boundary with count 2 intact.
Warning/error logs were empty. The tab is preserved for the unfinished browser
verification workflow.

Local Cloudflare verification of the large-dependency fixture, dev/HMR checks,
and cross-deployment browser recovery remain separate outstanding gates.

### Incremental verification: large dependency on local Cloudflare

Built a temporary Cloudflare copy of `examples/web-chunks` at
`/tmp/alder-chunks-worker.biAsiO`, served its production artifact with
`tools/serve-built-worker.mjs` on port 3008, and traced Chrome requests through
`tools/trace-web-requests.mjs 3008 3009`. The initial light route fetched only
`chunk-BTnxbAs7.mjs`, `chunk-DLiX1m_W.mjs`, `chunk-HaFzKlHd.mjs`, and
`entry-yDwS1ewY.mjs`. It did not request the catalog's 159,244-byte
`chunk-CxUfXMuq.mjs`. Navigating to `/catalog` requested that chunk; **Next record**
changed record 0 to record 1. Chrome tab 1491424772 reported no warning/error logs.
This establishes actual browser download isolation and execution through the
local Worker asset binding, in addition to the standalone evidence above.

### Incremental verification: development/HMR on both local targets

Ran `alder dev /tmp/alder-cloudflare-check.hDZAn9 --port 3010` and
`alder dev /tmp/alder-standalone-hmr.YzNQUO --port 3011`, each from a temporary copy
of the full example with its respective target configuration. In Chrome tab
1491424772, each application bootstrapped its greeting and shared counter, then
incremented both counter views from 1 to 2. Editing the temporary home heading
triggered recompilation and changed the visible heading without a manual reload;
both counter views remained 2 on each target. Warning/error logs were empty.
Both temporary source edits were restored. Checked-in example source was not
modified for these HMR checks. This verifies the state-preserving development
path remains operational alongside the separate production splitting policy.

### Incremental verification: real browser deployment and failure recovery

Built the standalone chunk fixture in `/tmp/alder-recovery-check.UbOHEK`, served
it on port 3012, and opened its light route through the request tracer on 3013 in
Chrome tab 1491424772. While that old session remained open, changed only the
temporary catalog heading to `Large catalog — build two`, rebuilt, and replaced
the local server process on the same port. The unrelated light/layout/shared
chunk names stayed unchanged. The old catalog chunk disappeared from disk and
returned HTTP 404/no-store from the replacement server.

Clicking the old session's catalog link produced one data request and one full
document request for `/catalog`, then fetched the new entry and catalog chunk.
Chrome rendered the build-two heading. The expected console diagnostic was
`Alder application was updated; loading the current build`. No repeated document
requests appeared in the captured recovery sequence.

The request tracer now optionally accepts exact `/_alder/` asset paths to inject
local 503/no-store failures. Through a fresh origin on port 3014, blocked the
production entry and directly loaded `/catalog`. The log showed exactly two
document requests and two failed entry requests (initial attempt plus one
automatic retry). Chrome preserved the SSR content and displayed the actionable
`Reload page` notice instead of looping. Removed the fault by restarting only the
local tracer without blocked paths; clicking **Reload page** removed the notice,
and **Next record** changed record 0 to record 1. This exercises the actual
minified document loader and recovery UI, not only the runtime shim tests.

### Audit strengthening: useful source-map positions

`minified_source_map_resolves_expression_to_generated_source_line` verifies an
actual optimized expression's source-map lookup, resolves it to the generated
JavaScript's throwing line (not merely line zero), and checks that the preserved
map metadata equals the emitted map bytes. It passes. This supplements map-file
existence/hidden-publication tests without claiming original-Alder mappings.

### Audit correction: malformed public output

Two new regressions exposed boundary defects: the checked single-file adapter
panicked if publicly constructible chunk metadata named a missing file, and
artifact validation accepted Windows drive paths. The adapter now returns
`InvalidOutput` for missing entry bytes, and filename validation rejects colons
on all platforms. Tests also cover traversal, absolute paths, duplicate names,
NUL, query/fragment separators, and backslashes. All 13 bundler tests and strict
workspace Clippy pass after the corrections. Final workspace/package validation
must include these changes before completion is claimed.

### Audit correction: server-only JavaScript build identity

The fresh package run at `/tmp/alder-final-package.R8Q5Su` passed, including the
malformed-output fixes (same macOS debug-linker unwind warning). A subsequent
direct compiler regression exposed a further versioning gap: changing a
server-only `#[extern]` JavaScript dependency changed the executable but not the
manifest build ID. Identity now includes the resolved server bundle before
client metadata is added, avoiding both omitted native dependencies and a
circular hash. The regression also asserts browser output stays unchanged.
Deployment/validation sign-off is reopened pending final checks of this change.

### Final validation after identity correction

The complete workspace test suite passes with the server-only JavaScript
regression (18 CLI tests, 13 bundler tests, 149 kernel tests), and strict workspace
Clippy and Rust/Alder formatting checks pass. A fresh full-example build retains
the same browser entry and chunk bytes as the Chrome-tested build; its browser
route-size totals are unchanged. The new server build identity changes only the
reported compressed server totals to 42,398 gzip / 36,687 Brotli, with raw size
unchanged at 254,567. The user-facing production-build documentation records the
final values and reproducible local checks.

Fresh package verification at `/tmp/alder-identity-package.PVIGCj` completed with
exit 0 after the identity correction. The macOS debug linker again reported its
compact-unwind size warning; packaging succeeded. Support selection tests passed
(2), installer/checksum tests passed (6, with the same 2 documented skips), the
pinned HTML schema matched, and the tracer passed Node syntax checking.

The acceptance ledger above is the final status; earlier incremental sections
retain the chronology of issues and gates that were still open at those points.
All ten objective areas have implementation and verification evidence. Limits
remain explicit: maps target generated JavaScript, compression reports are not
latency measurements, standalone embedding increases raw/Brotli server size,
reload recovery can discard unsaved state, and no external deployment or
cross-platform release-matrix execution is claimed. No commit or push was made.
