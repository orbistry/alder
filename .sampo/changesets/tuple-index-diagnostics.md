---
cargo/alder-constrain: patch
cargo/alder-solve: patch
cargo/alder-driver: patch
cargo/alder-cli: patch
---

Report out-of-range tuple reads and writes at the index with the tuple length
and valid zero-based bounds instead of a misleading unit-type mismatch.
