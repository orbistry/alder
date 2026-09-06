---
cargo/alder-codegen: patch
cargo/alder-cli: patch
---

Initialize trait dictionaries in superclass dependency order before top-level
values, fixing references to later-declared or derived superclass dictionaries.
