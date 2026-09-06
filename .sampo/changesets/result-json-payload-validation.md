---
cargo/alder-kernel: patch
cargo/alder-cli: patch
---

Reject JSON Result envelopes with a missing payload before invoking a custom
payload decoder, preserving the decoder's String argument contract.
