# alder-config

## 0.2.0 — 2026-09-03

### Minor changes

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

