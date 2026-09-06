---
cargo/alder-driver: minor
cargo/alder-cli: patch
---

Require explicit package and source-relative identity metadata for every source
module. Remove URI-based identity guesses and metadata-free driver entry points;
report missing metadata deterministically before publishing compiler artifacts.
