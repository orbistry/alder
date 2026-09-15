# Standalone counter and SSR example

`src/counter.ald` is also compiled directly by the driver's web regression suite.
The tests exercise actual emitted/bundled code with the deterministic DOM shim;
they never invoke the CLI or nested Cargo commands.

Optional manual SSR check from this directory:

```sh
cargo run --manifest-path ../../Cargo.toml -p alder-cli -- run
```

This prints escaped SSR HTML, including stable text-boundary comments. The
automated tests additionally cover hydration node identity, event attachment,
updates, independent instances, and cleanup. This example prints SSR HTML rather
than serving a routed app; see `../web` or `../web-full` for browser examples.

Manual verification on 2026-09-14: the command above passed and printed the
counter's initial value 2 and derived value 4 inside the expected SSR markup.
