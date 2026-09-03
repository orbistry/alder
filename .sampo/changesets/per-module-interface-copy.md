---
cargo/alder-ast: minor
cargo/alder-driver: patch
---

Deep-copy solved interfaces across arena boundaries so every source module can
release its parser, canonical, constraint, and solver allocations immediately
after compilation.
