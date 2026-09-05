---
cargo/alder-codegen: patch
cargo/alder-solve: patch
---

Preserve template interpolation evaluation and string-conversion order across
later setup statements. Check tagged templates against the actual tag function,
including argument types and its return type, instead of assuming String.
Preserve empty tagged-template segments around adjacent interpolations.
