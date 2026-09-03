---
cargo/alder-solve: patch
cargo/alder-codegen: patch
cargo/alder-kernel: patch
---

Resolve and retain trait evidence for every derived payload field, including
nested builtin containers. Generated Show, Eq, Ord, Hash, and Json dictionaries
now dispatch through the selected field dictionaries, and dictionary emission
orders Eq superclasses before their dependents.
