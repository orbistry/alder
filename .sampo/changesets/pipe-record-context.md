---
cargo/alder-solve: patch
cargo/alder-cli: patch
---

Preserve fresh record and array initializer context on pipe inputs, including
Option-valued fields and optional parameters. Check each input once without
converting existing mutable aliases or changing runtime evaluation order.
