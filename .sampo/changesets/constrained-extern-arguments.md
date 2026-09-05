---
cargo/alder-codegen: patch
---

Consume hidden trait dictionaries in extern adapters without leaking them into
foreign JavaScript arguments, including Result and abort-aware Task wrappers.
