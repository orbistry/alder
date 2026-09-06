---
cargo/alder-can: minor
cargo/alder-solve: minor
cargo/alder-codegen: minor
cargo/alder-kernel: minor
cargo/alder-cli: patch
---

Resolve Hash structurally for closed error rows, honoring payload dictionaries
and preserving hashes across equivalent groups and row widening. Share payload
evidence with the equality superclass without duplicating nested generated code.
Remove nominal error-group derives and their declaration-order-dependent behavior;
error groups use conditional structural Eq, Show, Hash, and Json capabilities.
Normalize derived enum payloads through the ordinary annotation converter so
nominal wrappers of named error groups retain their checked structural evidence.
