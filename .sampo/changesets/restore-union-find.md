---
cargo/alder-solve: patch
---

Restore shared inference types backed by weighted union-find and path compression.
Preserve Alder's kind checks, generic contracts, row/trait semantics, diagnostics,
and isolated recovery attempts. Add graph invariants and reproducible timing and
allocation benchmarks; remove the obsolete unlinked solver implementation.
