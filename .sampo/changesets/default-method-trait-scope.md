---
cargo/alder-solve: patch
cargo/alder-cli: patch
---

Preserve all trait parameters in default-method annotation scopes, including
parameters omitted from a method signature. Reject specialization through local
annotations and retain the trait's superclass evidence in nested bodies.
