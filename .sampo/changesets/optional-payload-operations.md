---
cargo/alder-kernel: patch
cargo/alder-cli: patch
---

Delegate every declared record field to its ordinary equality, ordering,
hashing, and JSON dictionary. Preserve nested Options and unit payloads, and
decode missing Option fields as None.
