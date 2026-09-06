---
cargo/alder-solve: patch
---

Preserve error-row tails when checking trait implementation overlap, rejecting
open-row instances that could match the same concrete Result constructor.
