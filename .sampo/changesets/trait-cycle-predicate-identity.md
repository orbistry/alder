---
cargo/alder-solve: patch
cargo/alder-driver: patch
---

Detect trait resolution cycles using complete predicate identities instead of rendered type names, allowing nested-record equality and preserving the actual missing-payload diagnostic.
