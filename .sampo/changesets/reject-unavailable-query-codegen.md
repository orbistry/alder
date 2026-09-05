---
cargo/alder-codegen: patch
---

Reject query expressions during executable code generation with source context
instead of emitting a runtime stub that always throws. Query execution remains
deferred to M7; check-only handling is unchanged.
