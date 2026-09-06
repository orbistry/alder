---
cargo/alder-codegen: patch
cargo/alder-cli: patch
---

Include runtime tag prefixes in structural error-row equality metadata so
errors with the same tag but different payloads no longer compare equal.
