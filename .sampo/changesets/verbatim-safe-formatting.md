---
cargo/alder-parse: patch
cargo/alder-fmt: patch
---

Preserve parser-designated template, markup, raw macro, and comment text while formatting. Keep literal whitespace and line endings intact, validate verbatim payloads and physical token lines, and track formatting ranges through parser backtracking.
