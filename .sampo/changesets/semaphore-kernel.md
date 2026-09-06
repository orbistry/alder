---
cargo/alder-kernel: minor
---

Add lazy fixed-capacity semaphores with FIFO weighted requests, cancellation-safe
permit ownership, and scoped cleanup before handing permits to the next waiter.
