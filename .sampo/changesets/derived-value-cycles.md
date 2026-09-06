---
cargo/alder-kernel: patch
cargo/alder-cli: patch
---

Detect active cycles in derived value operations: Show emits `<cycle>`, while
Hash, Ord, and JSON encoding raise explicit TypeError defects. Clear active
tracking after success or exceptions and preserve shared acyclic values.
