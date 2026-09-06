---
cargo/alder-kernel: patch
cargo/alder-cli: patch
---

Preserve interruption and deliver abort exactly once when a Promise extern
requests cancellation synchronously during registration, including failed or
malformed registrations.
