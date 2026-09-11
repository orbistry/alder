---
cargo/alder-cli: patch
---

Use one options-aware command execution path and remove the redundant compile
wrapper and constant persistence flag. Exercise checked-in end-to-end programs,
intentional failures, and deterministic bundles/diagnostics through the actual
CLI in isolated temporary projects.
