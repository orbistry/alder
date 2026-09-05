---
cargo/alder-ast: patch
cargo/alder-can: patch
cargo/alder-solve: patch
---

Base top-level generalization on resolved assignments rather than mutation permission flags. Preserve safe polymorphism for never-assigned function bindings while restricting shared replaceable functions, including writes inside nested closures.
