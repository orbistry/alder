# Examples

All standalone programs, web applications, and runnable regression examples
live here. Compiler tests and inference benchmarks reuse these sources directly.

From the repository root:

```sh
cargo run --bin alder -- run examples/hello
cargo run --bin alder -- run examples/pipes
cargo run --bin alder -- run examples/explicit_async
cargo run --bin alder -- test examples/tests
cargo run --bin alder -- dev examples/web-full
```

`web` is a small routed app, `web-full` exercises the full web workflow,
`web-chunks` demonstrates production chunking, and `web-counter` prints a
standalone component's SSR HTML. The remaining directories cover individual
language features and retain their assertions.

Some directories are diagnostic/regression cases rather than polished tutorials:
`test_failures` intentionally exercises failing tests. Existing diagnostic cases
are preserved as-is; consolidating the directory does not change their behavior.
