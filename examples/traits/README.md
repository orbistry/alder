# Traits

This example shows Alder's trait system in one runnable program:

- a generic trait and a prerequisite-bearing implementation;
- a higher-kinded `Functor` implementation;
- derived `Show`, `Ord`, `Hash`, and `Json` implementations; and
- explicit reference identity with `Ref.same`.

From the repository root:

```sh
cargo run -p alder-cli -- run examples/traits
```

Every feature is checked with an assertion. A successful run prints:

```text
Trait showcase passed!
```

To produce the bundled ESM artifact instead:

```sh
target/debug/alder build examples/traits
```

The bundle is written to `examples/traits/dist/main.mjs`.
