---
cargo/alder-solve: patch
---

Match constructor applications in generic trait implementation heads,
including leftmost partial-constructor recovery. Preserve shared constructor
bindings, prerequisite dictionaries, and overlap detection for recovered sections.
Honor explicit constructor sections before applying leftmost recovery during
coherence checking, including sections supplied by later trait arguments.
