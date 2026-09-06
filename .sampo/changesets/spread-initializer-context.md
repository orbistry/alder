---
cargo/alder-solve: patch
cargo/alder-cli: patch
---

Preserve contextual Option field initialization in records containing spreads.
Resolve written-field wrapping through ordered field relationships so overwritten
values retain their own types, later Option fields overwrite earlier values, and inherited
mutable payloads are not converted. Support nested fresh constructors and arrays.
