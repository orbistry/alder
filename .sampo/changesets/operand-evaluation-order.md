---
cargo/alder-codegen: patch
---

Preserve left-to-right evaluation of array and tuple elements, call arguments,
and tag payloads when later operands require setup statements. Evaluate earlier
values before those statements without copying referenced mutable containers.
