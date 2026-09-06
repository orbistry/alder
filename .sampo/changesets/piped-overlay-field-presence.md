---
cargo/alder-solve: patch
---

Resolve newly closed record overlays after bare-function pipe calls before
checking field access, preserving the final stored field type like ordinary calls.
