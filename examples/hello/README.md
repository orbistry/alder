# Hello

From the repository root:

```sh
cargo run -p alder-cli -- run examples/hello
```

After building the workspace, the shorter form is:

```sh
target/debug/alder run examples/hello
```

To produce the bundled ESM artifact:

```sh
target/debug/alder build examples/hello
```

The bundle is written to `examples/hello/dist/main.mjs`.
