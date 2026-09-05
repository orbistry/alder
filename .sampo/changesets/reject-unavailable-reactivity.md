---
cargo/alder-codegen: patch
---

Reject executable state expressions and component declarations until their M6
runtime semantics exist, rather than silently emitting identity expressions and
ordinary functions. Preserve provisional check-only support.
