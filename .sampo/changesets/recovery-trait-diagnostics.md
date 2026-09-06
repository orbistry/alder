---
cargo/alder-solve: patch
cargo/alder-driver: patch
---

Report independent missing trait evidence after core type-error recovery by
checking only the freshly re-inferred remainder. Preserve dependency suppression
and failed-build publication gates, and order mixed diagnostic kinds by source.
