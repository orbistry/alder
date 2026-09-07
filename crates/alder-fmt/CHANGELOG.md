# alder-fmt

## 0.3.0 — 2026-09-07

### Minor changes

- [2cc1d05](https://github.com/orbistry/alder/commit/2cc1d057460100bce03b09d5cda3f8e7975f1a87) Unify bundled, local, and external imports with grouped syntax, lowercase
  standard-library namespaces, explicit utility imports, identity-preserving
  namespace re-exports, and canonical comment-preserving formatting. Share public
  interfaces between prelude and explicit imports, reject conflicting bindings,
  and make module initialization independent of import declaration order.
  
  Interface format 8 replaces earlier contracts without compatibility readers. — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-parse@0.5.0, alder-source@0.4.0

## 0.2.2 — 2026-09-06

### Patch changes

- Updated dependencies: alder-parse@0.4.0

## 0.2.1 — 2026-09-06

### Patch changes

- [94c055b](https://github.com/orbistry/alder/commit/94c055bd548fac411537df3f0bea79f5e38d2edd) Preserve parser-designated template, markup, raw macro, and comment text while formatting. Keep literal whitespace and line endings intact, validate verbatim payloads and physical token lines, and track formatting ranges through parser backtracking. — Thanks @rvcas!
- Updated dependencies: alder-parse@0.3.0, alder-source@0.3.0

## 0.2.0 — 2026-09-03

### Minor changes

- [7d53578](https://github.com/orbistry/alder/commit/7d53578e5aef1c152bda29fb55181c78fd9af45d) Implement the M2 core-language pipeline through direct Oxc AST generation,
  Rolldown bundling, the embedded standalone runtime, stdlib/kernel foundations,
  formatting, and test execution. — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-parse@0.2.0, alder-source@0.2.0

