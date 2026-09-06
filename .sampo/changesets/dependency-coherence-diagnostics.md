---
cargo/alder-solve: patch
cargo/alder-driver: minor
cargo/alder-cli: patch
---

Report invalid stored dependency trait indexes once at build level, preserving
canonical module identities without attributing errors to unrelated source.
Reject incoherent registries even when the build contains no source modules.
