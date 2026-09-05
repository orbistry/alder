---
cargo/alder-parse: patch
cargo/alder-driver: patch
cargo/alder-cli: patch
---

Reject overflowing tuple indices with a source diagnostic instead of silently
changing them to the largest representable index.
