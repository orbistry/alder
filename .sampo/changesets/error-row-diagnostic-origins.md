---
cargo/alder-solve: patch
cargo/alder-driver: patch
---

Report deferred error-row constraint failures at the local function reference,
including calls through imported interfaces, instead of reusing definition
coordinates against the caller's source.
