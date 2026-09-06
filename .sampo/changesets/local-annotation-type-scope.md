---
cargo/alder-can: patch
cargo/alder-solve: patch
cargo/alder-cli: patch
---

Allow type variables in local let annotations and reuse the enclosing callable's
generic variables consistently with lambda annotations. Preserve generic
contract checking and monomorphic local bindings.
