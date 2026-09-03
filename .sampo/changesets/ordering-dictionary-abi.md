---
cargo/alder-can: patch
cargo/alder-codegen: patch
cargo/alder-kernel: patch
cargo/alder-solve: patch
---

Implement the documented `Ord.compare -> Ordering` dictionary ABI. Generic
comparison operators now inspect the tagged result, derived ordering composes
selected field dictionaries through `compare`, and primitive comparisons keep
their direct JavaScript lowering.
