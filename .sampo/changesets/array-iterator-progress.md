---
cargo/alder-can: patch
cargo/alder-solve: patch
cargo/alder-codegen: patch
cargo/alder-kernel: patch
cargo/alder-bundle: patch
cargo/alder-cli: patch
---

Add Array.iter and independent ArrayIterator cursors, replacing the non-advancing
Iterator instance on arrays. Preserve shared source values, live iteration, and
correct Option payloads through exhaustion.
