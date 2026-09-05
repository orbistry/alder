---
cargo/alder-can: patch
---

Include assignment targets in top-level inference dependencies, including writes inside nested functions, so write-only references preserve recursive groups and dependency ordering.
