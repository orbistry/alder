---
cargo/alder-can: patch
cargo/alder-driver: patch
---

Canonicalize package trait headers independently of value and method bodies,
then compile every module against one frozen package-wide header closure.
Coherence and sibling-instance behavior no longer depend on source order or on
whether another module's body type-checks.
