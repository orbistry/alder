# Alder Runtime and Targets

**Status: current direction, everything provisional.**

## Targets

A package declares its target in `alder.jsonc`:

```jsonc
{ "type": "application", "target": "cloudflare" } // or "server" | "browser" | "tui"
```

- One standard library with target-gated modules (like Rust `cfg`).
  Importing `Cloudflare.Kv` in a `tui` package is a compile error.
- Library packages are target-neutral unless they say otherwise; the
  compiler checks that only target-neutral code is reachable from them.
- Multiple entry points (a worker, a migration CLI, a TUI admin) are
  multiple workspace members, each with its own target. Workspaces already
  exist in `alder-config`.
- Web packages additionally split server and client code within one
  package. See `web.md`.

## JavaScript output

- One JS module per Alder module, bundled with rolldown (as a Rust library
  inside the compiler) for a Vite-like experience with no separate tool.
- `Option<a>` is `a | null`. Nested options box the inner value.
- `Number` is a JS number, `BigInt` a JS bigint, `Array` a JS array,
  records are plain objects, enums are tagged objects.
  **Open:** the exact enum representation (`{ $: "Some", 0: x }` vs
  arrays vs classes) and whether records use prototypes for trait dispatch.
- Trait dispatch is dictionary passing resolved at compile time where
  the type is known. **Open:** representation for HKT dictionaries.

## Kernel

The runtime is a hand-written JS/TS kernel shipped by the compiler,
exposed to the Alder stdlib through `extern` (Elm's kernel model). It
contains:

- The fiber scheduler: generator-based (`yield*`), structured concurrency,
  interruption, scopes, and the `Task` runner.
- The signal graph used by components and stores.
- The SSR renderer and hydration.
- Context (`provide`/`use`) propagation across fibers and render trees.

Everything above the kernel is written in Alder.

## Embedded runtime

The `alder` binary embeds `deno_core` (V8):

- `alder run` executes `server` and `tui` targets with no external
  runtime installed.
- Macros and `comptime` blocks execute in the same embedded V8 at build
  time.
- TUI I/O is provided from Rust (terminal raw mode, events, layout).
- Binary size (~100MB) is accepted.
- **Open:** whether `server` targets deploy on deno_core in a container or
  on an external runtime; Node compatibility surface needed by npm FFI
  packages.

## Cloudflare

Cloudflare concepts are ordinary types implementing traits, marked with
attributes. The grammar stays generic; the `@alder/cloudflare` package
interprets them and the compiler emits `wrangler.jsonc` and bindings.

```alder
#[durable_object]
type Counter = { count: Number }

impl DurableObject<Counter> {
    fn fetch(obj: Counter, req: Request) -> Response { ... }
}

fn handler(req: Request) -> Response {
    use Kv                      // bound to the worker's KV namespace via wrangler config
    Kv.get(cache, "key").await
}
```

- Bindings (KV, D1, R2, Queues, Hyperdrive, Workflows) are available
  through context (`use Kv`), provided by the generated entry point.
- Development runs on a vendored miniflare shipped as compiler support
  files, never by delegating to `wrangler dev` or Vite. `server` and `tui`
  targets use deno_core with HMR.

## Deployment

`alder deploy` owns the whole path:

- Generates `wrangler.jsonc` from the package and its attributes.
- Runs pending D1/Hyperdrive migrations as part of deploy.
- Builds container images for `server` targets.
- **Open:** how secrets and environments (`preview`, `production`) are
  modeled in `alder.jsonc`.
