---
cargo/alder-codegen: patch
cargo/alder-cli: patch
---

Share lazy child dictionaries between container Hash and its Eq superclass,
preventing exponential emitted-code growth while preserving equality dispatch.
