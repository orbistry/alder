---
cargo/alder-ast: minor
cargo/alder-can: patch
cargo/alder-solve: patch
cargo/alder-driver: minor
---

Add sparse exact-length tuple constraint metadata to annotations and stored interfaces, preserving arena copies and fingerprint identity with a new interface format. Carry imported constraints through inference and check concrete tuple lengths, constrained elements, and generic contracts.

Finalize source tuple projections without allocating by the largest index, preserve fixed tuple lengths, and reject recursive tuple-element constraints.
