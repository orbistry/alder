---
cargo/alder-solve: patch
cargo/alder-driver: patch
---

Normalize adjacent closed record spread operands before comparing inferred
contracts, so grouping fields cannot hide contradictory inherited-field types
in local or imported functions. Also discard closed fields guaranteed shadowed
by later closed writes across open spreads when comparing contracts. Preserve
rightmost writes, other inherited fields, and runtime initializer evaluation.
Known fields of acyclic open input records also establish guaranteed overwrites;
opaque cyclic producer obligations do not.
