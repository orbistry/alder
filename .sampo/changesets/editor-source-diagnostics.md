---
cargo/alder-language-server: minor
cargo/alder-cli: patch
---

Publish real compiler errors and warnings for versioned unsaved documents,
recheck dependents, and clear stale editor diagnostics. Preserve UTF-16 source
ranges and secondary requirement locations. Fix incomplete stdio responses by
using the native Tokio language-server transport.
