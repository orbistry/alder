---
cargo/alder-driver: patch
cargo/alder-solve: patch
---

Preserve source type-variable names in trait failures and add source-aware
rendering for unsatisfied bounds, orphan impls, overlaps, kind mismatches, and
associated-type cycles. Render canonical trait member, associated-type, derive,
and duplicate errors with precise source labels as well.
