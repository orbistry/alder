---
cargo/alder-codegen: patch
cargo/alder-cli: patch
---

Check refutable patterns before binding payloads in lets, parameters, and loops.
Failed bindings now report a source-located match failure instead of exposing
values that violate their checked types, including inside lazy async tasks.
