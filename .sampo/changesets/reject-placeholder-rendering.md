---
cargo/alder-codegen: patch
---

Reject executable markup and styles until the rendering/CSS backends exist.
Remove placeholder object lowering that silently discarded markup directives;
preserve provisional check-only support with source-aware build diagnostics.
