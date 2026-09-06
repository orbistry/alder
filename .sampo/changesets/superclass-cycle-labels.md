---
cargo/alder-driver: patch
---

Label a locally declared trait when reporting superclass cycles spanning modules,
instead of falling back to the start of the file for a foreign trait.
