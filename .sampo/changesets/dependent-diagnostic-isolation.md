---
cargo/alder-driver: patch
---

Suppress cascading importer diagnostics after a source module fails, including
transitive public re-exports. Failed builds return no executable artifacts,
interfaces, or partial package indexes, including consumers of invalid sibling
implementation bodies.
