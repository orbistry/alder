# Cloudflare web adapters

Cloudflare metadata is extracted from public concrete type declarations after
semantic compilation. Resource IDs and exported class names are explicit; builds
do not create resources or infer destructive migrations.

For standalone/Cloudflare build and local development commands, see `web.md`
and `examples/web-full/README.md`. This document describes the typed platform
facade, not a claim that resources or a deployed account have been provisioned.

```alder
import cloudflare.{Kv, DurableObjectNamespace, DurableObject, DurableObjectState}
import http
import http.{Request, Response}

#[binding("CACHE", "kv", "0123456789abcdef0123456789abcdef")]
pub type Cache = Kv

#[binding("COUNTERS", "durable_object", "Counter")]
pub type Counters = DurableObjectNamespace

#[durable_object("Counter")]
pub enum Counter { Counter(Number) }

impl DurableObject[Counter] {
    async fn initObject(storage: DurableObjectState) Counter { Counter::Counter(0) }
    async fn fetch(object: Counter, request: Request) Response { http.json(42) }
}
```

Platform implementations use owned nominal types, normally enums. Structural
record aliases do not confer ownership for the compiler's orphan rules. The
namespace binding type is separate from the object's per-instance state type.
Adapters import the solved singleton dictionary and preserve its Json evidence;
generic dictionary factories are rejected.

`#[queue("queue-name")]` marks a public type implementing
`Queue[Consumer, Message]`, where Message implements Json. Its methods are
`initConsumer() Consumer` and `consume(Consumer, Array[Message]) Task[()]`.
The host decodes the whole batch and acknowledges it on successful completion.
Decode errors and Task defects reject the batch for the configured platform
retry policy. Partial acknowledgement and custom per-message retry are not
exposed by this initial facade.

`#[workflow("ExportedClass")]` marks a public type implementing
`Workflow[State, Payload, Output]`, with Json evidence for Payload and Output.
`initWorkflow() State` creates its state and
`run(State, WorkflowEvent[Payload], WorkflowStep) Task[Output]` handles execution.
`cloudflare.step(step, name, callback)` checkpoints Json-encoded results and
restores request-local providers when the platform invokes the checkpoint
callback. The event exposes payload and instanceId.

Binding attributes are `#[binding("ENV", "kind", ...)]` on aliases of the
corresponding opaque cloudflare type:

| Kind | Type | Explicit arguments after kind |
| --- | --- | --- |
| kv | Kv | namespace ID |
| r2 | R2 | bucket name |
| d1 | D1 | database name, database UUID |
| hyperdrive | Hyperdrive | configuration ID |
| service | Service | Worker name |
| durable_object | DurableObjectNamespace | exported local class |
| queue | QueueBinding | queue name |
| workflow | WorkflowBinding | workflow name, exported local class |

Aliases preserve their qualified provider identity, allowing distinct resources
with the same underlying handle type. Binding values are installed inside the
request's Task fiber, never in process-global state. `ASSETS` is reserved for the
generated static-assets binding. Missing environment bindings fail explicitly.
This uses the existing provider context; static service/layer construction and
transitive dependency-availability proofs remain deferred to the DI plan.
KV reads/writes and Json-backed Durable Object storage reads/writes are currently
exposed by std/cloudflare. Other handles provide typed binding identity; their
service-specific data APIs are not implemented by this module.

`DurableObjectState` and `WorkflowStep` implement `Eq` by native handle identity,
not by comparing their storage or checkpoint contents. This allows an owned
enum such as `Counter(DurableObjectState)` to retain the state supplied to
`initObject` and use it for storage operations in later `fetch` calls. Derived
equality for the wrapper preserves that identity; these handles do not gain
serialization or ordering capabilities.

The pure `alder_driver::cloudflare` API exposes `extract`, `extract_build`, and
`wrangler_config`. Generated configuration includes nodejs_compat, observability,
static assets, queue consumers/producers, and the resource bindings above.
The Worker is already bundled by Alder, so generated config explicitly sets
`no_bundle: true` and `find_additional_modules: false`. Wrangler must upload only
the entry module, not recursively discover browser files, prerender output, or
previous `dist/deploy-check` artifacts beside it. Browser/public files remain
part of the separate static-assets binding. Runtime-provided imports such as
`cloudflare:workers` are resolved by Workers, not uploaded as local modules.
New SQLite Durable Objects use the current declarative `exports` configuration.
Callers supporting an existing deployment may supply an explicit legacy
migration history instead; no history is synthesized or deleted. Account IDs
are optional for local config generation and must be supplied by the deployment
workflow. Configuration generation itself performs no deployment.

The current platform shape was checked against Wrangler 4.131.2 and Workers
types 5.20260914.1, including the declarative Durable Object exports schema.
