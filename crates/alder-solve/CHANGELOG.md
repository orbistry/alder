# alder-solve

## 0.5.1 — 2026-09-07

### Patch changes

- [2cc1d05](https://github.com/orbistry/alder/commit/2cc1d057460100bce03b09d5cda3f8e7975f1a87) Unify bundled, local, and external imports with grouped syntax, lowercase
  standard-library namespaces, explicit utility imports, identity-preserving
  namespace re-exports, and canonical comment-preserving formatting. Share public
  interfaces between prelude and explicit imports, reject conflicting bindings,
  and make module initialization independent of import declaration order.
  
  Interface format 8 replaces earlier contracts without compatibility readers. — Thanks @rvcas!
- Updated dependencies: alder-ast@0.3.1, alder-can@0.6.0, alder-constrain@0.5.1

## 0.5.0 — 2026-09-06

### Minor changes

- [8cb8b18](https://github.com/orbistry/alder/commit/8cb8b181878b325ebd12585d03f7b0172d8c7ef1) Check pattern exhaustiveness and redundancy uniformly across enums, Option,
  Result, nested payloads, tuples, records and array prefixes. Reject refutable
  bindings and parameters at compile time, report uncovered patterns and covering
  source locations, and preserve effectful guard and pin behavior. — Thanks @rvcas!
- [563041a](https://github.com/orbistry/alder/commit/563041aaef0af67e3c1813ad41235a2bd42c2f1e) Accumulate independent declaration type errors using fresh inference attempts,
  suppress dependent failures, and retain the failed-module publication barrier. — Thanks @rvcas!

### Patch changes

- [52732c2](https://github.com/orbistry/alder/commit/52732c2173f38376da3372582de0347b8105a3ac) Preserve structured types in missing-return, invalid Result error-kind, and associated-equality diagnostics so nested types use resolved import names at reporting time. — Thanks @rvcas!
- [6b0010f](https://github.com/orbistry/alder/commit/6b0010f05674b8203029299d717e0c44de267101) Collect invalid declaration error kinds before body inference, preventing dependent type-error cascades. — Thanks @rvcas!
- [8e03ebe](https://github.com/orbistry/alder/commit/8e03ebe56450b5f86574e1a2f5a5651c2d7b5023) Recover independent implementation and trait-default body failures using fresh inference attempts, retaining declaration contracts for diagnostics and suppressing dependent bodies without hiding unrelated methods. — Thanks @rvcas!
- [8dc960c](https://github.com/orbistry/alder/commit/8dc960c1cfd21d9fce64470714dae904d8f2d2b8) Retain available record fields in missing-field diagnostics and offer conservative, unambiguous typo suggestions. — Thanks @rvcas!
- [8c09806](https://github.com/orbistry/alder/commit/8c098064375e1f07292f89a8dba4288fcb35e12b) Keep enclosing function, tuple and applied type comparisons in mismatch diagnostics, including deferred Option payload checks and normalized associated types. — Thanks @rvcas!
- [30635be](https://github.com/orbistry/alder/commit/30635be9554d9419a6fae026b0a01f88be6726b2) Retain structured recursive type equations for occurs-check failures and explain
  why structural cycles cannot have finite types. Use shared readable variable
  names and preserve call, branch and array-element diagnostic context. — Thanks @rvcas!
- [ea12495](https://github.com/orbistry/alder/commit/ea1249529f95314799f5241bc725bc9d7541a65d) Preserve owned nominal identities in diagnostic types and render consumer import names, re-export aliases, or explicit module/package provenance instead of ambiguous short names. — Thanks @rvcas!
- [db5af22](https://github.com/orbistry/alder/commit/db5af22dcae75be7c815d00a3a329fb2a5578d48) Preserve structured mismatch types until diagnostic rendering, including distinct generic variables and open record/error rows. — Thanks @rvcas!
- [5adcd76](https://github.com/orbistry/alder/commit/5adcd76c4e07726d71ba42a6566fe3901c1a6df6) Report independent statement type errors within a function using isolated retries, suppressing invalid local dependencies and delivering the results to CLI and editor consumers. — Thanks @rvcas!
- [00451d1](https://github.com/orbistry/alder/commit/00451d103570fbec7a31237cb86d2fbc01a0b1d0) Label the earlier local requirement when associated-type equalities conflict. — Thanks @rvcas!
- [750ac88](https://github.com/orbistry/alder/commit/750ac88dddbee64ae137fd553cd7d54bc5fe06cc) Preserve complete structured trait arguments and obligation chains, localize imported names consistently, retain declared and distinct inferred variables, and keep trait hints within valid equality and generic-contract rules. — Thanks @rvcas!
- [5567bf1](https://github.com/orbistry/alder/commit/5567bf17dd7b016c2cc1f6b108f5db863f887a64) Retain structured generic restrictions and explain concrete specialization, independent-variable equality, open-row restrictions, and shared-storage escape without speculative signature changes. — Thanks @rvcas!
- [66077d5](https://github.com/orbistry/alder/commit/66077d55c9314328f5fadcefa74b47f62678f103) Do not count nested pinned Result payloads as unconditional pattern coverage. — Thanks @rvcas!
- [b0bb65b](https://github.com/orbistry/alder/commit/b0bb65bf4f36b5fde164fe8b8ae0f6cb6472db1d) Preserve structured record comparisons and distinguish missing from unexpected fields, including conservative typo suggestions and enclosing nested-record shapes. — Thanks @rvcas!
- [43eaf30](https://github.com/orbistry/alder/commit/43eaf300b668b9f52371a40fbd92240e19e44e6a) Preserve argument position and callee names when optional-argument compatibility
  is checked after inference, including piped calls and inconsistent Option depths. — Thanks @rvcas!
- [5120a13](https://github.com/orbistry/alder/commit/5120a136a3b806684d16edd3856b045e1c53743e) Report independent missing trait evidence after core type-error recovery by
  checking only the freshly re-inferred remainder. Preserve dependency suppression
  and failed-build publication gates, and order mixed diagnostic kinds by source. — Thanks @rvcas!
- [f137a2b](https://github.com/orbistry/alder/commit/f137a2b800e41595e921e29e216a255df9301363) Detect trait resolution cycles using complete predicate identities instead of rendered type names, allowing nested-record equality and preserving the actual missing-payload diagnostic. — Thanks @rvcas!
- [9e410e9](https://github.com/orbistry/alder/commit/9e410e938fccf5630c83e243b1421b9bf742a29c) Retain return annotation locations for explicit returns and lambda tails, and
  highlight the returned expression. Isolate annotation origins at nested lambda
  and async boundaries so diagnostics do not blame an outer function's signature. — Thanks @rvcas!
- [51a458d](https://github.com/orbistry/alder/commit/51a458d94bb965f8d59121b70af0114339572456) Preserve return-annotation context for early record type comparisons, including transparent aliases and explicit returns, without relabeling unrelated projection failures. — Thanks @rvcas!
- [a63a5e0](https://github.com/orbistry/alder/commit/a63a5e0be9ae9437dcd8b60e615cce5ca8fe1afb) Label written return annotations in missing-return diagnostics, including aliases and async functions, without inventing annotation origins for inferred results. — Thanks @rvcas!
- [875607f](https://github.com/orbistry/alder/commit/875607f62375e37441f8a9522e97b8955fa443ef) Explain immediate type expectations with contextual source labels and originating annotations, and report call arity with argument counts. — Thanks @rvcas!
- Updated dependencies: alder-can@0.5.0, alder-constrain@0.5.0

## 0.4.0 — 2026-09-06

### Minor changes

- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Provide structural Json codecs for closed error rows with Json-capable payloads,
  independent of group names and declaration order. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Apply contextual recursive Option lifting to fresh record field initializers,
  including direct record return contexts, without converting existing mutable
  record aliases. Emit field wrapping through the centralized Option helpers. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Provide structural Show for closed error rows when every payload supports Show,
  independent of named error groups and their declaration order. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Treat optional record-field shorthand as an ordinary Option type and materialize omitted Option fields as None during contextual record construction.
  
  Remove separate record-field optionality from serialized interfaces and invalidate the previous interface format.
  
  Read Fiber traversal concurrency options using ordinary Option semantics so omitted and explicit None fields both select the sequential default.
  
  Preserve known generic Result error rows during propagation, including explicit Option fallback branches.
  
  Provide conditional Option ordering and Unit ordering so derived record payloads use ordinary field dictionaries, including nested Options. — Thanks @rvcas!
- [f87e540](https://github.com/orbistry/alder/commit/f87e540113feb6c1052b2d782a7ad6e1faa7d204) Add inferred lazy tasks, generator-based async lowering, Promise extern lifting,
  and a structured fiber runtime with interruption, scopes, finalizers, `all`, and
  `race`. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Reject custom trait implementations directly targeting named error groups or
  their transparent aliases, with a source-aware structural-row diagnostic. — Thanks @rvcas!
- [0db756d](https://github.com/orbistry/alder/commit/0db756d7014df5c506fafe13df0b93306896d1fa) Represent explicit async function declarations and lazy async block syntax. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Resolve Hash structurally for closed error rows, honoring payload dictionaries
  and preserving hashes across equivalent groups and row widening. Share payload
  evidence with the equality superclass without duplicating nested generated code.
  Remove nominal error-group derives and their declaration-order-dependent behavior;
  error groups use conditional structural Eq, Show, Hash, and Json capabilities.
  Normalize derived enum payloads through the ordinary annotation converter so
  nominal wrappers of named error groups retain their checked structural evidence. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Infer contextual outer Option wrapping jointly across call arguments and emit
  the resolved Some layers directly through the centralized kernel representation.
  Report incompatible inference preferences with a source-aware diagnostic. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Allow omission of trailing Option arguments and emit explicit None values at
  direct, higher-order, and piped call sites using solver-provided call metadata. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Support Option propagation with postfix `?` in Option-returning contexts,
  including async bodies and pipe destinations, without implicit Result conversion.
  Wait for recursive peers to constrain unknown propagation carriers before
  generalization, preserving inferred Option contracts across module boundaries. — Thanks @rvcas!

### Patch changes

- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Preserve expected field types through if, match, and block-tail initializers.
  Keep implicit Option wrapping at call and field boundaries, reject conversions
  of existing mutable records, and preserve reachable fallthrough checks. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Reject contradictory field requirements on equivalent parenthesizations of
  record spreads, empty operands, and redundant earlier repeated inputs while preserving
  independent generic merge instantiations and rightmost overwrite priority. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Normalize adjacent closed record spread operands before comparing inferred
  contracts, so grouping fields cannot hide contradictory inherited-field types
  in local or imported functions. Also discard closed fields guaranteed shadowed
  by later closed writes across open spreads when comparing contracts. Preserve
  rightmost writes, other inherited fields, and runtime initializer evaluation.
  Known fields of acyclic open input records also establish guaranteed overwrites;
  opaque cyclic producer obligations do not. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Match constructor applications in generic trait implementation heads,
  including leftmost partial-constructor recovery. Preserve shared constructor
  bindings, prerequisite dictionaries, and overlap detection for recovered sections.
  Honor explicit constructor sections before applying leftmost recovery during
  coherence checking, including sections supplied by later trait arguments. — Thanks @rvcas!
- [23b763c](https://github.com/orbistry/alder/commit/23b763c58372b388f9af9388bd3afc0ae658205d) Respect Boolean short circuits and false match guards when checking loop exits
  and structural divergence. — Thanks @rvcas!
- [3156c57](https://github.com/orbistry/alder/commit/3156c574db10a698f5e48b0f6cc2f2452f380261) Check ordinary record field types across reachable loop exits independently of
  break order, rejecting incompatible result alternatives. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Normalize direct named error-group annotations to structural rows, including
  nested aliases, arrays, records, and stored function signatures. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Propagate expected Option payload types through explicit Some and Option.some
  construction, allowing fresh record fields to receive their contextual Option
  types without converting existing mutable payload aliases. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Keep later pattern alternatives reachable after a failed guard. Check their pin
  exits against loop results, matching the existing per-alternative runtime guard
  retry semantics. — Thanks @rvcas!
- [f7fb26b](https://github.com/orbistry/alder/commit/f7fb26bab952eeb7c574638b5e237e82ebd817ad) Include assignment indices when detecting function suspension, and check their
  early returns and error propagation against the enclosing function's contract. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Select trait implementations for function types by matching arity, parameter
  types, result types, and shared template variables. — Thanks @rvcas!
- [e8ed91a](https://github.com/orbistry/alder/commit/e8ed91a4411928673a776a2f64e2fa8946a5e716) Check implementation bounds against the trait contract and preserve declared method dictionary order, including inherited bounds and renamed type variables. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Report out-of-range tuple reads and writes at the index with the tuple length
  and valid zero-based bounds instead of a misleading unit-type mismatch. — Thanks @rvcas!
- [ea6b284](https://github.com/orbistry/alder/commit/ea6b2849eee22f4332de8319392a1cbe60d7c5d7) Resolve embedded stdlib members through packaged Alder signatures, rejecting unknown members, wrong arguments, invalid callbacks, and arity mismatches. Remove untyped builtin references and correct expected-versus-actual call diagnostics. — Thanks @rvcas!
- [0304ee5](https://github.com/orbistry/alder/commit/0304ee5bf8bf0a6525a371556a86d5be161a54e4) Reject specialization and escape of declared generic function and trait method
  contracts, with source-aware diagnostics for invalid implementations. — Thanks @rvcas!
- [0ddfea3](https://github.com/orbistry/alder/commit/0ddfea32658c54e74da4aa92e583caae15147876) Report invalid stored dependency trait indexes once at build level, preserving
  canonical module identities without attributing errors to unrelated source.
  Reject incoherent registries even when the build contains no source modules. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Reject ordinary types in converted Result error annotations even when the
  function only forwards its argument without constructing or matching a Result.
  Validate unused alias and enum payload declarations too, and report invalid
  error arguments with a source-labeled error-row diagnostic.
  Check bodyless trait signatures, associated-type bindings, and error-group
  payloads without requiring a use site.
  Validate fixed error slots in partial Result constructors and match structural
  error rows when resolving higher-kinded trait implementations. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Compare named Result error groups structurally when detecting overlapping implementations and selecting trait dictionaries, including aliases and fixed error slots in partial constructors. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Check returns and propagation inside match-pin expressions against their
  enclosing function contract, including pins nested inside destructuring patterns. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Track pattern matching, rejection, and pin-expression exits in control-flow
  analysis. Keep pin break values in loop result checking and exclude guards,
  arm bodies, later alternatives, and sibling pattern operands that an earlier
  pin exits before reaching. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Preserve contextual field types inside fresh Option-wrapped record and array
  payloads. Apply ordinary checked-call rules to bare pipe destinations, including
  Option lifting, omitted arguments, and dictionary passing. — Thanks @rvcas!
- [12b09ad](https://github.com/orbistry/alder/commit/12b09ad1f41251491b7a548e9d6f1e16ad320bd4) Validate primitive Json trait decoding against the requested type, preserving
  validation inside derived and container codecs. Encode unit as null and BigInt
  as a decimal JSON string; reject non-finite JSON numbers instead of silently
  encoding them as null. — Thanks @rvcas!
- [677e20f](https://github.com/orbistry/alder/commit/677e20fd158941294eb7688d34180b68cda0e17e) Preserve record-tail identities during inference and publication, check ordinary
  field types at value boundaries, and read stored Option fields directly. — Thanks @rvcas!
- [c9bb0b6](https://github.com/orbistry/alder/commit/c9bb0b6fa425b1b26c5d5c21f2705129ec0e9c34) Fix Option coalescing to return its payload type, preserve unit and nested
  Options, and evaluate the default only for None. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Collect tuple read and write projections before inferring a fixed length, making
  inference independent of projection order and preserving nested alias relations. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Preserve error-row tails when checking trait implementation overlap, rejecting
  open-row instances that could match the same concrete Result constructor. — Thanks @rvcas!
- [80159fd](https://github.com/orbistry/alder/commit/80159fd415bb27186262eb250de7013f3c21efe6) Check payloads hidden in open record spread tails when they can overwrite an earlier property, including Option-valued fields. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Require compatible payload types for alternative match bindings, preventing
  incompatible values escaping through another alternative. — Thanks @rvcas!
- [13079e7](https://github.com/orbistry/alder/commit/13079e73b1ea0756df0323a7bbabcb9fe8841967) Read stored Option fields directly in record destructuring and record-shaped
  enum patterns, preserving nested Option values without presence-based wrapping. — Thanks @rvcas!
- [138d0d0](https://github.com/orbistry/alder/commit/138d0d0f10b38f2d6065ef790a5a46c61b584950) Distinguish normal fallthrough from control-flow exits, reject missing function results after zero-iteration loops, accept returning branches and lambdas without artificial unit values, and preserve loop-tail and break-payload effects in generated JavaScript. — Thanks @rvcas!
- [9c00543](https://github.com/orbistry/alder/commit/9c005437e69c689a3360bdef4aae08daa5c177c1) Preserve universal error-row tails when unifying empty residual rows, allowing
  generic Result identities directly and through transparent aliases. — Thanks @rvcas!
- [d8c4983](https://github.com/orbistry/alder/commit/d8c4983815b570e62b4c3085e309a460f508ce20) Allow type variables in local let annotations and reuse the enclosing callable's
  generic variables consistently with lambda annotations. Preserve generic
  contract checking and monomorphic local bindings. — Thanks @rvcas!
- [b58dec3](https://github.com/orbistry/alder/commit/b58dec3394c2ed7e6a5c5f6d52488a9b75c05876) Accumulate inferred error rows across early returns and resolve mutually
  recursive exact unions without closing externally open error sources. — Thanks @rvcas!
- [f7fb26b](https://github.com/orbistry/alder/commit/f7fb26bab952eeb7c574638b5e237e82ebd817ad) Preserve ordered record-overlay relationships through inference, generalization, and owned interfaces. Keep independent input rows distinct, respect rightmost field overwrites, and check exposed fields against generic contracts. — Thanks @rvcas!
- [69af036](https://github.com/orbistry/alder/commit/69af0361975479f2d18511a46ae37c335cdec9a9) Infer right-biased record spreads using ordinary field types. A later Option field overwrites earlier values even when it contains None. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Allow fresh Err constructors to satisfy contextual multi-tag error rows while checking permitted tags and payload types. Preserve expected types through block result expressions without changing early-return boundaries. — Thanks @rvcas!
- [8be2b83](https://github.com/orbistry/alder/commit/8be2b83a931c55e68518e2cd1bfcb527c98236bc) Preserve all trait parameters in default-method annotation scopes, including
  parameters omitted from a method signature. Reject specialization through local
  annotations and retain the trait's superclass evidence in nested bodies. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Carry reachability across aggregate elements, record fields, tag payloads,
  template interpolations, indexing, and assignment operands so unreachable break
  payloads do not constrain an enclosing loop's result. Include contextually
  checked arrays and records. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Retain and emit inherited default methods when implementing an imported trait,
  including async defaults and constrained dictionary factories. Publish accurate
  default-helper symbols and implementation method metadata, and invalidate stale
  interface caches. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Preserve contextual Option field initialization in records containing spreads.
  Resolve written-field wrapping through ordered field relationships so overwritten
  values retain their own types, later Option fields overwrite earlier values, and inherited
  mutable payloads are not converted. Support nested fresh constructors and arrays. — Thanks @rvcas!
- [edbcd63](https://github.com/orbistry/alder/commit/edbcd633652e4097acc18656bca8cd1b891a219a) Add Array.iter and independent ArrayIterator cursors, replacing the non-advancing
  Iterator instance on arrays. Preserve shared source values, live iteration, and
  correct Option payloads through exhaustion. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Attribute Option lifting failures to a source argument in the failing constraint
  component, preserving its actual types instead of blaming an unrelated earlier
  argument. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Preserve fresh record and array initializer context on pipe inputs, including
  Option-valued fields and optional parameters. Check each input once without
  converting existing mutable aliases or changing runtime evaluation order. — Thanks @rvcas!
- [8055981](https://github.com/orbistry/alder/commit/8055981a5072ecb558f0932c7a2bb9dffa4c0b48) Infer loop expression results from reachable break values, isolate nested loop
  targets, and reject incompatible break payloads. — Thanks @rvcas!
- [d87e721](https://github.com/orbistry/alder/commit/d87e721ad6aa621a26853257257f3f5b2989d3e9) Carry error-row inclusion relationships in canonical and serialized annotations,
  preserving them through arena copies and interface hydration. Bump the interface
  format for the new scheme layout.
  
  Retain inclusion dependencies during solver generalization and instantiation,
  and reject inferred inclusions between independent universal error-row tails.
  
  Resolve concrete error unions for inferred results, including exhaustive matches
  across module boundaries. Preserve explicitly open result contracts and apply
  directional Result return checking consistently to named functions and lambdas. — Thanks @rvcas!
- [a708527](https://github.com/orbistry/alder/commit/a70852762e00f2168b7cb3462ad722cc4393efe1) Keep shared top-level state monomorphic, preserve safe factory polymorphism, and prevent interfaces from turning unresolved shared types into independent generics. Diagnose incomplete shared export types. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Preserve explicitly universal type parameters during contextual Option lifting.
  Insert Some wrappers when required instead of specializing a generic function
  or trait method to an Option-shaped parameter. — Thanks @rvcas!
- [da17cab](https://github.com/orbistry/alder/commit/da17cab2397dca24da7124364a3c9a06c992c4d6) Support lowercase type variables in lambda annotations and share enclosing generic variables and trait bounds without leaking names between sibling scopes. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Check generic call arguments from left to right, including piped arguments,
  so mismatches report the expected type and label the incompatible argument. — Thanks @rvcas!
- [fd795d5](https://github.com/orbistry/alder/commit/fd795d5efec4b19179a2a3fc451fdfc793674a9a) Report deferred error-row constraint failures at the local function reference,
  including calls through imported interfaces, instead of reusing definition
  coordinates against the caller's source. — Thanks @rvcas!
- [f7fb26b](https://github.com/orbistry/alder/commit/f7fb26bab952eeb7c574638b5e237e82ebd817ad) Preserve template interpolation evaluation and string-conversion order across
  later setup statements. Check tagged templates against the actual tag function,
  including argument types and its return type, instead of assuming String.
  Preserve empty tagged-template segments around adjacent interpolations. — Thanks @rvcas!
- [30f6650](https://github.com/orbistry/alder/commit/30f6650e4b737300d115cf89ce8c259c6dc17416) Check record-field assignments against their ordinary stored type, requiring
  Option values for Option fields and rejecting implicit payload lifting or
  traversal through an Option parent. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Preserve record row tails when checking overlapping trait implementations,
  including shared residual-field constraints across trait arguments.
  Match record instance heads during dictionary selection so valid record
  implementations can be called, including across module boundaries. — Thanks @rvcas!
- [f7fb26b](https://github.com/orbistry/alder/commit/f7fb26bab952eeb7c574638b5e237e82ebd817ad) Base top-level generalization on resolved assignments rather than mutation permission flags. Preserve safe polymorphism for never-assigned function bindings while restricting shared replaceable functions, including writes inside nested closures. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Index Option depth constraints by their incident variables instead of rescanning
  all constraints for every independent component. Preserve field initializer
  context in explicitly annotated lambda record returns. — Thanks @rvcas!
- [f7fb26b](https://github.com/orbistry/alder/commit/f7fb26bab952eeb7c574638b5e237e82ebd817ad) Resolve newly closed record overlays after bare-function pipe calls before
  checking field access, preserving the final stored field type like ordinary calls. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Detect cyclic structural error-group expansion and report the recursive source
  reference instead of aborting the compiler with a stack overflow. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Add sparse exact-length tuple constraint metadata to annotations and stored interfaces, preserving arena copies and fingerprint identity with a new interface format. Carry imported constraints through inference and check concrete tuple lengths, constrained elements, and generic contracts.
  
  Finalize source tuple projections without allocating by the largest index, preserve fixed tuple lengths, and reject recursive tuple-element constraints. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Solve consistent direct-match Option depth constraints without dense closure.
  Report impossible constraint components consistently even when another component
  has ambiguous wrapping preferences.
  Preserve inferred record/error-row kinds while selecting contextual Some wrappers. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Require the built-in Result module identity when granting Result.err error-tag construction behavior, rather than accepting similarly named user modules. — Thanks @rvcas!
- Updated dependencies: alder-ast@0.3.0, alder-can@0.4.0, alder-constrain@0.4.0

## 0.3.0 — 2026-09-04

### Minor changes

- [ac24445](https://github.com/orbistry/alder/commit/ac24445101bb7a8d5bef6076ff145b94e103c91a) Add row-typed `Result` errors, inferred and propagated error rows, exhaustive
  error matching, diagnostics, direct AST lowering, and typed JSON failures. — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-can@0.3.0, alder-constrain@0.3.0

## 0.2.1 — 2026-09-03

### Patch changes

- Updated dependencies: alder-can@0.2.1, alder-constrain@0.2.1

## 0.2.0 — 2026-09-03

### Minor changes

- [ff0e0a3](https://github.com/orbistry/alder/commit/ff0e0a303fec4805bef7bfcb2ac002feb09d44e4) Add built-in `Show`, `Eq`, `Ord`, `Hash`, and `Json` enum derives with callable trait methods and direct Oxc AST dictionary lowering. — Thanks @rvcas!
- [36492e6](https://github.com/orbistry/alder/commit/36492e624fd3075fef935ac54d3ab5209fae18a3) Reject trait implementation heads whose constructor kind does not match the trait parameter. — Thanks @rvcas!
- [6e721dc](https://github.com/orbistry/alder/commit/6e721dc86e8cc39fac1f4464eac6f1beea422803) Add the built-in `Iterator` trait, its associated `Item` type, and the initial Array implementation. — Thanks @rvcas!
- [0258ff1](https://github.com/orbistry/alder/commit/0258ff1fef3e279249933be2a3c8e149ad28afcf) Adopt arrow lambdas and juxtaposed function return types, forward piped values
  to the first argument of existing calls, and add `Array.filter` for pipeline
  composition. — Thanks @rvcas!
- [9939d2b](https://github.com/orbistry/alder/commit/9939d2b638073809ba463d7a145f84e33f96a93e) Add kernel-backed `Show`, `Hash`, and `Json` instances for primitives and built-in containers. — Thanks @rvcas!
- [b664174](https://github.com/orbistry/alder/commit/b6641741292b8240922ad7c1ae8822e0edfc16b1) Preseed declared type signatures and trait predicates for every value SCC so mutually recursive calls pass stable dictionary arguments independent of source order. — Thanks @rvcas!
- [7dab530](https://github.com/orbistry/alder/commit/7dab5303f81d1b4d07a9b9c43b6ea3bb6297d11a) Add the built-in `Traversable` trait and Array, Option, and Result implementations with method-level Applicative evidence. — Thanks @rvcas!
- [2b5848b](https://github.com/orbistry/alder/commit/2b5848b8d5b48c8ed3c954010e999a34567970e3) Add built-in `Applicative` and `Monad` traits and Array, Option, and Result implementations. — Thanks @rvcas!
- [7d53578](https://github.com/orbistry/alder/commit/7d53578e5aef1c152bda29fb55181c78fd9af45d) Implement the M2 core-language pipeline through direct Oxc AST generation,
  Rolldown bundling, the embedded standalone runtime, stdlib/kernel foundations,
  formatting, and test execution. — Thanks @rvcas!
- [e45ef4a](https://github.com/orbistry/alder/commit/e45ef4aee1ee5c8dceceb90e9b29ac586394c3a5) Provide automatic structural equality for arrays, options, and results whose type arguments implement `Eq`. — Thanks @rvcas!
- [51c443a](https://github.com/orbistry/alder/commit/51c443ae6d3ac718b0f9b955f84ef33f91b8a3f2) Carry and lower transitive superclass dictionary paths for generic bounds and implementation prerequisites. — Thanks @rvcas!
- [70ab929](https://github.com/orbistry/alder/commit/70ab9292acb31029b43046e525bc9abaaa705e75) Load imported dependency package instance indexes into package-aware builds and deduplicate their implementations by stable identity. — Thanks @rvcas!
- [7f62c2d](https://github.com/orbistry/alder/commit/7f62c2d71e83499bb88a66d0a93f03eb1bd70576) Expose `Eq` and `Ord` superclass dictionaries through built-in `Hash` and `Num`, and return 64-bit `BigInt` hashes. — Thanks @rvcas!
- [5b56b86](https://github.com/orbistry/alder/commit/5b56b86556473042b72d8e2abc6b6471303c1172) Add the built-in `Functor` trait with kernel-backed `Array`, `Option`, and partially applied `Result` instances. — Thanks @rvcas!
- [730c9f3](https://github.com/orbistry/alder/commit/730c9f38d0c06e27dcf4f1084783171b62a25cf7) Validate trait implementation superclasses and emit their resolved dictionary fields. — Thanks @rvcas!
- [4dc0e01](https://github.com/orbistry/alder/commit/4dc0e0118bd1ce68549d110f2499eb5f510739bb) Resolve associated-type equalities to stable trait identities, preserve them in inferred schemes and interfaces, and normalize projections through declared equalities and impl bindings. — Thanks @rvcas!

### Patch changes

- [df4e6ab](https://github.com/orbistry/alder/commit/df4e6ab1176da2149c39bb847d9f8aad597d4e21) Preserve source type-variable names in trait failures and add source-aware
  rendering for unsatisfied bounds, orphan impls, overlaps, kind mismatches, and
  associated-type cycles. Render canonical trait member, associated-type, derive,
  and duplicate errors with precise source labels as well. — Thanks @rvcas!
- [77dda67](https://github.com/orbistry/alder/commit/77dda671c2140f36af781535ecf1f0b289dad92e) Keep mutable and local bindings monomorphic, and subtract type variables held
  by the outer environment when generalizing top-level values. Report unresolved
  trait obligations over non-generalized variables as type ambiguity rather than
  suggesting an impossible generic bound. — Thanks @rvcas!
- [725ee34](https://github.com/orbistry/alder/commit/725ee34ee4e9951913f73d5cc42ca17b542009e2) Load first-party trait and primitive/container instance headers from the
  audited Alder source module `std/Traits.ald`. Builtin instances now participate
  in ordinary database matching and prerequisite resolution before intrinsic
  code-generation evidence is selected. — Thanks @rvcas!
- [838db93](https://github.com/orbistry/alder/commit/838db931669daf8fa6c4ff95e17e964446cd6076) Report contradictory associated-type equalities as a dedicated structured mismatch. — Thanks @rvcas!
- [6a6d7a4](https://github.com/orbistry/alder/commit/6a6d7a466e1bad50fdea3504857298d632f3bd9e) Implement the documented `Ord.compare -> Ordering` dictionary ABI. Generic
  comparison operators now inspect the tagged result, derived ordering composes
  selected field dictionaries through `compare`, and primitive comparisons keep
  their direct JavaScript lowering. — Thanks @rvcas!
- [e1d71c8](https://github.com/orbistry/alder/commit/e1d71c81b96d5afc8b473cf5e7ab0aaf3ed152f0) Resolve and retain trait evidence for every derived payload field, including
  nested builtin containers. Generated Show, Eq, Ord, Hash, and Json dictionaries
  now dispatch through the selected field dictionaries, and dictionary emission
  orders Eq superclasses before their dependents. — Thanks @rvcas!
- [794af50](https://github.com/orbistry/alder/commit/794af5063f5133c39e68ecdb18f34e9b493258fc) Infer record-payload enum constructors as their enum result type while checking each payload field. — Thanks @rvcas!
- [916932b](https://github.com/orbistry/alder/commit/916932b78abe4ac1c414697417d5f5384ec37361) Reject cyclic associated-type bindings with a structured projection-cycle error. — Thanks @rvcas!
- [b699950](https://github.com/orbistry/alder/commit/b699950207423f198107d2872dabf690d84235b3) Add source-aware miette diagnostics for compiler errors and warnings, render
  trait and parser failures with labeled snippets, and preserve Alder source in
  code generation snapshots. — Thanks @rvcas!
- [329bdbe](https://github.com/orbistry/alder/commit/329bdbea92a1ae1f3d4d7669a184c8b1ebfb9009) Make solver evidence obligations consume the stable requirement seeds produced by constraint generation. — Thanks @rvcas!
- [6bd5000](https://github.com/orbistry/alder/commit/6bd500018bfd701ee1d21c864c4534c6c2824bad) Render trait and inference failures as stable user-facing diagnostics with actionable bound guidance. — Thanks @rvcas!
- [11b51d9](https://github.com/orbistry/alder/commit/11b51d983968415374018be19fafd9803f58f774) Retain nested trait obligation chains and render ambiguous implementation
  candidates with all available source locations. — Thanks @rvcas!
- [630f635](https://github.com/orbistry/alder/commit/630f635691158c8def6ca564a4950b6df98eca57) Preserve optional enum payload fields through canonicalization and inference,
  omit them from derived JSON, and accept them as absent when decoding. Render
  derived record-payload constructors in Alder syntax and source field order.
  Derived hashes now include canonical type identity, declaration variant index,
  and every payload field. Derived JSON decoding rejects unexpected envelope and
  payload fields.
  Associated-type validation now rejects indirect projection cycles and reports
  the complete cycle through the structured diagnostic renderer. — Thanks @rvcas!
- Updated dependencies: alder-ast@0.2.0, alder-can@0.2.0, alder-constrain@0.2.0, alder-parse@0.2.0, alder-region@0.2.0, alder-source@0.2.0

