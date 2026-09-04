---
cargo/alder-kernel: patch
---

Unwind task cleanup for malformed runtime operations and synchronous handler
defects instead of skipping cleanup or escaping the shared scheduler.
