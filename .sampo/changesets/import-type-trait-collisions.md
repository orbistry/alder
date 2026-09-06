---
cargo/alder-can: patch
cargo/alder-driver: patch
---

Reject imported types and traits bound under the same name, consistently with
local declarations, including wildcard and renamed public re-exports. Preserve
distinct aliases and report both conflicting source imports.
