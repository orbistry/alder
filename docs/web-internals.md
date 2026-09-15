# Component execution contracts

## Full-M6 application integration

The expanded application path uses separate generated Oxc entry modules for
server and browser. Server companions, endpoints, and server hooks are absent
from the browser entry. This is not a substitute for transitive boundary
checking: the driver must also reject prohibited client dependency paths.
The server entry exports a standard `{ fetch(request, env, context) }` object;
runtime adapters supply HTTP serving and assets, not application rendering.
The browser entry owns hydration, navigation, disposal, and development updates.

Hydration and HTTP data use a versioned tagged graph envelope (`version`,
`root`, `nodes`), not executable JavaScript. Distinct tags represent records,
arrays, Map, Set, Date, BigInt, undefined, special numbers, and boxed Options.
Ordinary enum records cannot collide with protocol tags. Reference indices
preserve aliases and cycles. Record fields are sorted for deterministic output;
script delimiters and Unicode line separators are escaped. Decoding validates
all graph entries and defines record fields without invoking prototype setters.
Functions, symbols, accessors, and non-data host objects fail explicitly.
Containers must be ordinary dense arrays, Maps, Sets, Dates, or unmodified Option
boxes; custom container properties/subclasses and hidden record fields are
rejected rather than executing getters or silently dropping data. Encoding and
decoding share limits of 100,000 graph nodes, depth 128, and 1,000,000 traversal
visits (including the decoder's validation of unreachable entries). HTTP byte
limits are an additional application-layer responsibility.
The full example is separately exercised in a real browser on both runtime
targets; direct codec tests alone are not browser verification.

The component contracts below cover composition, directives, resources, stores,
SSR/hydration, and hot-state transfer. Application integration is described in
the subsequent sections. Unsupported executable constructs must produce a
source-region diagnostic, never an undefined value or an inert descriptor.

## Checking

Literal markup text is normalized once by the parser according to
[markup-whitespace.md](markup-whitespace.md). Codegen forwards that text
unchanged to DOM and SSR renderers. Hydration checks the same normalized text;
runtime string expressions are not normalized. `<pre>`/`<textarea>` preserve
literal whitespace. SSR's textarea newline sentinel compensates for HTML's
initial-newline stripping; pre text anchors prevent that stripping naturally.

Component parameters require explicit annotations and use ordinary Alder record
and function-argument checking. A component name is a callable value, including
uppercase names and imported names. `Counter(props)` creates an `Html` value;
it does not run setup yet. Import `Html` explicitly from `html` when annotating
it (`import html.{Html}`); there is no new global HTML namespace.
The checked-in HTML schema validates ordinary elements, attributes, content
models, and typed browser events. Handlers may be synchronous or return Task;
zero-argument handlers remain supported. Text holes accept scalar text values
and Html composition. Component tags pass a typed record of props and optional
Html children; props are live read-only inputs, including destructured fields.
Inline script/style content requires the asset pipeline and is rejected by the
component emitter. Content checks prevent browser parser repairs for supported
markup, including phrasing/block and interactive-element nesting rules.
Unknown tags/attributes, invalid nesting, attribute types, and handlers report
the corresponding source region.

The WHATWG input is pinned as `tools/html-schema-source.html.gz`; its SHA-256
appears in the generated table. `python3 tools/generate-html-schema.py --check`
verifies offline reproduction. The current vocabulary includes 115 elements and
31 global attributes. Literal descendants are checked through fragments and
directives: nested anchors/buttons/forms, interactive descendants of anchors or
buttons, and descendant tabindex are rejected. Arbitrary Html values and opaque
component bodies are not whole-program content-model proofs; conditional
WHATWG rules beyond the implemented structural checks remain out of scope.

`onInput` uses InputEvent only on textarea and statically known text-like input
controls; checkbox/select/dynamic input types use the common Event fields.
Native DOM target objects are intentionally not exposed as untyped records;
the supported form-value workflow uses `html.submit`/`html.formValues`.

## State and dependencies

Each rendering of a component value creates a new owner. Its body runs once per mount,
server render, or hydration. Direct component-body `let x = state(value)`
bindings allocate writable cells. Plain direct-body lets reading those cells
are memoized derived cells; dependencies are obtained from canonical local
references, not runtime getter tracking. Reads lower to cell values and writes
notify subscribers synchronously. Equal writes use `Object.is` and do not notify.
Memo work settles before deduplicated DOM effects run, including diamond-shaped
dependencies. Derived bindings, props, and non-state setup bindings are read-only.
Nested mutation through cell aggregate fields is rejected: replace the whole
state value to notify subscribers. Event closures capture live cells. Root setup
supports let bindings followed by final markup or an Html value. Reactive
destructuring setup lets and polymorphic component dictionaries are explicitly
rejected. Callers should pass reactive inputs explicitly to external helpers;
opaque JavaScript callbacks do not carry hidden-capture/effect metadata.
Named callback lets may capture live state, including Task handlers. Constructing
their closures is a derived computation; callback bodies execute only when
called, not during setup or hydration.

Module `let value = state(initial)` declarations allocate lazy scoped stores,
not process-global values. Initializers run once per request scope on the server
and once per browser singleton. Eager module initializers cannot read stores,
including through statically resolved helpers. Compiler dependency closure
follows direct/transitive local and imported helpers, including private store
captures and writes. Solved interfaces persist these capture identities; changes
affect interface fingerprints and invalidate dependent caches. Compiler-only JS
exports make private store handles available for subscriptions without making
them public Alder bindings. Store declarations require a simple named binding
and a concrete type.
Remote function interfaces deliberately omit server-store capture dependencies:
their callers depend on resource results, not private server cells. Server
implementations retain scoped store access; client stubs never export those cells.
The compiler also collects exact store keys reachable from browser roots, stopping
at remote replacements. Only this allowlist is serialized into HTML/navigation
hydration payloads: initializing a private store in a load or remote function never
exposes it. Shared stores used only by event handlers remain eligible for transfer.
An absent allowlist transfers no stores. This filters automatic store transfer;
explicitly returning sensitive values from a load or remote remains application code.

## Rendering and ownership

Compiled markup is a render program, not a virtual DOM. It issues element,
attribute, text, and event operations against a renderer. Each dynamic text or
attribute has its own dependency subscription. Derived computations subscribe
before rendering so updates see current values. Disposal is idempotent: detach
listeners and subscriptions, deactivate writes, and release owned nodes when
unmounting. Instances never share local cells or subscriptions; browser module
stores are intentionally shared. Server rendering
disposes its owner before returning, including on errors. A component `Html`
value can be rendered repeatedly; each render gets independent state. Props
are read-only inputs, not deep-cloned or automatically observed host objects.
`@if` and `@match` own anchored branch regions. A retained match arm updates its
payload cells without rerunning setup; changing arms disposes the prior owner.
Keyed `@for` moves existing DOM ranges, updates retained item cells, rejects
duplicate keys, and disposes removed rows. Empty-list branches have their own
owners. Callback props update the current listener target without reattaching
the DOM listener; removed branches and rows retain no live listeners or edges.

## SSR and hydration

SSR escapes `&`, `<`, and `>` in text and also quotes in attributes. Boolean
attributes are present only when true. Handlers are never serialized. Text
boundaries use comments so adjacent/empty text holes remain identifiable after
HTML parsing. Empty text nodes omitted by the HTML parser are not allocated
during hydration; the first nonempty update inserts a text node between its
existing anchors. There is no serialized executable state: the client receives the
same explicit props and reconstructs local initial state deterministically.
Serialized module-store and resource values are restored before component
setup. Void tags omit closing tags; textarea/title share an RCDATA text node;
textarea serialization accounts for the browser's leading-newline stripping.
Template children render and hydrate in `template.content`.

Hydration walks the existing nodes, checks tag/text/attribute structure, and
attaches listeners/subscriptions without replacing nodes. A mismatch throws;
there is no silent repair or fallback remount. Failed hydration releases all
partially attached listeners/subscriptions without deleting existing DOM.
Caller-provided props and initial state must agree with SSR. Initial mounting
uses the same render operations with newly created nodes. Renderer unit tests
use a deterministic DOM shim; application/browser validation is a separate gate.

`resource(() -> Task[Result[a,e]])` must initialize a direct owned component or
directive let. The value is `Resource[a,e]` (`Loading`, `Ready(a)`, `Failed(e)`).
Its statically supplied dependencies refresh it; `html.refresh(binding)` and
`html.cancel(binding)` operate on the live binding. Refresh interrupts the old
fiber and suppresses stale completion. Disposal interrupts outstanding loads.
Async SSR waits for resources before serializing each component's render program,
without rerunning setup. Typed Err remains Failed; defects fail SSR instead of
being rendered as user errors. Aborted SSR waits for cancellation finalizers.
Synchronous SSR rejects resources explicitly. Hydration requires the matching
resource payload and reuses it without an initial refetch.

Owners capture the full provider context, not only their store scope. Nested
setup, deferred SSR reads, resource fibers, memos, and browser events run in that
context. Concurrent async SSR requests therefore retain their own providers and
store values across suspension. Query execution records module dependencies in
the resource fiber context. SSR transfers these dependencies with resource values;
commands/actions can invalidate matching live resources after hydration without
first issuing a browser query. Explicit resource refresh and invalidation mark
query calls to bypass caches; initial and dependency-driven starts may reuse
cached query results. Cancellation and disposal remove registrations.

HMR snapshots include only live writable component cells, component signatures,
and stable keyed-region identities. Final inferred state types participate in
signatures, including empty collection element types. Restoration happens before
memos run; signature/type-shape mismatches fall back atomically to fresh local
state while retaining valid resource payloads. Function-containing local state
is explicitly incompatible. Disposed regions cannot resurrect earlier snapshot
state when later recreated. Browser stores transfer separately through the tagged
graph codec and concrete type signatures; navigation primes previously unseen
stores without overwriting existing singleton values.

## Entry points and verification

`import html` exposes `html.renderToString(view)` and the Task-returning
`html.renderToStringAsync(view)` to Alder. Browser hosts use the
embedded kernel's `$webMount(view, target)` and `$webHydrate(view, target)`;
both return disposal, snapshot, and restoration-status operations. These are
host integration entry points. The target is an element with an
`ownerDocument`. Mount appends owned nodes; hydration consumes the entire
target's child sequence. Disposal removes owned nodes. Failed mount removes
only newly created nodes; failed hydration preserves the supplied tree.

Implementation locations:

- `alder-solve/src/inference.rs`: bounded HTML schema and type checking.
- `alder-codegen/src/oxc_backend/web.rs`: canonical dependencies and Oxc lowering.
- `alder-kernel/kernel/src/web.ts`: owner, signals, SSR, DOM, and hydration.
- `alder-driver/src/compile/tests/web.rs`: real parse → canonicalize → solve →
  codegen → Rolldown → V8 regressions, with source-diagnostic snapshots.
- `alder-kernel/src/web_tests.rs`: dependency/disposal and renderer edge cases.
- `tests/support/dom-shim.js`: test-only parser for this SSR grammar and a
  deterministic DOM with identity, allocation, mutation, and listener checks.

The [counter fixture](../examples/web-counter/src/counter.ald) is shared by compiler
regressions and optional manual CLI checks. The JS test bridge only asserts
behavior of the compiled component; it does not implement a substitute counter.
No test launches the CLI, nested Cargo, browser, or platform processes.

## Routing and staged generated types

`alder-driver::web_routes` discovers normalized source paths without performing
filesystem I/O. Filesystem route IDs retain `(group)` directories; URL matching
omits them. Whole-segment `[id]`, `[[optional]]`, and `[...rest]` parameters are
supported, including static suffixes after a rest parameter. A route may have
one rest parameter; an optional parameter cannot follow it. Embedded parameters
and parameter matchers are rejected explicitly. Equivalent patterns with
different parameter names or groups report both source URIs. Precedence is
static, required parameter, optional parameter, rest, with deterministic ordering.

Layouts, errors, and public option sources retain outer-to-inner order. At each
level server loads precede universal loads, and later data fields replace earlier
fields. Matching validates UTF-8 percent escapes and rejects encoded separators,
dot segments, NULs, and empty path segments. Link generation percent-encodes each
segment, validates parameter names, and rejects ambiguous optional assignments.
The generated `~/routes` module exposes one typed link function per route and a
`routes` record with a `Routes` type. Function names are a reversible hexadecimal
encoding of the filesystem route ID (`route_2f` is `/`); the manifest exposes the
complete ID/name mapping. This deliberately avoids unstable naming heuristics.

`alder-driver::web_build` injects route-local `Params`, `LoadEvent`, and `PageData`
aliases into the parsed AST without rewriting source text. Synthetic imports are
used for checking only; they produce neither runtime module dependencies nor
unused-import warnings. `LoadEvent` includes typed `params`, ancestor/server
`data`, `http.Request` and `http.Url`. Layout params are limited to the layout's
own directory. Optional parameters are `Option[String]`; rest params are strings
containing slash-separated path segments, with an empty string for an empty rest.

A provisional pass solves loads with route components omitted. Repeated passes
resolve nested load data from solved semantic interfaces. The final pass checks
components with the complete data from their own universal load as well as
ancestor/server loads. Final build artifacts and interfaces remain atomic on
failure. Route pages export a `page` component; layouts may export a `layout`
component, and error files export `error`. Zero-argument components are allowed.
When accepting props, these components receive one record; data fields must
agree with generated data, and layout children must be `Html`.

### Universal loads during navigation

Initial requests (including `ssr = false`), prerender, and HMR resolve the full
ordered server/universal load pipeline on the server. Initial hydration consumes
that payload without invoking universal loads again. Client navigation sends
`Accept: application/x-alder-data` with `x-alder-navigation: 1`; the browser then
invokes each layout/page universal load in order, merging its result before the
next level and mounting only after the load Task completes. Its `LoadEvent`
contains the destination Request/URL, typed params, merged data, and `fetch`.

The response contains one explicit server-load return record per layout/page
level (`serverData`), with an empty record for a missing server load. Each record
is merged immediately before that level's browser universal load. **Every field
returned by a server load is public transport data**, even if a later load
overwrites it. Keep secrets in request-local providers/server state; returning a
secret and hiding it through a later overwrite is not a security boundary. The
protocol transfers neither events, bindings, providers, nor request scopes.

Server child loads already have typed access to ancestor universal results.
Preserving that contract requires trusted server evaluation of universal
ancestors before a subsequent server load, including on navigation. Universal
loads after the last server load run only in the browser for navigation; earlier
ones may run in both environments. A child server load sees its server-computed
ancestor data, while browser universal loads see browser-computed ancestors plus
the explicit server return records. No browser-computed data is sent back as
trusted input to a server load. Load functions should avoid side effects that
assume exactly one execution across both environments.

Prerendered navigation data retains the same server records, so universal loads
still execute in the browser when navigating to cached pages. HMR deliberately
continues using a complete server payload, including SSR resource values; ordinary
navigation skips server resource rendering and lets browser owners load them.
Expected browser load failures retain
their tagged payload in `PageError::Expected`; unexpected failures reach the
client reporting hook and use sanitized `Unexpected` boundary props. Superseding
navigation or disposal interrupts the load fiber, runs its finalizers, and
prevents stale DOM/history commits. Cancellation does not roll back application
side effects performed before interruption.

## Remote effect and wire contracts

A module named `users.remote.ald` is imported as `users`: `.remote` marks its
server boundary and is omitted from the module's import identity. Every public
function in that module is asynchronous on both targets. A declaration written
`pub fn getUser(id: String) Result[User]` has the public type
`fn(String) Task[Result[User]]`, as if declared `pub async fn`. Existing `async fn`
and explicitly `Task`-returning functions are not wrapped a second time. Calls
must use `.await` inside an async body or pass the Task to a resource. Assigning
an unawaited remote call to its completed result type is a compile error.

`#[command]` marks mutation/invalidation semantics; `#[query]` and an absent
annotation select query caching. These annotations do not establish server
boundaries. Query classification is the author's promise of read-only behavior;
the compiler does not infer this from function names or inspect external effects.

The driver validates solved parameter and completed-result types before emitting
a remote descriptor. Results retain their `Result` wrapper and open typed-error
rows. Supported wire types include scalar values, arrays, closed records, tuples,
Options, Results, Map/Set and shared serializable enums. Function values, open
record rows, unresolved payload type variables, caller-supplied trait dictionaries,
and opaque host objects are rejected. Public runtime values, reexports, and enum
constructors must live in ordinary shared modules; remote modules expose HTTP
functions and type aliases/error declarations.

Browser replacement modules are direct Oxc ASTs with exactly the original ESM
identity and exported function signatures. Their bodies return the Task from
`$webRemote(moduleId, functionName, args, kind)`. They contain no original remote
body or imports. Client boundary traversal stops at these replacements and
reports full source import paths for other reachable server files or server-only
standard modules. The bundler must replace original remote modules with the
driver-provided `client_replacements` before building a browser bundle.

Server bundles use `server_replacements` at the same public identities and
include `server_implementations` under private `.__server` identities. These
facades preserve auxiliary exports and wrap each public call in
`$webRemoteServer(module, kind, invoke)`. Query-resource dependency tracking and
command invalidation therefore happen when the lazy Task executes in its
request scope, including during SSR; constructing an unused Task has no effect.

Remote argument and result schemas are normalized from solved types into a
finite node graph; recursive generic shared enums reuse schema nodes. Generated
Oxc modules export separate argument/result validators. `$webValidate` returns
the original value on success and throws `TypeError` on failure. Argument tuples
have exact arity, records have exact own data fields, and optional record fields
use the canonical explicit-null ABI produced by the compiler. Validation does
not execute accessors or coerce primitive types. Boxed Options are distinguished
from ordinary same-tag enums through kernel identity. Map/Set/Date/BigInt/Unit
retain their actual runtime representations. Open error rows accept unknown
colon tags only with contiguous positional payloads containing valid wire data.
Traversal is iterative, tracks each object/schema-node pair for graph cycles,
and enforces depth (128) and visit (100,000) budgets. HTTP dispatch separately
enforces transport/body limits and maps invalid inputs to 400 and invalid server
results to 500.

Page server `actions` records are solved before their universal page. The page
receives an implicit typed `actions` value whose binding points directly at a
generated action stub module. No implementation from `+page.server` is imported
by that value. Action metadata carries the same input/result wire validators.
As with other exported shared values, action records need concrete function
types: a function with a shorthand polymorphic `Result[Output]` error row cannot
be stored in an exported shared record without fixing that row (for example
`Result[Output, [:invalid]]`).

Page options are extracted from the parsed source AST and validated after the
ordinary typechecker. `ssr`, `csr`, and `prerender` accept static Bool literals,
local constant references, and negation. `trailingSlash` is the generated
`~/routes` enum `TrailingSlash::{Never, Always, Ignore}`, also implicitly imported
in route modules. Other computations for these options are rejected, not run as
part of compilation. Server/universal layout options inherit outer-to-inner,
then page options override them; disabling both SSR and CSR is an error.

Pages can export `entries` as `Array[Params]` or a zero-argument function returning
that array (including an async function). Unannotated declarations receive a
source-preserving contextual annotation before inference, including empty
arrays. Layout entries are rejected. Static literal entries are encoded and
checked for invalid parameters and duplicate paths. The driver also supplies
the export identity, value/function/Task shape, and exact completed-value wire
schema so the build host can evaluate dynamic entries, validate them, and emit
prerendered artifacts. Generated `LoadEvent.fetch` has the same native
`fn(Request) Task[Result[Response, [:network_error(String)]]]` contract as
`http.Fetch`, allowing request-scoped fetch hooks without an untyped escape.

`+error` modules receive a specialized `PageError` alias whose expected-error
payload is the union of known tagged load failures beneath that boundary. Use
`import http.{PageError as ErrorKind}` to name the native enum constructors in
patterns (`ErrorKind::Expected` / `ErrorKind::Unexpected`); as elsewhere in
Alder, type aliases do not introduce constructor namespaces. Unexpected failures
expose only `{status, message}`. Error components cannot declare `data` props:
failed loads do not establish the successful `PageData` contract. The reserved
word `error` is accepted narrowly as a component name and record field so the
conventional `pub component error(props: {error: PageError})` API is usable.
Boundaries render inside layouts at their own directory or above, never inside
descendant layouts. A layout-load failure skips boundaries at or below that
layout, since its data contract did not complete. Component/setup failures and
failing error components retry successively outer boundaries; each retry drops
layouts below the selected boundary. SSR and client navigation share this cutoff.
Final solved hook and endpoint contracts are checked before artifacts can be
returned, including generic global request parameters and exact native Fetch
Task/Result callback contracts.
