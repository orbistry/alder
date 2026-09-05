---
cargo/alder-driver: patch
cargo/alder-cli: patch
---

Reject distinct workspace roots declaring the same package name before module
discovery, with a deterministic diagnostic identifying both roots. Repeated
paths to the same physical package remain one member.
