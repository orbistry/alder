# Pipes

This example chains array transformations with Alder's `|>` operator and
arrow-form anonymous functions, including concise and fully annotated forms.
A pipe into a call forwards to its first argument by default; `_` can select a
different argument position.

From the repository root:

```sh
cargo run -p alder-cli -- run examples/pipes
```

A successful run prints:

```text
Pipe showcase passed!
```
