---
cargo/alder-solve: patch
cargo/alder-driver: patch
---

Retain return annotation locations for explicit returns and lambda tails, and
highlight the returned expression. Isolate annotation origins at nested lambda
and async boundaries so diagnostics do not blame an outer function's signature.
