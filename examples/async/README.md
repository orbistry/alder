# Async and fibers

This example exercises Alder's generator-backed task runtime through the real
CLI. It automatically lifts a declared Promise-returning extern, awaits the
last stage of a multiline pipe directly, composes `.await?` with a typed
`Result` boundary, runs tasks concurrently, forks and joins a fiber, races two
tasks, and verifies that the losing fiber's finalizer ran.

From the repository root:

```sh
cargo run -p alder-cli -- run examples/async
```

A successful run prints:

```text
Async fibers are alive!
```
