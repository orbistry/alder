---
cargo/alder-can: minor
cargo/alder-ast: minor
cargo/alder-solve: minor
cargo/alder-codegen: minor
cargo/alder-cli: minor
cargo/alder-kernel: minor
cargo/alder-driver: minor
---

Treat optional record-field shorthand as an ordinary Option type and materialize omitted Option fields as None during contextual record construction.

Remove separate record-field optionality from serialized interfaces and invalidate the previous interface format.

Read Fiber traversal concurrency options using ordinary Option semantics so omitted and explicit None fields both select the sequential default.

Preserve known generic Result error rows during propagation, including explicit Option fallback branches.

Provide conditional Option ordering and Unit ordering so derived record payloads use ordinary field dictionaries, including nested Options.
