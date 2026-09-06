---
cargo/alder-can: patch
cargo/alder-cli: patch
---

Resolve pinned expressions against the enclosing scope before pattern bindings
are introduced, preventing field-order-dependent lookup and uninitialized
local references when patterns shadow an outer name.
