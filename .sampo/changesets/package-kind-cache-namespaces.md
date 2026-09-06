---
cargo/alder-driver: patch
cargo/alder-cli: patch
---

Separate interface and instance-index cache paths by package identity kind so
application modules, workspace members, named packages, and builtins cannot
overwrite one another's artifacts. Remove the unused unqualified cache lookup.
