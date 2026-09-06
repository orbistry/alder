---
cargo/alder-kernel: patch
cargo/alder-cli: patch
---

Wait for owned child cleanup in scope, all, and race before delivering cancellation
to the caller, including cancellation during winner/failure cleanup.
