---
cargo/alder-ast: minor
cargo/alder-can: patch
cargo/alder-solve: patch
cargo/alder-driver: patch
---

Carry error-row inclusion relationships in canonical and serialized annotations,
preserving them through arena copies and interface hydration. Bump the interface
format for the new scheme layout.

Retain inclusion dependencies during solver generalization and instantiation,
and reject inferred inclusions between independent universal error-row tails.

Resolve concrete error unions for inferred results, including exhaustive matches
across module boundaries. Preserve explicitly open result contracts and apply
directional Result return checking consistently to named functions and lambdas.
