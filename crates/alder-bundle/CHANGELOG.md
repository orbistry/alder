# alder-bundle

## 0.3.0 — 2026-09-04

### Minor changes

- [f87e540](https://github.com/orbistry/alder/commit/f87e540113feb6c1052b2d782a7ad6e1faa7d204) Add inferred lazy tasks, generator-based async lowering, Promise extern lifting,
  and a structured fiber runtime with interruption, scopes, finalizers, `all`, and
  `race`. — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-codegen@0.4.0, alder-kernel@0.4.0

## 0.2.2 — 2026-09-04

### Patch changes

- Updated dependencies: alder-codegen@0.3.0, alder-kernel@0.3.0

## 0.2.1 — 2026-09-03

### Patch changes

- Updated dependencies: alder-codegen@0.2.1

## 0.2.0 — 2026-09-03

### Minor changes

- [0258ff1](https://github.com/orbistry/alder/commit/0258ff1fef3e279249933be2a3c8e149ad28afcf) Adopt arrow lambdas and juxtaposed function return types, forward piped values
  to the first argument of existing calls, and add `Array.filter` for pipeline
  composition. — Thanks @rvcas!
- [7d53578](https://github.com/orbistry/alder/commit/7d53578e5aef1c152bda29fb55181c78fd9af45d) Implement the M2 core-language pipeline through direct Oxc AST generation,
  Rolldown bundling, the embedded standalone runtime, stdlib/kernel foundations,
  formatting, and test execution. — Thanks @rvcas!

### Patch changes

- [a9be91f](https://github.com/orbistry/alder/commit/a9be91f26e5dc7b825c8d15ade9f13c12ebca8da) Add the explicit `Ref.same` identity operation selected by the trait equality
  design, backed by JavaScript reference equality and covered at runtime. — Thanks @rvcas!
- Updated dependencies: alder-codegen@0.2.0, alder-kernel@0.2.0

