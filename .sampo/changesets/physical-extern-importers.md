---
cargo/alder-codegen: patch
cargo/alder-driver: patch
cargo/alder-bundle: patch
cargo/alder-cli: patch
---

Carry physical source origins alongside generated ASTs so local JavaScript extern modules resolve beside their Alder declarations. Preserve virtual module identities after AST transfer, order bundle inputs deterministically, and verify Promise fulfillment, foreign defects, and cancellation through local wrappers.
