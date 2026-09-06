---
cargo/alder-solve: minor
cargo/alder-constrain: minor
cargo/alder-driver: patch
---

Check pattern exhaustiveness and redundancy uniformly across enums, Option,
Result, nested payloads, tuples, records and array prefixes. Reject refutable
bindings and parameters at compile time, report uncovered patterns and covering
source locations, and preserve effectful guard and pin behavior.
