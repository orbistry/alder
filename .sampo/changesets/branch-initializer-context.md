---
cargo/alder-solve: patch
cargo/alder-cli: patch
---

Preserve expected field types through if, match, and block-tail initializers.
Keep implicit Option wrapping at call and field boundaries, reject conversions
of existing mutable records, and preserve reachable fallthrough checks.
