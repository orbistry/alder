---
cargo/alder-solve: minor
cargo/alder-codegen: minor
cargo/alder-driver: patch
---

Apply contextual recursive Option lifting to fresh record field initializers,
including direct record return contexts, without converting existing mutable
record aliases. Emit field wrapping through the centralized Option helpers.
