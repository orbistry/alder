---
cargo/alder-solve: patch
cargo/alder-codegen: patch
cargo/alder-cli: patch
---

Fix Option coalescing to return its payload type, preserve unit and nested
Options, and evaluate the default only for None.
