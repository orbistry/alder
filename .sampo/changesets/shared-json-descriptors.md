---
cargo/alder-codegen: patch
cargo/alder-cli: patch
---

Avoid exponential generated-code growth for nested JSON codecs by sharing lazy
payload descriptors between encoding and decoding, preserving recursive codecs.
