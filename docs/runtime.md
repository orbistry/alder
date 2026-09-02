# Alder Runtime and Targets

**Status: current direction, everything provisional.**

## Targets

A package declares its target in `alder.jsonc`:

```jsonc
{ "type": "application", "target": "cloudflare" } // or "standalone"
```

There are two targets because there are two runtimes. Everything else is
a library or a framework switch, not a target.

|                | you write `main`                  | entry generated from `src/routes/` |
| -------------- | --------------------------------- | ---------------------------------- |
| **cloudflare** | a worker with a `fetch` handler   | the web framework on Workers       |
| **standalone** | CLI, TUI, self-hosted HTTP server | the web framework self-hosted      |

- `cloudflare` runs on workerd. `standalone` runs on the embedded runtime
  inside the `alder` binary, with no platform underneath.
- A CLI is `fn main()`. A TUI is `fn main() { Tui.run(App) }`. A server is
  `fn main() { Http.serve(handler).await }`. Same target, same toolchain;
  `Tui` and `Http.serve` are modules.
- The web framework switches on when `src/routes/` exists. A purely
  client-side app is the web framework with `ssr = false` and
  `prerender = true` on the root layout, producing static files; there is
  no separate browser target.
- One standard library with target-gated modules (like Rust `cfg`). `Fs`,
  `Tui`, and raw sockets are `standalone`-only; KV, D1, and the other
  bindings are `cloudflare`-only; the web-standard surface is both.
  Importing `Cloudflare.Kv` in a `standalone` package is a compile error.
- Library packages are target-neutral unless they declare a `target`; the
  compiler checks that only target-neutral code is reachable from them.
- Multiple entry points (a worker, a migration CLI, a TUI admin) are
  multiple workspace members. Workspaces already exist in `alder-config`.
- Web packages additionally split server and client code within one
  package. See `web.md`.

## JavaScript output

- One JS module per Alder module, bundled with rolldown (as a Rust library
  inside the compiler) for a Vite-like experience with no separate tool.
- `Option[a]` is `a | null`. Nested options box the inner value.
- `Number` is a JS number, `BigInt` a JS bigint, `Array` a JS array,
  records are plain objects, and enums are tagged objects. Unit variants are
  frozen `{ $: "Name" }` singletons, tuple variants use `_0`, `_1`, … fields,
  and record variants retain their field names.
- Trait dispatch is dictionary passing resolved at compile time where
  the type is known. **Open:** representation for HKT dictionaries.

## Kernel

The runtime is a hand-written TypeScript kernel shipped by the compiler,
exposed to the Alder stdlib through `extern` (Elm's kernel model). It
currently contains the M2 value ABI, structural equality, Option and Result
helpers, collection primitives, context-stack scaffolding, and the minimal
test registry. Later milestones add:

- The fiber scheduler: generator-based (`yield*`), structured concurrency,
  interruption, scopes, and the `Task` runner.
- The signal graph used by components and stores.
- The SSR renderer and hydration.
- Context (`provide`/`use`) propagation across fibers and render trees.

Everything above the kernel is written in Alder.

## Embedded runtime

The `alder` binary embeds V8 through one exact Deno 2.8.1-compatible crate
family. The versions are intentionally pinned in lockstep:

| crate | version |
| --- | --- |
| `deno_core` | 0.402.0 |
| `deno_webidl` | 0.249.0 |
| `deno_web` | 0.280.0 |
| `deno_crypto` | 0.263.0 |
| `deno_fetch` | 0.273.0 |
| `deno_fs` | 0.159.0 |
| `deno_net` | 0.241.0 |
| `deno_http` | 0.247.0 |
| `deno_websocket` | 0.254.0 |

In this Deno family URL and console implementations live in `deno_web`, so
there are no separate `deno_url` or `deno_console` crates. Alder installs its
web globals in its own extension bootstrap and explicitly selects AWS-LC as
Rustls's process provider, avoiding feature-unification-dependent TLS startup.
`deno_node` is not embedded: Node compatibility is a non-goal.

- That surface is, by design, the same one Cloudflare Workers expose
  (`fetch`, `Request`/`Response`, `URL`, streams, `crypto.subtle`, timers,
  WebSocket). The kernel and stdlib are written once against it; `standalone`
  adds file system and raw network access on top.
- `alder run` executes `standalone` targets with no external runtime
  installed. A `standalone` build can also emit a self-contained binary (as
  `deno compile` does), and a container image is that binary; there is no
  external runtime to pick.
- Macros and `comptime` blocks execute in the same embedded V8 at build
  time.
- TUI I/O is provided from Rust (terminal raw mode, events, layout).
- CLI argument parsing is a stdlib derive, not a compiler feature:
  `#[derive(Args)]` on a record and `#[derive(Subcommand)]` on an enum,
  with doc comments as help text, optional fields as optional flags, and
  `Cli.parse()` typed by annotation (clap's derive model).
- Binary size (~100MB) is accepted.
- npm packages that need Node built-ins are out of scope for `extern`
  until wrapped by a first-party package.

The only direct generated-entry/host boundary is the frozen, non-enumerable
`globalThis.__alderHost` object (`args` and `exit` in M2). Alder modules use
stdlib/kernel functions rather than Deno ops. Standalone execution loads the
bundled ESM as the main module and drives V8's event loop to completion.

## Cloudflare

Cloudflare concepts are ordinary types implementing traits, marked with
attributes. The grammar stays generic; the `@alder/cloudflare` package
interprets them and the compiler emits `wrangler.jsonc` and bindings.

```alder
#[durable_object]
type Counter = { count: Number }

impl DurableObject[Counter] {
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
  files, never by delegating to `wrangler dev` or Vite. `standalone`
  targets use deno_core with HMR.

## Deployment

`alder deploy` owns the whole path:

- Generates `wrangler.jsonc` from the package and its attributes.
- Runs pending D1/Hyperdrive migrations as part of deploy.
- Builds container images for `standalone` targets that serve HTTP.
- **Open:** how secrets and environments (`preview`, `production`) are
  modeled in `alder.jsonc`.
