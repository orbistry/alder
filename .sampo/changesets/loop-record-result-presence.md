---
cargo/alder-solve: patch
---

Preserve optional record fields across reachable loop exits independently of
break order, preventing absent fields from acquiring required payload types.
