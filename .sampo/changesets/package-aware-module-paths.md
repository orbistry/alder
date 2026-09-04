---
cargo/alder-driver: patch
cargo/alder-cli: patch
---

Reject duplicate module identities before publishing interfaces or code, with diagnostics for both source files. Resolve CLI project imports and canonical module paths using package identity and actual source roots, and resolve package-root imports to mod.ald.
