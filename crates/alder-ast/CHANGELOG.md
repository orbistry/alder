# alder-ast

## 0.3.1 — 2026-09-07

### Patch changes

- [2cc1d05](https://github.com/orbistry/alder/commit/2cc1d057460100bce03b09d5cda3f8e7975f1a87) Unify bundled, local, and external imports with grouped syntax, lowercase
  standard-library namespaces, explicit utility imports, identity-preserving
  namespace re-exports, and canonical comment-preserving formatting. Share public
  interfaces between prelude and explicit imports, reject conflicting bindings,
  and make module initialization independent of import declaration order.
  
  Interface format 8 replaces earlier contracts without compatibility readers. — Thanks @rvcas!
- Updated dependencies: alder-source@0.4.0

## 0.3.0 — 2026-09-06

### Minor changes

- [ea6b284](https://github.com/orbistry/alder/commit/ea6b2849eee22f4332de8319392a1cbe60d7c5d7) Resolve embedded stdlib members through packaged Alder signatures, rejecting unknown members, wrong arguments, invalid callbacks, and arity mismatches. Remove untyped builtin references and correct expected-versus-actual call diagnostics. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Treat optional record-field shorthand as an ordinary Option type and materialize omitted Option fields as None during contextual record construction.
  
  Remove separate record-field optionality from serialized interfaces and invalidate the previous interface format.
  
  Read Fiber traversal concurrency options using ordinary Option semantics so omitted and explicit None fields both select the sequential default.
  
  Preserve known generic Result error rows during propagation, including explicit Option fallback branches.
  
  Provide conditional Option ordering and Unit ordering so derived record payloads use ordinary field dictionaries, including nested Options. — Thanks @rvcas!
- [f87e540](https://github.com/orbistry/alder/commit/f87e540113feb6c1052b2d782a7ad6e1faa7d204) Add inferred lazy tasks, generator-based async lowering, Promise extern lifting,
  and a structured fiber runtime with interruption, scopes, finalizers, `all`, and
  `race`. — Thanks @rvcas!
- [0db756d](https://github.com/orbistry/alder/commit/0db756d7014df5c506fafe13df0b93306896d1fa) Represent explicit async function declarations and lazy async block syntax. — Thanks @rvcas!
- [d87e721](https://github.com/orbistry/alder/commit/d87e721ad6aa621a26853257257f3f5b2989d3e9) Carry error-row inclusion relationships in canonical and serialized annotations,
  preserving them through arena copies and interface hydration. Bump the interface
  format for the new scheme layout.
  
  Retain inclusion dependencies during solver generalization and instantiation,
  and reject inferred inclusions between independent universal error-row tails.
  
  Resolve concrete error unions for inferred results, including exhaustive matches
  across module boundaries. Preserve explicitly open result contracts and apply
  directional Result return checking consistently to named functions and lambdas. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Add sparse exact-length tuple constraint metadata to annotations and stored interfaces, preserving arena copies and fingerprint identity with a new interface format. Carry imported constraints through inference and check concrete tuple lengths, constrained elements, and generic contracts.
  
  Finalize source tuple projections without allocating by the largest index, preserve fixed tuple lengths, and reject recursive tuple-element constraints. — Thanks @rvcas!

### Patch changes

- [23b763c](https://github.com/orbistry/alder/commit/23b763c58372b388f9af9388bd3afc0ae658205d) Respect Boolean short circuits and false match guards when checking loop exits
  and structural divergence. — Thanks @rvcas!
- [f7fb26b](https://github.com/orbistry/alder/commit/f7fb26bab952eeb7c574638b5e237e82ebd817ad) Permit type-checked reassignment and field/index writes through ordinary let bindings and function or lambda parameters. Keep assignment-aware generalization restrictions on shared replaceable values.
  
  Remove obsolete mutability fields from canonical lets, parameters, and assignment places. Local pattern bindings are writable; non-storage references retain assignment-target checks with diagnostics that no longer suggest adding `mut`.
  
  Remove `mut` from the grammar, keyword list, and source AST. Parsing uses the current grammar without compatibility handling or migration diagnostics. — Thanks @rvcas!
- [f7fb26b](https://github.com/orbistry/alder/commit/f7fb26bab952eeb7c574638b5e237e82ebd817ad) Include assignment indices when detecting function suspension, and check their
  early returns and error propagation against the enclosing function's contract. — Thanks @rvcas!
- [38a9ac9](https://github.com/orbistry/alder/commit/38a9ac987ad58f354bcd7c87569563a868f62674) Preserve pin-expression exits in structural match control-flow summaries.
  Distinguish matching from rejection so unreachable sibling patterns, alternatives,
  guards, and arm bodies cannot contribute exits.
  Apply guards per alternative, preserving retries after a false guard. — Thanks @rvcas!
- [138d0d0](https://github.com/orbistry/alder/commit/138d0d0f10b38f2d6065ef790a5a46c61b584950) Distinguish normal fallthrough from control-flow exits, reject missing function results after zero-iteration loops, accept returning branches and lambdas without artificial unit values, and preserve loop-tail and break-payload effects in generated JavaScript. — Thanks @rvcas!
- [f7fb26b](https://github.com/orbistry/alder/commit/f7fb26bab952eeb7c574638b5e237e82ebd817ad) Preserve ordered record-overlay relationships through inference, generalization, and owned interfaces. Keep independent input rows distinct, respect rightmost field overwrites, and check exposed fields against generic contracts. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Retain and emit inherited default methods when implementing an imported trait,
  including async defaults and constrained dictionary factories. Publish accurate
  default-helper symbols and implementation method metadata, and invalidate stale
  interface caches. — Thanks @rvcas!
- [f7fb26b](https://github.com/orbistry/alder/commit/f7fb26bab952eeb7c574638b5e237e82ebd817ad) Base top-level generalization on resolved assignments rather than mutation permission flags. Preserve safe polymorphism for never-assigned function bindings while restricting shared replaceable functions, including writes inside nested closures. — Thanks @rvcas!
- Updated dependencies: alder-source@0.3.0

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

