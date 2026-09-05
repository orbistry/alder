---
cargo/alder-codegen: patch
---

Evaluate assignment targets before right-hand setup statements, preserving the
original receiver across mutation. Support indexed targets with setup statements
and capture compound-assignment values before right-hand effects, including
dictionary-dispatched arithmetic.
