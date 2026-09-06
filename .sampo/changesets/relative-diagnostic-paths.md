---
cargo/alder-report: patch
cargo/alder-cli: patch
---

Display compiler errors and warnings with project-relative source paths in the
CLI, including related and bundler diagnostics, without changing editor file
identities or diagnostic locations. Supported terminals receive explicit absolute
file hyperlinks behind the short labels so navigation does not depend on the
shell's working directory.
