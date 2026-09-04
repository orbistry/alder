---
cargo/alder-solve: patch
---

Check optional record-field assignments against their stored payload type,
while preserving Option-valued reads and rejecting traversal through absent
optional parents.
