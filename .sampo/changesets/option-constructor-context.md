---
cargo/alder-solve: patch
cargo/alder-cli: patch
---

Propagate expected Option payload types through explicit Some and Option.some
construction, allowing fresh record fields to receive their contextual Option
types without converting existing mutable payload aliases.
