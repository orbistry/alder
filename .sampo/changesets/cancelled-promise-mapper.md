---
cargo/alder-kernel: patch
cargo/alder-cli: patch
---

Ignore late Promise rejection mapping after interruption invalidates the waiter,
while still observing rejection and preserving exactly-once cancellation cleanup.
