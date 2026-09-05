---
cargo/alder-can: minor
cargo/alder-ast: patch
cargo/alder-driver: patch
cargo/alder-source: minor
cargo/alder-parse: minor
---

Permit type-checked reassignment and field/index writes through ordinary let bindings and function or lambda parameters. Keep assignment-aware generalization restrictions on shared replaceable values.

Remove obsolete mutability fields from canonical lets, parameters, and assignment places. Local pattern bindings are writable; non-storage references retain assignment-target checks with diagnostics that no longer suggest adding `mut`.

Remove `mut` from the grammar, keyword list, and source AST. Parsing uses the current grammar without compatibility handling or migration diagnostics.
