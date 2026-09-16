# alder-driver

## 0.8.1 — 2026-09-16

### Patch changes

- [355ab9b](https://github.com/orbistry/alder/commit/355ab9b80f4681c0976fa23888aa6081ad033e2c) Recognize routes on Windows by normalizing canonical source roots and native
  path separators before route discovery, preserving generated route types. — Thanks @rvcas!
- [dca31d3](https://github.com/orbistry/alder/commit/dca31d3f75ea784108f8d1c10b8e8be7a25a4821) Consolidate runnable projects under `examples/` and use those shared sources in
  compiler regressions and inference benchmarks. — Thanks @rvcas!
- [a9ea854](https://github.com/orbistry/alder/commit/a9ea854d6cebed289229f4e7be47827079995e57) Match canonical Windows source roots to file URL paths during module discovery,
  and reject unsafe archive path separators consistently across platforms. — Thanks @rvcas!
- Updated dependencies: alder-codegen@0.6.1, alder-solve@0.6.1

## 0.8.0 — 2026-09-15

### Minor changes

- [ee01c34](https://github.com/orbistry/alder/commit/ee01c34d1568654083951d3d44a232b624bd5cbb) Implement the M6 web application workflow: checked-in reproducible HTML schema,
  typed components/events/children, compiler-tracked signals and stores, reactive
  directives and keyed reconciliation, resources, escaped SSR, safe hydration
  transport, and lifecycle ownership. Generate filesystem routes, typed params
  and load data, browser navigation, error boundaries, HTTP endpoints, remote
  query/command stubs, typed page actions/text forms, and server/client hooks.
  Enforce server-only reachability and exclude private stores from hydration.
  
  Add standalone and Cloudflare web build/dev adapters, state-preserving HMR and
  recoverable diagnostics, public assets, staged stale-output cleanup, static and
  dynamic prerendering, Cloudflare binding/DO/queue/workflow adapters, and an
  explicit-target deployment path with local dry-run validation. Package pinned
  compiler-owned platform support in native releases and version-proxy installs.
  Support owned Durable Object state and Workflow step wrappers with explicit
  native-handle identity equality and compiled execution coverage.
  Add direct compiler/kernel/routing/bundler/runtime regressions, opt-in browser
  checks, a source-only full example, and documented implementation/acceptance
  boundaries. A real preview deployment remains separately authorized verification,
  not a claim made by this changeset. — Thanks @rvcas!

### Patch changes

- [ee01c34](https://github.com/orbistry/alder/commit/ee01c34d1568654083951d3d44a232b624bd5cbb) Fold ordinary multiline markup text consistently before DOM/SSR code generation,
  preserving inline spaces, explicit string expressions, and whitespace-sensitive
  pre/textarea subtrees. Format nested markup, prose, attributes, directives, and
  embedded blocks with parsed-structure equivalence checks and idempotence tests.
  Add compiled before/after formatting coverage for SSR, DOM, and node-reusing
  hydration, and format the web examples. — Thanks @rvcas!
- Updated dependencies: alder-ast@0.4.0, alder-can@0.7.0, alder-codegen@0.6.0, alder-constrain@0.6.0, alder-parse@0.6.0, alder-solve@0.6.0

## 0.7.0 — 2026-09-11

### Minor changes

- [f001988](https://github.com/orbistry/alder/commit/f001988a97cea7cb6d4b246d59f0e941e96f745b) Remove unused `ModuleMeta`, `InterfaceCache::start_build`, and
  `InterfaceCache::needs_rebuild` APIs that did not participate in source builds.
  Remove the unused error-discarding `load_package_index` convenience method;
  `load_package_index_checked` remains the validated loading API. — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-can@0.6.1, alder-codegen@0.5.1, alder-config@0.2.1, alder-constrain@0.5.2, alder-solve@0.5.2

## 0.6.0 — 2026-09-07

### Minor changes

- [2cc1d05](https://github.com/orbistry/alder/commit/2cc1d057460100bce03b09d5cda3f8e7975f1a87) Unify bundled, local, and external imports with grouped syntax, lowercase
  standard-library namespaces, explicit utility imports, identity-preserving
  namespace re-exports, and canonical comment-preserving formatting. Share public
  interfaces between prelude and explicit imports, reject conflicting bindings,
  and make module initialization independent of import declaration order.
  
  Interface format 8 replaces earlier contracts without compatibility readers. — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-ast@0.3.1, alder-can@0.6.0, alder-codegen@0.5.0, alder-constrain@0.5.1, alder-parse@0.5.0, alder-solve@0.5.1, alder-source@0.4.0

## 0.5.0 — 2026-09-06

### Minor changes

- [52f2954](https://github.com/orbistry/alder/commit/52f29549f4881d0c493332ee8549df1adcf1fbbe) Add consistent Cargo-style CLI statuses, global quiet/verbose/color options,
  elapsed summaries, accurate diagnostic counts, and compiler proxy reporting.
  Expose optional semantic driver progress and an injected CLI renderer while
  preserving source diagnostics, hyperlinks, and program/LSP streams. Add opt-in
  structured runtime test results so CLI test summaries use actual executed counts
  on stderr without intercepting user output or duplicating failure summaries. — Thanks @rvcas!

### Patch changes

- [52732c2](https://github.com/orbistry/alder/commit/52732c2173f38376da3372582de0347b8105a3ac) Preserve structured types in missing-return, invalid Result error-kind, and associated-equality diagnostics so nested types use resolved import names at reporting time. — Thanks @rvcas!
- [6b0010f](https://github.com/orbistry/alder/commit/6b0010f05674b8203029299d717e0c44de267101) Collect invalid declaration error kinds before body inference, preventing dependent type-error cascades. — Thanks @rvcas!
- [8cb8b18](https://github.com/orbistry/alder/commit/8cb8b181878b325ebd12585d03f7b0172d8c7ef1) Check pattern exhaustiveness and redundancy uniformly across enums, Option,
  Result, nested payloads, tuples, records and array prefixes. Reject refutable
  bindings and parameters at compile time, report uncovered patterns and covering
  source locations, and preserve effectful guard and pin behavior. — Thanks @rvcas!
- [8e03ebe](https://github.com/orbistry/alder/commit/8e03ebe56450b5f86574e1a2f5a5651c2d7b5023) Recover independent implementation and trait-default body failures using fresh inference attempts, retaining declaration contracts for diagnostics and suppressing dependent bodies without hiding unrelated methods. — Thanks @rvcas!
- [563041a](https://github.com/orbistry/alder/commit/563041aaef0af67e3c1813ad41235a2bd42c2f1e) Accumulate independent declaration type errors using fresh inference attempts,
  suppress dependent failures, and retain the failed-module publication barrier. — Thanks @rvcas!
- [841eb16](https://github.com/orbistry/alder/commit/841eb160815769e46a830e7366206be112bff138) Keep impossible error-pattern advice within the scrutinee's declared contract rather than recommending a broader error type. — Thanks @rvcas!
- [8dc960c](https://github.com/orbistry/alder/commit/8dc960c1cfd21d9fce64470714dae904d8f2d2b8) Retain available record fields in missing-field diagnostics and offer conservative, unambiguous typo suggestions. — Thanks @rvcas!
- [8c09806](https://github.com/orbistry/alder/commit/8c098064375e1f07292f89a8dba4288fcb35e12b) Keep enclosing function, tuple and applied type comparisons in mismatch diagnostics, including deferred Option payload checks and normalized associated types. — Thanks @rvcas!
- [30635be](https://github.com/orbistry/alder/commit/30635be9554d9419a6fae026b0a01f88be6726b2) Retain structured recursive type equations for occurs-check failures and explain
  why structural cycles cannot have finite types. Use shared readable variable
  names and preserve call, branch and array-element diagnostic context. — Thanks @rvcas!
- [ea12495](https://github.com/orbistry/alder/commit/ea1249529f95314799f5241bc725bc9d7541a65d) Preserve owned nominal identities in diagnostic types and render consumer import names, re-export aliases, or explicit module/package provenance instead of ambiguous short names. — Thanks @rvcas!
- [db5af22](https://github.com/orbistry/alder/commit/db5af22dcae75be7c815d00a3a329fb2a5578d48) Preserve structured mismatch types until diagnostic rendering, including distinct generic variables and open record/error rows. — Thanks @rvcas!
- [5adcd76](https://github.com/orbistry/alder/commit/5adcd76c4e07726d71ba42a6566fe3901c1a6df6) Report independent statement type errors within a function using isolated retries, suppressing invalid local dependencies and delivering the results to CLI and editor consumers. — Thanks @rvcas!
- [0b70ba7](https://github.com/orbistry/alder/commit/0b70ba754c594fecfb7fe7344e77ffb32f2ffe3f) Generate unused-local and parameter warnings from resolved source bindings,
  respecting shadowing, alternatives, captures, pins, guards, and writes. Explain
  safe pattern discards without recommending removal of needed initializer effects. — Thanks @rvcas!
- [6590795](https://github.com/orbistry/alder/commit/65907955dca4efc84622b5e72219274763826e95) Generate unused import-binding warnings through resolved source lookups, respecting
  aliases, shadowing, type and trait uses, and public re-exports. Preserve initializer
  effects and source-order diagnostic delivery in check, build, and test commands. — Thanks @rvcas!
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
- [00451d1](https://github.com/orbistry/alder/commit/00451d103570fbec7a31237cb86d2fbc01a0b1d0) Label the earlier local requirement when associated-type equalities conflict. — Thanks @rvcas!
- [fe6e840](https://github.com/orbistry/alder/commit/fe6e840c87737e69bcfb3ce033bc274d6f933964) Retain pre-whitespace/comment boundaries in shared delimiter errors, including
  parser backtracking. Report cross-line missing punctuation at the insertion
  boundary and label the actual opening punctuation, not the enclosing declaration.
  Keep detection evidence internal rather than labeling valid following code, retain
  actual mismatched closer locations, and distinguish required separators from list
  entries in CLI/editor messages. Delimiter error variants now carry ExpectedEnd
  instead of a row/column pair; error-row extension and non-select query closers have
  separate variants so their messages do not offer invalid continuations. — Thanks @rvcas!
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
- [9893f6c](https://github.com/orbistry/alder/commit/9893f6cf59467b63bd8371bd0d2944a319f7988f) Suppress cascading importer diagnostics after a source module fails, including
  transitive public re-exports. Failed builds return no executable artifacts,
  interfaces, or partial package indexes, including consumers of invalid sibling
  implementation bodies. — Thanks @rvcas!
- [9e410e9](https://github.com/orbistry/alder/commit/9e410e938fccf5630c83e243b1421b9bf742a29c) Retain return annotation locations for explicit returns and lambda tails, and
  highlight the returned expression. Isolate annotation origins at nested lambda
  and async boundaries so diagnostics do not blame an outer function's signature. — Thanks @rvcas!
- [effb878](https://github.com/orbistry/alder/commit/effb878c6a279ad19eb2f77feae8f57724ea1416) Warn on unused module value bindings using resolved references and conservative
  reachability. Respect exports, recursion, shadowing, entry points, tests and
  method bodies, while retaining side-effectful initializers and their helpers. — Thanks @rvcas!
- [51a458d](https://github.com/orbistry/alder/commit/51a458d94bb965f8d59121b70af0114339572456) Preserve return-annotation context for early record type comparisons, including transparent aliases and explicit returns, without relabeling unrelated projection failures. — Thanks @rvcas!
- [a63a5e0](https://github.com/orbistry/alder/commit/a63a5e0be9ae9437dcd8b60e615cce5ca8fe1afb) Label written return annotations in missing-return diagnostics, including aliases and async functions, without inventing annotation origins for inferred results. — Thanks @rvcas!
- [875607f](https://github.com/orbistry/alder/commit/875607f62375e37441f8a9522e97b8955fa443ef) Explain immediate type expectations with contextual source labels and originating annotations, and report call arity with argument counts. — Thanks @rvcas!
- Updated dependencies: alder-can@0.5.0, alder-codegen@0.4.1, alder-constrain@0.5.0, alder-parse@0.4.0, alder-report@0.3.0, alder-solve@0.5.0

## 0.4.0 — 2026-09-06

### Minor changes

- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Treat optional record-field shorthand as an ordinary Option type and materialize omitted Option fields as None during contextual record construction.
  
  Remove separate record-field optionality from serialized interfaces and invalidate the previous interface format.
  
  Read Fiber traversal concurrency options using ordinary Option semantics so omitted and explicit None fields both select the sequential default.
  
  Preserve known generic Result error rows during propagation, including explicit Option fallback branches.
  
  Provide conditional Option ordering and Unit ordering so derived record payloads use ordinary field dictionaries, including nested Options. — Thanks @rvcas!
- [0ddfea3](https://github.com/orbistry/alder/commit/0ddfea32658c54e74da4aa92e583caae15147876) Report invalid stored dependency trait indexes once at build level, preserving
  canonical module identities without attributing errors to unrelated source.
  Reject incoherent registries even when the build contains no source modules. — Thanks @rvcas!
- [f87e540](https://github.com/orbistry/alder/commit/f87e540113feb6c1052b2d782a7ad6e1faa7d204) Add inferred lazy tasks, generator-based async lowering, Promise extern lifting,
  and a structured fiber runtime with interruption, scopes, finalizers, `all`, and
  `race`. — Thanks @rvcas!
- [9c16136](https://github.com/orbistry/alder/commit/9c161364551c7f921bef5c7c3394349ac50e328a) Require explicit package and source-relative identity metadata for every source
  module. Remove URI-based identity guesses and metadata-free driver entry points;
  report missing metadata deterministically before publishing compiler artifacts. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Add sparse exact-length tuple constraint metadata to annotations and stored interfaces, preserving arena copies and fingerprint identity with a new interface format. Carry imported constraints through inference and check concrete tuple lengths, constrained elements, and generic contracts.
  
  Finalize source tuple projections without allocating by the largest index, preserve fixed tuple lengths, and reject recursive tuple-element constraints. — Thanks @rvcas!
- [0ddfea3](https://github.com/orbistry/alder/commit/0ddfea32658c54e74da4aa92e583caae15147876) Stop body compilation after source-package coherence failures, reporting errors
  at their defining modules and marking other modules blocked. Avoid phantom
  source labels for overlapping implementations declared in different modules. — Thanks @rvcas!

### Patch changes

- [470e3ef](https://github.com/orbistry/alder/commit/470e3ef31e57097de46903b6a2c9513af3410c1c) Parse named optional parameter annotations and canonicalize their shorthand to
  ordinary builtin Option types, including lambda and trait signatures. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Apply contextual recursive Option lifting to fresh record field initializers,
  including direct record return contexts, without converting existing mutable
  record aliases. Emit field wrapping through the centralized Option helpers. — Thanks @rvcas!
- [0b1add3](https://github.com/orbistry/alder/commit/0b1add3a764ce6e4e537394b84f33cd7bee94235) Keep workspace applications in distinct module namespaces using stable member-specific identities, preventing collisions between local imports, generated modules, and cached interfaces. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Normalize adjacent closed record spread operands before comparing inferred
  contracts, so grouping fields cannot hide contradictory inherited-field types
  in local or imported functions. Also discard closed fields guaranteed shadowed
  by later closed writes across open spreads when comparing contracts. Preserve
  rightmost writes, other inherited fields, and runtime initializer evaluation.
  Known fields of acyclic open input records also establish guaranteed overwrites;
  opaque cyclic producer obligations do not. — Thanks @rvcas!
- [f7fb26b](https://github.com/orbistry/alder/commit/f7fb26bab952eeb7c574638b5e237e82ebd817ad) Permit type-checked reassignment and field/index writes through ordinary let bindings and function or lambda parameters. Keep assignment-aware generalization restrictions on shared replaceable values.
  
  Remove obsolete mutability fields from canonical lets, parameters, and assignment places. Local pattern bindings are writable; non-storage references retain assignment-target checks with diagnostics that no longer suggest adding `mut`.
  
  Remove `mut` from the grammar, keyword list, and source AST. Parsing uses the current grammar without compatibility handling or migration diagnostics. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Report out-of-range tuple reads and writes at the index with the tuple length
  and valid zero-based bounds instead of a misleading unit-type mismatch. — Thanks @rvcas!
- [54d1b63](https://github.com/orbistry/alder/commit/54d1b636097930bb115607e6b11b391954927339) Carry physical source origins alongside generated ASTs so local JavaScript extern modules resolve beside their Alder declarations. Preserve virtual module identities after AST transfer, order bundle inputs deterministically, and verify Promise fulfillment, foreign defects, and cancellation through local wrappers. — Thanks @rvcas!
- [9b47b90](https://github.com/orbistry/alder/commit/9b47b901ab68583292cdaf7e2574075eba85222c) Separate interface and instance-index cache paths by package identity kind so
  application modules, workspace members, named packages, and builtins cannot
  overwrite one another's artifacts. Remove the unused unqualified cache lookup. — Thanks @rvcas!
- [0304ee5](https://github.com/orbistry/alder/commit/0304ee5bf8bf0a6525a371556a86d5be161a54e4) Reject specialization and escape of declared generic function and trait method
  contracts, with source-aware diagnostics for invalid implementations. — Thanks @rvcas!
- [8918255](https://github.com/orbistry/alder/commit/8918255ba17b27d7ae2c62db2d89cc13ce9fe4db) Make dependency ordering, compilation depth groups, and import-cycle selection deterministic regardless of module discovery and import order. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Reject ordinary types in converted Result error annotations even when the
  function only forwards its argument without constructing or matching a Result.
  Validate unused alias and enum payload declarations too, and report invalid
  error arguments with a source-labeled error-row diagnostic.
  Check bodyless trait signatures, associated-type bindings, and error-group
  payloads without requiring a use site.
  Validate fixed error slots in partial Result constructors and match structural
  error rows when resolving higher-kinded trait implementations. — Thanks @rvcas!
- [0ddfea3](https://github.com/orbistry/alder/commit/0ddfea32658c54e74da4aa92e583caae15147876) Locate trait implementation diagnostic labels by implementation identity so imports and omitted header-pass declarations cannot shift their source spans. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Reject custom trait implementations directly targeting named error groups or
  their transparent aliases, with a source-aware structural-row diagnostic. — Thanks @rvcas!
- [2971f2a](https://github.com/orbistry/alder/commit/2971f2ac82ab548eb91228568b60a8d1063909de) Preserve external application-member identities when a workspace and its sibling members are relocated together. — Thanks @rvcas!
- [08c77e5](https://github.com/orbistry/alder/commit/08c77e55441ccd3a3ca944c7a5425a6139ac41f8) Reject imported types and traits bound under the same name, consistently with
  local declarations, including wildcard and renamed public re-exports. Preserve
  distinct aliases and report both conflicting source imports. — Thanks @rvcas!
- [0db756d](https://github.com/orbistry/alder/commit/0db756d7014df5c506fafe13df0b93306896d1fa) Represent explicit async function declarations and lazy async block syntax. — Thanks @rvcas!
- [41f55b6](https://github.com/orbistry/alder/commit/41f55b6201ca3abebac3fafd4e88ff0031d627a7) Retain Alder source and extern declaration regions through bundling, render unresolved externs as labeled shared diagnostics, and verify sibling JavaScript wrapper resolution across path-dependency packages. — Thanks @rvcas!
- [daef79b](https://github.com/orbistry/alder/commit/daef79b53ec5d3249cef75704a1ed3d2f99c06fb) Reject recursive type-alias dependencies during canonicalization and report a
  source-aware diagnostic explaining how to represent recursive data with enums.
  Expand local and imported alias references with instantiated canonical targets,
  including generic arguments and ordinary record field types. — Thanks @rvcas!
- [32f5ff4](https://github.com/orbistry/alder/commit/32f5ff4672ede60400b057cf54cd9cccddfcbfb7) Reject duplicate module identities before publishing interfaces or code, with diagnostics for both source files. Resolve CLI project imports and canonical module paths using package identity and actual source roots, and resolve package-root imports to mod.ald. — Thanks @rvcas!
- [adb35ac](https://github.com/orbistry/alder/commit/adb35acf5e70ddd26ca44e1c21771af88e3f70ac) Resolve nested workspace source files against their most specific containing source root, keeping package identity and local imports independent of member discovery order. — Thanks @rvcas!
- [138d0d0](https://github.com/orbistry/alder/commit/138d0d0f10b38f2d6065ef790a5a46c61b584950) Distinguish normal fallthrough from control-flow exits, reject missing function results after zero-iteration loops, accept returning branches and lambdas without artificial unit values, and preserve loop-tail and break-payload effects in generated JavaScript. — Thanks @rvcas!
- [6b4af4c](https://github.com/orbistry/alder/commit/6b4af4ce186ce4539bcde262161d4b6f67a39f0d) Reject interface-only dependency caches whose package instance index disagrees
  with the implementation headers in their module interfaces, even when each file
  has a valid fingerprint. — Thanks @rvcas!
- [f7fb26b](https://github.com/orbistry/alder/commit/f7fb26bab952eeb7c574638b5e237e82ebd817ad) Preserve ordered record-overlay relationships through inference, generalization, and owned interfaces. Keep independent input rows distinct, respect rightmost field overwrites, and check exposed fields against generic contracts. — Thanks @rvcas!
- [cb89192](https://github.com/orbistry/alder/commit/cb89192e12cf6d2d57b92ba023c8a3302370155d) Reject distinct workspace roots declaring the same package name before module
  discovery, with a deterministic diagnostic identifying both roots. Repeated
  paths to the same physical package remain one member. — Thanks @rvcas!
- [743a09c](https://github.com/orbistry/alder/commit/743a09c81ed6f65f5bddd21f7ebd853cf8350e18) Require alternative match patterns to bind identical names and share canonical
  binding identities, with source-aware diagnostics for missing or extra names. — Thanks @rvcas!
- [14be079](https://github.com/orbistry/alder/commit/14be07997423e789b7a0f52954a1f709fcfa7afe) Reject overflowing tuple indices with a source diagnostic instead of silently
  changing them to the largest representable index. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Infer contextual outer Option wrapping jointly across call arguments and emit
  the resolved Some layers directly through the centralized kernel representation.
  Report incompatible inference preferences with a source-aware diagnostic. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Retain and emit inherited default methods when implementing an imported trait,
  including async defaults and constrained dictionary factories. Publish accurate
  default-helper symbols and implementation method metadata, and invalidate stale
  interface caches. — Thanks @rvcas!
- [209141d](https://github.com/orbistry/alder/commit/209141d018dd734c5294da337c763128ea362224) Discover transitive source dependencies using each dependency project's own
  manifest and path base, without requiring prebuilt semantic caches.
  Reject distinct dependency roots claiming one package identity, while coalescing
  equivalent paths to the same canonical root.
  Resolve workspace imports against their owning member's dependency declarations,
  without activating unused sibling dependencies. — Thanks @rvcas!
- [0ddfea3](https://github.com/orbistry/alder/commit/0ddfea32658c54e74da4aa92e583caae15147876) Label a locally declared trait when reporting superclass cycles spanning modules,
  instead of falling back to the start of the file for a foreign trait. — Thanks @rvcas!
- [cc5d41e](https://github.com/orbistry/alder/commit/cc5d41edbddcd232512dc9148f0acf13bf28a79d) Build source-backed dependencies from current source without mixing in saved
  interfaces or instance indexes, so removed trait implementations cannot remain
  available to consumers through stale caches. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Attribute Option lifting failures to a source argument in the failing constraint
  component, preserving its actual types instead of blaming an unrelated earlier
  argument. — Thanks @rvcas!
- [d87e721](https://github.com/orbistry/alder/commit/d87e721ad6aa621a26853257257f3f5b2989d3e9) Carry error-row inclusion relationships in canonical and serialized annotations,
  preserving them through arena copies and interface hydration. Bump the interface
  format for the new scheme layout.
  
  Retain inclusion dependencies during solver generalization and instantiation,
  and reject inferred inclusions between independent universal error-row tails.
  
  Resolve concrete error unions for inferred results, including exhaustive matches
  across module boundaries. Preserve explicitly open result contracts and apply
  directional Result return checking consistently to named functions and lambdas. — Thanks @rvcas!
- [a708527](https://github.com/orbistry/alder/commit/a70852762e00f2168b7cb3462ad722cc4393efe1) Keep shared top-level state monomorphic, preserve safe factory polymorphism, and prevent interfaces from turning unresolved shared types into independent generics. Diagnose incomplete shared export types. — Thanks @rvcas!
- [fd795d5](https://github.com/orbistry/alder/commit/fd795d5efec4b19179a2a3fc451fdfc793674a9a) Report deferred error-row constraint failures at the local function reference,
  including calls through imported interfaces, instead of reusing definition
  coordinates against the caller's source. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Support Option propagation with postfix `?` in Option-returning contexts,
  including async bodies and pipe destinations, without implicit Result conversion.
  Wait for recursive peers to constrain unknown propagation carriers before
  generalization, preserving inferred Option contracts across module boundaries. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Detect cyclic structural error-group expansion and report the recursive source
  reference instead of aborting the compiler with a stack overflow. — Thanks @rvcas!
- [ca9ab0e](https://github.com/orbistry/alder/commit/ca9ab0eb46454b5407d43eabb79a517cb679aa38) Canonicalize and deduplicate workspace member roots so repeated patterns, parent-path spellings, and symlink aliases cannot compile one source tree under multiple identities. — Thanks @rvcas!
- [749ba31](https://github.com/orbistry/alder/commit/749ba31c566f66ced41107e9591da82ddd329370) Publish public named and wildcard imports in module interfaces, preserving
  original identities and checked schemes including multiple aliases of one name.
  Interface construction APIs now require dependency interfaces; the driver threads
  them through header and final publication. — Thanks @rvcas!
- Updated dependencies: alder-ast@0.3.0, alder-can@0.4.0, alder-codegen@0.4.0, alder-constrain@0.4.0, alder-parse@0.3.0, alder-solve@0.4.0, alder-source@0.3.0

## 0.3.0 — 2026-09-04

### Minor changes

- [ac24445](https://github.com/orbistry/alder/commit/ac24445101bb7a8d5bef6076ff145b94e103c91a) Add row-typed `Result` errors, inferred and propagated error rows, exhaustive
  error matching, diagnostics, direct AST lowering, and typed JSON failures. — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-can@0.3.0, alder-codegen@0.3.0, alder-constrain@0.3.0, alder-solve@0.3.0

## 0.2.1 — 2026-09-03

### Patch changes

- Updated dependencies: alder-can@0.2.1, alder-codegen@0.2.1, alder-constrain@0.2.1, alder-solve@0.2.1

## 0.2.0 — 2026-09-03

### Minor changes

- [7757405](https://github.com/orbistry/alder/commit/7757405a85dbdd52f0a4d3109df65dfd2b34414a) Preserve implementation source locations across semantic interfaces and add validated package-instance-index persistence and hydration. — Thanks @rvcas!
- [2278da9](https://github.com/orbistry/alder/commit/2278da9f8e58a1c12f6bfdf0723af001c669fc41) Produce and persist solved module interfaces and deduplicated package instance indexes after successful builds. — Thanks @rvcas!
- [28c93de](https://github.com/orbistry/alder/commit/28c93de0f460222eb04001942292c875f7329dfb) Replace the legacy export-name cache with versioned semantic interface and
  package-instance-index files. Preserve complete trait, associated-type,
  dictionary, and public type metadata, validate hydration round trips, and use
  canonical SHA-256 fingerprints. — Thanks @rvcas!
- [7d53578](https://github.com/orbistry/alder/commit/7d53578e5aef1c152bda29fb55181c78fd9af45d) Implement the M2 core-language pipeline through direct Oxc AST generation,
  Rolldown bundling, the embedded standalone runtime, stdlib/kernel foundations,
  formatting, and test execution. — Thanks @rvcas!
- [b699950](https://github.com/orbistry/alder/commit/b699950207423f198107d2872dabf690d84235b3) Add source-aware miette diagnostics for compiler errors and warnings, render
  trait and parser failures with labeled snippets, and preserve Alder source in
  code generation snapshots. — Thanks @rvcas!
- [9789f23](https://github.com/orbistry/alder/commit/9789f237bd68714c67bffafa7e8836fc9a731a7f) Compile imported path-dependency sources into the same in-memory Oxc/Rolldown
  module graph, allowing dictionary factories selected from unimported sibling
  modules to bundle and execute without serializing generated JavaScript. — Thanks @rvcas!
- [70ab929](https://github.com/orbistry/alder/commit/70ab9292acb31029b43046e525bc9abaaa705e75) Load imported dependency package instance indexes into package-aware builds and deduplicate their implementations by stable identity. — Thanks @rvcas!

### Patch changes

- [b49583d](https://github.com/orbistry/alder/commit/b49583dbcb4896846de69824e3a235df6ec77315) Deep-copy solved interfaces across arena boundaries so every source module can
  release its parser, canonical, constraint, and solver allocations immediately
  after compilation. — Thanks @rvcas!
- [df4e6ab](https://github.com/orbistry/alder/commit/df4e6ab1176da2149c39bb847d9f8aad597d4e21) Preserve source type-variable names in trait failures and add source-aware
  rendering for unsatisfied bounds, orphan impls, overlaps, kind mismatches, and
  associated-type cycles. Render canonical trait member, associated-type, derive,
  and duplicate errors with precise source labels as well. — Thanks @rvcas!
- [77dda67](https://github.com/orbistry/alder/commit/77dda671c2140f36af781535ecf1f0b289dad92e) Keep mutable and local bindings monomorphic, and subtract type variables held
  by the outer environment when generalizing top-level values. Report unresolved
  trait obligations over non-generalized variables as type ambiguity rather than
  suggesting an impossible generic bound. — Thanks @rvcas!
- [70ce3d4](https://github.com/orbistry/alder/commit/70ce3d413172bbfaf11865e1bef9e6cad2450d6c) Retry module solving as sibling interfaces become available so package instances do not depend on graph-level build order. — Thanks @rvcas!
- [c52cdaf](https://github.com/orbistry/alder/commit/c52cdafeac8e9823dd1aa71b5883b8bad8997cbd) Render trait and impl signature parser failures from their innermost function,
  parameter, and type errors with syntax-specific miette labels and help. — Thanks @rvcas!
- [9f2343d](https://github.com/orbistry/alder/commit/9f2343d6d372b0d798d3b30e5f408d5beb8d81f3) Render superclass, invalid-termination, and defensive instance-cycle diagnostics
  with source snippets, and point local superclass cycles at the closing trait. — Thanks @rvcas!
- [3af5769](https://github.com/orbistry/alder/commit/3af576961e371c3a993f4e6176c36725e6f471b0) Canonicalize package trait headers independently of value and method bodies,
  then compile every module against one frozen package-wide header closure.
  Coherence and sibling-instance behavior no longer depend on source order or on
  whether another module's body type-checks. — Thanks @rvcas!
- [6bd5000](https://github.com/orbistry/alder/commit/6bd500018bfd701ee1d21c864c4534c6c2824bad) Render trait and inference failures as stable user-facing diagnostics with actionable bound guidance. — Thanks @rvcas!
- [11b51d9](https://github.com/orbistry/alder/commit/11b51d983968415374018be19fafd9803f58f774) Retain nested trait obligation chains and render ambiguous implementation
  candidates with all available source locations. — Thanks @rvcas!
- [7193977](https://github.com/orbistry/alder/commit/7193977e280cc2feb7d2beee160419ff3e552e9a) Reject orphan implementations during canonicalization while retaining the
  package-wide coherence check for imported metadata. — Thanks @rvcas!
- Updated dependencies: alder-ast@0.2.0, alder-can@0.2.0, alder-codegen@0.2.0, alder-config@0.2.0, alder-constrain@0.2.0, alder-parse@0.2.0, alder-region@0.2.0, alder-report@0.2.0, alder-solve@0.2.0, alder-source@0.2.0

