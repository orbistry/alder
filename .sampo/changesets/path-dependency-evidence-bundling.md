---
cargo/alder-driver: minor
cargo/alder-cli: patch
---

Compile imported path-dependency sources into the same in-memory Oxc/Rolldown
module graph, allowing dictionary factories selected from unimported sibling
modules to bundle and execute without serializing generated JavaScript.
