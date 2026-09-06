---
cargo/alder-codegen: patch
cargo/alder-kernel: patch
cargo/alder-cli: patch
---

Track active nominal comparison pairs in derived equality so recursive values
can compare cyclic payloads without repeatedly following the same cycle.
Preserve payload dictionary checks, NaN inequality, and cleanup after failures.
