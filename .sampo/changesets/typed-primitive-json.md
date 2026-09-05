---
cargo/alder-solve: patch
cargo/alder-codegen: patch
cargo/alder-kernel: patch
---

Validate primitive Json trait decoding against the requested type, preserving
validation inside derived and container codecs. Encode unit as null and BigInt
as a decimal JSON string; reject non-finite JSON numbers instead of silently
encoding them as null.
