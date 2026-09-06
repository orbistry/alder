---
cargo/alder-can: minor
cargo/alder-driver: patch
---

Warn on unused module value bindings using resolved references and conservative
reachability. Respect exports, recursion, shadowing, entry points, tests and
method bodies, while retaining side-effectful initializers and their helpers.
