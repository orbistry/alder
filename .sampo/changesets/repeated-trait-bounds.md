---
cargo/alder-can: patch
---

Preserve individual repeated trait-bound clauses without panicking, and avoid
false associated-type ambiguity when the same trait is mentioned more than once.
