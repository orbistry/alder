---
cargo/alder-kernel: patch
---

Execute nested task awaits with explicit fiber-local continuation frames,
preserving lazy reuse and stack-safe success, failure, and interruption cleanup.
