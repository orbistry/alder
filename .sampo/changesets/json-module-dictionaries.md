---
cargo/alder-can: patch
cargo/alder-codegen: patch
cargo/alder-bundle: patch
cargo/alder-kernel: patch
---

Require Json evidence for module encode/decode calls and dispatch through the
selected codec, including custom instances. Remove unchecked JSON entry points
and fix public export names for imported direct dictionary calls.
