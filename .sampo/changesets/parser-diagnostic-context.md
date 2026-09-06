---
cargo/alder-driver: patch
cargo/alder-parse: minor
---

Preserve detailed parser failures and nearby source context for core expressions,
patterns, and loops. Explain common separator and branch-syntax mistakes, label
whole Unicode characters, and identify end-of-file failures without changing
source identities or editor coordinates.

Render dedicated declaration, template, and escape diagnostics. Raw macro errors
now retain the expected closing delimiter and its opening position, enabling
precise nested-delimiter and end-of-file messages.

Cover active types, traits, impls, imports, attributes, queries, styles, markup,
reserved operators, direct errors, and nesting guards with reviewed rendered
regressions. Verify canonical CLI hyperlinks and LSP Unicode/EOF ranges and
related labels end to end without changing accepted syntax.
