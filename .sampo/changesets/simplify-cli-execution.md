---
cargo/alder-cli: patch
---

Use one options-aware command execution path and remove the redundant compile
wrapper and constant persistence flag. Remove subprocess-based CLI end-to-end
tests and their test-only dependencies; retain direct compiler, runtime, and
reporting renderer tests.
