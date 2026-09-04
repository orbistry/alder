---
cargo/alder-kernel: patch
---

Preserve scheduler fairness across immediately ready Promise resumptions,
child fibers, and finalizers using a shared host-yield budget.
