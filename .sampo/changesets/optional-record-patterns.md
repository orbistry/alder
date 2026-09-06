---
cargo/alder-solve: patch
cargo/alder-codegen: patch
---

Read stored Option fields directly in record destructuring and record-shaped
enum patterns, preserving nested Option values without presence-based wrapping.
