---
cargo/alder-kernel: patch
cargo/alder-cli: patch
---

Append variable-length hash payload bytes without spreading them into function
arguments, preventing large strings from exceeding the JavaScript argument limit
while preserving the existing hash byte format.
