---
cargo/alder-constrain: minor
cargo/alder-solve: minor
cargo/alder-codegen: minor
cargo/alder-driver: patch
---

Infer contextual outer Option wrapping jointly across call arguments and emit
the resolved Some layers directly through the centralized kernel representation.
Report incompatible inference preferences with a source-aware diagnostic.
