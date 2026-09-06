---
cargo/alder-codegen: patch
cargo/alder-cli: patch
---

Make equality inherited through primitive Hash dictionaries agree with ordinary
equality, including signed zero and NaN through nested containers and derives.
