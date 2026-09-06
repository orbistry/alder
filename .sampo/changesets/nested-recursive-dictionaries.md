---
cargo/alder-codegen: patch
cargo/alder-cli: patch
---

Resolve recursive derived dictionary references through nested container and
structural evidence, preventing undefined self bindings in emitted equality,
showing, and hashing functions.
