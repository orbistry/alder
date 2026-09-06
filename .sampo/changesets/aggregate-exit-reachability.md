---
cargo/alder-solve: patch
cargo/alder-cli: patch
---

Carry reachability across aggregate elements, record fields, tag payloads,
template interpolations, indexing, and assignment operands so unreachable break
payloads do not constrain an enclosing loop's result. Include contextually
checked arrays and records.
