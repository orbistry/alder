---
cargo/alder-kernel: patch
---

Prevent all/race from stranding owned child fibers when a later task factory
fails, and wait for partial-child cleanup before propagating that failure.
