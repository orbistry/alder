---
cargo/alder-solve: patch
cargo/alder-cli: patch
---

Keep later pattern alternatives reachable after a failed guard. Check their pin
exits against loop results, matching the existing per-alternative runtime guard
retry semantics.
