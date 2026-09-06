# alder-source

## 0.3.0 — 2026-09-06

### Minor changes

- [470e3ef](https://github.com/orbistry/alder/commit/470e3ef31e57097de46903b6a2c9513af3410c1c) Parse named optional parameter annotations and canonicalize their shorthand to
  ordinary builtin Option types, including lambda and trait signatures. — Thanks @rvcas!
- [f7fb26b](https://github.com/orbistry/alder/commit/f7fb26bab952eeb7c574638b5e237e82ebd817ad) Permit type-checked reassignment and field/index writes through ordinary let bindings and function or lambda parameters. Keep assignment-aware generalization restrictions on shared replaceable values.
  
  Remove obsolete mutability fields from canonical lets, parameters, and assignment places. Local pattern bindings are writable; non-storage references retain assignment-target checks with diagnostics that no longer suggest adding `mut`.
  
  Remove `mut` from the grammar, keyword list, and source AST. Parsing uses the current grammar without compatibility handling or migration diagnostics. — Thanks @rvcas!
- [0db756d](https://github.com/orbistry/alder/commit/0db756d7014df5c506fafe13df0b93306896d1fa) Represent explicit async function declarations and lazy async block syntax. — Thanks @rvcas!

## 0.2.0 — 2026-09-03

### Minor changes

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

- Updated dependencies: alder-region@0.2.0

