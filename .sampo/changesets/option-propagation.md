---
cargo/alder-solve: minor
cargo/alder-codegen: minor
cargo/alder-driver: patch
---

Support Option propagation with postfix `?` in Option-returning contexts,
including async bodies and pipe destinations, without implicit Result conversion.
Wait for recursive peers to constrain unknown propagation carriers before
generalization, preserving inferred Option contracts across module boundaries.
