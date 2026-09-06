# alder-parse

## 0.4.0 — 2026-09-06

### Minor changes

- [8727140](https://github.com/orbistry/alder/commit/8727140fdbb63100e9609baea374591a608ff748) Preserve detailed parser failures and nearby source context for core expressions,
  patterns, and loops. Explain common separator and branch-syntax mistakes, label
  whole Unicode characters, and identify end-of-file failures without changing
  source identities or editor coordinates.
  
  Render dedicated declaration, template, and escape diagnostics. Raw macro errors
  now retain the expected closing delimiter and its opening position, enabling
  precise nested-delimiter and end-of-file messages.
  
  Cover active types, traits, impls, imports, attributes, queries, styles, markup,
  reserved operators, direct errors, and nesting guards with reviewed rendered
  regressions. Verify canonical CLI hyperlinks and LSP Unicode/EOF ranges and
  related labels end to end without changing accepted syntax. — Thanks @rvcas!
- [fe6e840](https://github.com/orbistry/alder/commit/fe6e840c87737e69bcfb3ce033bc274d6f933964) Retain pre-whitespace/comment boundaries in shared delimiter errors, including
  parser backtracking. Report cross-line missing punctuation at the insertion
  boundary and label the actual opening punctuation, not the enclosing declaration.
  Keep detection evidence internal rather than labeling valid following code, retain
  actual mismatched closer locations, and distinguish required separators from list
  entries in CLI/editor messages. Delimiter error variants now carry ExpectedEnd
  instead of a row/column pair; error-row extension and non-select query closers have
  separate variants so their messages do not offer invalid continuations. — Thanks @rvcas!

## 0.3.0 — 2026-09-06

### Minor changes

- [470e3ef](https://github.com/orbistry/alder/commit/470e3ef31e57097de46903b6a2c9513af3410c1c) Parse named optional parameter annotations and canonicalize their shorthand to
  ordinary builtin Option types, including lambda and trait signatures. — Thanks @rvcas!
- [f7fb26b](https://github.com/orbistry/alder/commit/f7fb26bab952eeb7c574638b5e237e82ebd817ad) Permit type-checked reassignment and field/index writes through ordinary let bindings and function or lambda parameters. Keep assignment-aware generalization restrictions on shared replaceable values.
  
  Remove obsolete mutability fields from canonical lets, parameters, and assignment places. Local pattern bindings are writable; non-storage references retain assignment-target checks with diagnostics that no longer suggest adding `mut`.
  
  Remove `mut` from the grammar, keyword list, and source AST. Parsing uses the current grammar without compatibility handling or migration diagnostics. — Thanks @rvcas!
- [0db756d](https://github.com/orbistry/alder/commit/0db756d7014df5c506fafe13df0b93306896d1fa) Represent explicit async function declarations and lazy async block syntax. — Thanks @rvcas!

### Patch changes

- [94c055b](https://github.com/orbistry/alder/commit/94c055bd548fac411537df3f0bea79f5e38d2edd) Preserve parser-designated template, markup, raw macro, and comment text while formatting. Keep literal whitespace and line endings intact, validate verbatim payloads and physical token lines, and track formatting ranges through parser backtracking. — Thanks @rvcas!
- [14be079](https://github.com/orbistry/alder/commit/14be07997423e789b7a0f52954a1f709fcfa7afe) Reject overflowing tuple indices with a source diagnostic instead of silently
  changing them to the largest representable index. — Thanks @rvcas!
- Updated dependencies: alder-source@0.3.0

## 0.2.0 — 2026-09-03

### Minor changes

- [0258ff1](https://github.com/orbistry/alder/commit/0258ff1fef3e279249933be2a3c8e149ad28afcf) Adopt arrow lambdas and juxtaposed function return types, forward piped values
  to the first argument of existing calls, and add `Array.filter` for pipeline
  composition. — Thanks @rvcas!
- [7d53578](https://github.com/orbistry/alder/commit/7d53578e5aef1c152bda29fb55181c78fd9af45d) Implement the M2 core-language pipeline through direct Oxc AST generation,
  Rolldown bundling, the embedded standalone runtime, stdlib/kernel foundations,
  formatting, and test execution. — Thanks @rvcas!
- [241b1d5](https://github.com/orbistry/alder/commit/241b1d50da84a6cb4890a817f1d654f37470688c) Rewrite the source AST and the parser foundation for the M1 grammar
  (docs/parser-internals.md): curly-brace items, statements and blocks, flat
  binop chains with a fixed precedence table, `//` comments, the new
  syntax-error hierarchy, and `todo!()` stubs with final signatures for every
  remaining parse file.
  
  Widen `Position`'s `line` and `column` (and the parser's `Row` / `Col`
  aliases) from `u16` to `u32`, so a line longer than 65535 bytes or a file
  with more than 65535 lines no longer overflows the position counters.
  Nesting deeper than `alder_parse::MAX_NESTING` (128 levels) is a `TooDeep`
  syntax error instead of a stack overflow.
  
  `alder-config`: drop `exposedModules` and `sourceDirectories`; add the required application `target` and optional package `target` (`cloudflare` | `standalone`). — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-region@0.2.0, alder-source@0.2.0

