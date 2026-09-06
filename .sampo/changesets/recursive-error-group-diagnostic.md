---
cargo/alder-solve: patch
cargo/alder-constrain: patch
cargo/alder-driver: patch
---

Detect cyclic structural error-group expansion and report the recursive source
reference instead of aborting the compiler with a stack overflow.
