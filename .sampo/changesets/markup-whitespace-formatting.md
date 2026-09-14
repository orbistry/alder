---
cargo/alder-parse: minor
cargo/alder-fmt: minor
cargo/alder-driver: patch
---

Fold ordinary multiline markup text consistently before DOM/SSR code generation,
preserving inline spaces, explicit string expressions, and whitespace-sensitive
pre/textarea subtrees. Format nested markup, prose, attributes, directives, and
embedded blocks with parsed-structure equivalence checks and idempotence tests.
Add compiled before/after formatting coverage for SSR, DOM, and node-reusing
hydration, and format the web examples.
