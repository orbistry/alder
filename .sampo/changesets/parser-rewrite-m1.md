---
cargo/alder-source: minor
cargo/alder-parse: minor
---

Rewrite the source AST and the parser foundation for the M1 grammar
(docs/parser-internals.md): curly-brace items, statements and blocks, flat
binop chains with a fixed precedence table, `//` comments, the new
syntax-error hierarchy, and `todo!()` stubs with final signatures for every
remaining parse file.
