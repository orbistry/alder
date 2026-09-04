---
cargo/alder-can: patch
cargo/alder-driver: patch
---

Reject recursive type-alias dependencies during canonicalization and report a
source-aware diagnostic explaining how to represent recursive data with enums.
Expand local and imported alias references with instantiated canonical targets,
including generic arguments and record field presence.
