---
cargo/alder-codegen: patch
cargo/alder-cli: patch
---

Capture reached pattern payloads before later pins can mutate their source,
preserving checked binding types, array-rest values, and alias identity.
