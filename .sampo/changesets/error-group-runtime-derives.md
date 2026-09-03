---
cargo/alder-codegen: patch
---

Match generated error-group derive dictionaries against the runtime's
colon-prefixed tag representation, and make recursive derived dictionaries
refer to their emitted binding instead of a factory-only local.
