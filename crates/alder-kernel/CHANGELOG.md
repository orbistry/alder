# alder-kernel

## 0.2.0 — 2026-09-03

### Minor changes

- [ff0e0a3](https://github.com/orbistry/alder/commit/ff0e0a303fec4805bef7bfcb2ac002feb09d44e4) Add built-in `Show`, `Eq`, `Ord`, `Hash`, and `Json` enum derives with callable trait methods and direct Oxc AST dictionary lowering. — Thanks @rvcas!
- [6e721dc](https://github.com/orbistry/alder/commit/6e721dc86e8cc39fac1f4464eac6f1beea422803) Add the built-in `Iterator` trait, its associated `Item` type, and the initial Array implementation. — Thanks @rvcas!
- [0258ff1](https://github.com/orbistry/alder/commit/0258ff1fef3e279249933be2a3c8e149ad28afcf) Adopt arrow lambdas and juxtaposed function return types, forward piped values
  to the first argument of existing calls, and add `Array.filter` for pipeline
  composition. — Thanks @rvcas!
- [7dab530](https://github.com/orbistry/alder/commit/7dab5303f81d1b4d07a9b9c43b6ea3bb6297d11a) Add the built-in `Traversable` trait and Array, Option, and Result implementations with method-level Applicative evidence. — Thanks @rvcas!
- [2b5848b](https://github.com/orbistry/alder/commit/2b5848b8d5b48c8ed3c954010e999a34567970e3) Add built-in `Applicative` and `Monad` traits and Array, Option, and Result implementations. — Thanks @rvcas!
- [7d53578](https://github.com/orbistry/alder/commit/7d53578e5aef1c152bda29fb55181c78fd9af45d) Implement the M2 core-language pipeline through direct Oxc AST generation,
  Rolldown bundling, the embedded standalone runtime, stdlib/kernel foundations,
  formatting, and test execution. — Thanks @rvcas!

### Patch changes

- [453c72d](https://github.com/orbistry/alder/commit/453c72dc916563c7c50154af8226def856b5fa47) Order derived enum values by declaration position before comparing payloads. — Thanks @rvcas!
- [ff239fa](https://github.com/orbistry/alder/commit/ff239fabf2f05239e699028b4a7c6d149e6f0e14) Hash primitive and structural values through deterministic typed 64-bit FNV-1a byte streams. — Thanks @rvcas!
- [6a6d7a4](https://github.com/orbistry/alder/commit/6a6d7a466e1bad50fdea3504857298d632f3bd9e) Implement the documented `Ord.compare -> Ordering` dictionary ABI. Generic
  comparison operators now inspect the tagged result, derived ordering composes
  selected field dictionaries through `compare`, and primitive comparisons keep
  their direct JavaScript lowering. — Thanks @rvcas!
- [794af50](https://github.com/orbistry/alder/commit/794af5063f5133c39e68ecdb18f34e9b493258fc) Encode and decode derived enums and error groups through their documented tagged JSON shape. — Thanks @rvcas!
- [e1d71c8](https://github.com/orbistry/alder/commit/e1d71c81b96d5afc8b473cf5e7ab0aaf3ed152f0) Resolve and retain trait evidence for every derived payload field, including
  nested builtin containers. Generated Show, Eq, Ord, Hash, and Json dictionaries
  now dispatch through the selected field dictionaries, and dictionary emission
  orders Eq superclasses before their dependents. — Thanks @rvcas!
- [a9be91f](https://github.com/orbistry/alder/commit/a9be91f26e5dc7b825c8d15ade9f13c12ebca8da) Add the explicit `Ref.same` identity operation selected by the trait equality
  design, backed by JavaScript reference equality and covered at runtime. — Thanks @rvcas!
- [7f62c2d](https://github.com/orbistry/alder/commit/7f62c2d71e83499bb88a66d0a93f03eb1bd70576) Expose `Eq` and `Ord` superclass dictionaries through built-in `Hash` and `Num`, and return 64-bit `BigInt` hashes. — Thanks @rvcas!
- [630f635](https://github.com/orbistry/alder/commit/630f635691158c8def6ca564a4950b6df98eca57) Preserve optional enum payload fields through canonicalization and inference,
  omit them from derived JSON, and accept them as absent when decoding. Render
  derived record-payload constructors in Alder syntax and source field order.
  Derived hashes now include canonical type identity, declaration variant index,
  and every payload field. Derived JSON decoding rejects unexpected envelope and
  payload fields.
  Associated-type validation now rejects indirect projection cycles and reports
  the complete cycle through the structured diagnostic renderer. — Thanks @rvcas!

