---
cargo/alder-solve: patch
---

Check record-field assignments against their ordinary stored type, requiring
Option values for Option fields and rejecting implicit payload lifting or
traversal through an Option parent.
