---
cargo/alder-kernel: patch
cargo/alder-cli: patch
---

Invoke array map, filter, flatMap, and applicative callbacks with only their
declared value argument. Do not leak JavaScript array indexes or source arrays
to unary extern-returned callbacks.
