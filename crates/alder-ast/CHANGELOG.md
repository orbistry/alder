# alder-ast

## 0.2.0 — 2026-09-03

### Minor changes

- [b49583d](https://github.com/orbistry/alder/commit/b49583dbcb4896846de69824e3a235df6ec77315) Deep-copy solved interfaces across arena boundaries so every source module can
  release its parser, canonical, constraint, and solver allocations immediately
  after compilation. — Thanks @rvcas!
- [7757405](https://github.com/orbistry/alder/commit/7757405a85dbdd52f0a4d3109df65dfd2b34414a) Preserve implementation source locations across semantic interfaces and add validated package-instance-index persistence and hydration. — Thanks @rvcas!
- [7d53578](https://github.com/orbistry/alder/commit/7d53578e5aef1c152bda29fb55181c78fd9af45d) Implement the M2 core-language pipeline through direct Oxc AST generation,
  Rolldown bundling, the embedded standalone runtime, stdlib/kernel foundations,
  formatting, and test execution. — Thanks @rvcas!
- [4dc0e01](https://github.com/orbistry/alder/commit/4dc0e0118bd1ce68549d110f2499eb5f510739bb) Resolve associated-type equalities to stable trait identities, preserve them in inferred schemes and interfaces, and normalize projections through declared equalities and impl bindings. — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-region@0.2.0, alder-source@0.2.0

