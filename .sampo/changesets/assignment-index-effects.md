---
cargo/alder-ast: patch
cargo/alder-solve: patch
---

Include assignment indices when detecting function suspension, and check their
early returns and error propagation against the enclosing function's contract.
