# alder-cli

## 0.5.1 — 2026-09-11

### Patch changes

- [f001988](https://github.com/orbistry/alder/commit/f001988a97cea7cb6d4b246d59f0e941e96f745b) Use one options-aware command execution path and remove the redundant compile
  wrapper and constant persistence flag. Remove subprocess-based CLI end-to-end
  tests and their test-only dependencies; retain direct compiler, runtime, and
  reporting renderer tests. — Thanks @rvcas!
- Updated dependencies: alder-bundle@0.4.1, alder-config@0.2.1, alder-driver@0.7.0, alder-language-server@0.2.2

## 0.5.0 — 2026-09-07

### Minor changes

- [2cc1d05](https://github.com/orbistry/alder/commit/2cc1d057460100bce03b09d5cda3f8e7975f1a87) Unify bundled, local, and external imports with grouped syntax, lowercase
  standard-library namespaces, explicit utility imports, identity-preserving
  namespace re-exports, and canonical comment-preserving formatting. Share public
  interfaces between prelude and explicit imports, reject conflicting bindings,
  and make module initialization independent of import declaration order.
  
  Interface format 8 replaces earlier contracts without compatibility readers. — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-bundle@0.4.0, alder-driver@0.6.0, alder-fmt@0.3.0, alder-language-server@0.2.1

## 0.4.0 — 2026-09-06

### Minor changes

- [52f2954](https://github.com/orbistry/alder/commit/52f29549f4881d0c493332ee8549df1adcf1fbbe) Add consistent Cargo-style CLI statuses, global quiet/verbose/color options,
  elapsed summaries, accurate diagnostic counts, and compiler proxy reporting.
  Expose optional semantic driver progress and an injected CLI renderer while
  preserving source diagnostics, hyperlinks, and program/LSP streams. Add opt-in
  structured runtime test results so CLI test summaries use actual executed counts
  on stderr without intercepting user output or duplicating failure summaries. — Thanks @rvcas!

### Patch changes

- [5adcd76](https://github.com/orbistry/alder/commit/5adcd76c4e07726d71ba42a6566fe3901c1a6df6) Report independent statement type errors within a function using isolated retries, suppressing invalid local dependencies and delivering the results to CLI and editor consumers. — Thanks @rvcas!
- [166922e](https://github.com/orbistry/alder/commit/166922ed53cd591e6b647fc6d03e168961b8d8dc) Display compiler errors and warnings with project-relative source paths in the
  CLI, including related and bundler diagnostics, without changing editor file
  identities or diagnostic locations. Supported terminals receive explicit absolute
  file hyperlinks behind the short labels so navigation does not depend on the
  shell's working directory. — Thanks @rvcas!
- [6590795](https://github.com/orbistry/alder/commit/65907955dca4efc84622b5e72219274763826e95) Generate unused import-binding warnings through resolved source lookups, respecting
  aliases, shadowing, type and trait uses, and public re-exports. Preserve initializer
  effects and source-order diagnostic delivery in check, build, and test commands. — Thanks @rvcas!
- [2802ca8](https://github.com/orbistry/alder/commit/2802ca86466b50ca2ad33a300c1b452220a4df7b) Publish real compiler errors and warnings for versioned unsaved documents,
  recheck dependents, and clear stale editor diagnostics. Preserve UTF-16 source
  ranges and secondary requirement locations. Fix incomplete stdio responses by
  using the native Tokio language-server transport. — Thanks @rvcas!
- Updated dependencies: alder-bundle@0.3.1, alder-driver@0.5.0, alder-fmt@0.2.2, alder-language-server@0.2.0, alder-runtime@0.3.0

## 0.3.0 — 2026-09-06

### Minor changes

- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Treat optional record-field shorthand as an ordinary Option type and materialize omitted Option fields as None during contextual record construction.
  
  Remove separate record-field optionality from serialized interfaces and invalidate the previous interface format.
  
  Read Fiber traversal concurrency options using ordinary Option semantics so omitted and explicit None fields both select the sequential default.
  
  Preserve known generic Result error rows during propagation, including explicit Option fallback branches.
  
  Provide conditional Option ordering and Unit ordering so derived record payloads use ordinary field dictionaries, including nested Options. — Thanks @rvcas!
- [f87e540](https://github.com/orbistry/alder/commit/f87e540113feb6c1052b2d782a7ad6e1faa7d204) Add inferred lazy tasks, generator-based async lowering, Promise extern lifting,
  and a structured fiber runtime with interruption, scopes, finalizers, `all`, and
  `race`. — Thanks @rvcas!

### Patch changes

- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Capture reached pattern payloads before later pins can mutate their source,
  preserving checked binding types, array-rest values, and alias identity. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Provide structural Json codecs for closed error rows with Json-capable payloads,
  independent of group names and declaration order. — Thanks @rvcas!
- [36e654e](https://github.com/orbistry/alder/commit/36e654e810d0145995ec464e01894e24d23a9ddf) Wait for owned child cleanup in scope, all, and race before delivering cancellation
  to the caller, including cancellation during winner/failure cleanup. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Preserve expected field types through if, match, and block-tail initializers.
  Keep implicit Option wrapping at call and field boundaries, reject conversions
  of existing mutable records, and preserve reachable fallthrough checks. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Reject contradictory field requirements on equivalent parenthesizations of
  record spreads, empty operands, and redundant earlier repeated inputs while preserving
  independent generic merge instantiations and rightmost overwrite priority. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Normalize direct named error-group annotations to structural rows, including
  nested aliases, arrays, records, and stored function signatures. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Share lazy child dictionaries between container Hash and its Eq superclass,
  preventing exponential emitted-code growth while preserving equality dispatch. — Thanks @rvcas!
- [a0b9a41](https://github.com/orbistry/alder/commit/a0b9a41b148b86bf6f93567a56ed6d7c98227410) Check refutable patterns before binding payloads in lets, parameters, and loops.
  Failed bindings now report a source-located match failure instead of exposing
  values that violate their checked types, including inside lazy async tasks. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Propagate expected Option payload types through explicit Some and Option.some
  construction, allowing fresh record fields to receive their contextual Option
  types without converting existing mutable payload aliases. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Keep later pattern alternatives reachable after a failed guard. Check their pin
  exits against loop results, matching the existing per-alternative runtime guard
  retry semantics. — Thanks @rvcas!
- [13e97ef](https://github.com/orbistry/alder/commit/13e97ef55e34c6092020aaa8a873940d6de07337) Reject JSON Result envelopes with a missing payload before invoking a custom
  payload decoder, preserving the decoder's String argument contract. — Thanks @rvcas!
- [743a09c](https://github.com/orbistry/alder/commit/743a09c81ed6f65f5bddd21f7ebd853cf8350e18) Resolve pinned expressions against the enclosing scope before pattern bindings
  are introduced, preventing field-order-dependent lookup and uninitialized
  local references when patterns shadow an outer name. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Report out-of-range tuple reads and writes at the index with the tuple length
  and valid zero-based bounds instead of a misleading unit-type mismatch. — Thanks @rvcas!
- [54d1b63](https://github.com/orbistry/alder/commit/54d1b636097930bb115607e6b11b391954927339) Carry physical source origins alongside generated ASTs so local JavaScript extern modules resolve beside their Alder declarations. Preserve virtual module identities after AST transfer, order bundle inputs deterministically, and verify Promise fulfillment, foreign defects, and cancellation through local wrappers. — Thanks @rvcas!
- [9b47b90](https://github.com/orbistry/alder/commit/9b47b901ab68583292cdaf7e2574075eba85222c) Separate interface and instance-index cache paths by package identity kind so
  application modules, workspace members, named packages, and builtins cannot
  overwrite one another's artifacts. Remove the unused unqualified cache lookup. — Thanks @rvcas!
- [83c2be3](https://github.com/orbistry/alder/commit/83c2be3acb3e9216511e9e91168a8365df08e992) Add lazy Ref cells with synchronous atomic reads, writes, updates, and modifications. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Provide structural Show for closed error rows when every payload supports Show,
  independent of named error groups and their declaration order. — Thanks @rvcas!
- [0ddfea3](https://github.com/orbistry/alder/commit/0ddfea32658c54e74da4aa92e583caae15147876) Report invalid stored dependency trait indexes once at build level, preserving
  canonical module identities without attributing errors to unrelated source.
  Reject incoherent registries even when the build contains no source modules. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Expose Fiber.map, forEach, tryMap, and tryForEach with optional MapOptions,
  execution-time concurrency validation, and sequential defaults. — Thanks @rvcas!
- [9c16136](https://github.com/orbistry/alder/commit/9c161364551c7f921bef5c7c3394349ac50e328a) Require explicit package and source-relative identity metadata for every source
  module. Remove URI-based identity guesses and metadata-free driver entry points;
  report missing metadata deterministically before publishing compiler artifacts. — Thanks @rvcas!
- [b8924f2](https://github.com/orbistry/alder/commit/b8924f26405306239a1e33b87592a1e61eeb3fcc) Make equality inherited through primitive Hash dictionaries agree with ordinary
  equality, including signed zero and NaN through nested containers and derives. — Thanks @rvcas!
- [10379a9](https://github.com/orbistry/alder/commit/10379a92b002824f382e96743b9417bfd5dfed8b) Detect active cycles in derived value operations: Show emits `<cycle>`, while
  Hash, Ord, and JSON encoding raise explicit TypeError defects. Clear active
  tracking after success or exceptions and preserve shared acyclic values. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Resolve recursive derived dictionary references through nested container and
  structural evidence, preventing undefined self bindings in emitted equality,
  showing, and hashing functions. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Check returns and propagation inside match-pin expressions against their
  enclosing function contract, including pins nested inside destructuring patterns. — Thanks @rvcas!
- [2971f2a](https://github.com/orbistry/alder/commit/2971f2ac82ab548eb91228568b60a8d1063909de) Preserve external application-member identities when a workspace and its sibling members are relocated together. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Delegate every declared record field to its ordinary equality, ordering,
  hashing, and JSON dictionary. Preserve nested Options and unit payloads, and
  decode missing Option fields as None. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Track pattern matching, rejection, and pin-expression exits in control-flow
  analysis. Keep pin break values in loop result checking and exclude guards,
  arm bodies, later alternatives, and sibling pattern operands that an earlier
  pin exits before reaching. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Avoid exponential generated-code growth for nested JSON codecs by sharing lazy
  payload descriptors between encoding and decoding, preserving recursive codecs. — Thanks @rvcas!
- [13e97ef](https://github.com/orbistry/alder/commit/13e97ef55e34c6092020aaa8a873940d6de07337) Reject unknown JSON variant names that coincide with inherited JavaScript
  properties using the normal path-qualified unknown-variant diagnostic. — Thanks @rvcas!
- [0db756d](https://github.com/orbistry/alder/commit/0db756d7014df5c506fafe13df0b93306896d1fa) Represent explicit async function declarations and lazy async block syntax. — Thanks @rvcas!
- [41f55b6](https://github.com/orbistry/alder/commit/41f55b6201ca3abebac3fafd4e88ff0031d627a7) Retain Alder source and extern declaration regions through bundling, render unresolved externs as labeled shared diagnostics, and verify sibling JavaScript wrapper resolution across path-dependency packages. — Thanks @rvcas!
- [c9bb0b6](https://github.com/orbistry/alder/commit/c9bb0b6fa425b1b26c5d5c21f2705129ec0e9c34) Fix Option coalescing to return its payload type, preserve unit and nested
  Options, and evaluate the default only for None. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Resolve Hash structurally for closed error rows, honoring payload dictionaries
  and preserving hashes across equivalent groups and row widening. Share payload
  evidence with the equality superclass without duplicating nested generated code.
  Remove nominal error-group derives and their declaration-order-dependent behavior;
  error groups use conditional structural Eq, Show, Hash, and Json capabilities.
  Normalize derived enum payloads through the ordinary annotation converter so
  nominal wrappers of named error groups retain their checked structural evidence. — Thanks @rvcas!
- [83c2be3](https://github.com/orbistry/alder/commit/83c2be3acb3e9216511e9e91168a8365df08e992) Add fixed-capacity semaphores with FIFO weighted requests and scoped,
  cancellation-safe permit ownership. — Thanks @rvcas!
- [32f5ff4](https://github.com/orbistry/alder/commit/32f5ff4672ede60400b057cf54cd9cccddfcbfb7) Reject duplicate module identities before publishing interfaces or code, with diagnostics for both source files. Resolve CLI project imports and canonical module paths using package identity and actual source roots, and resolve package-root imports to mod.ald. — Thanks @rvcas!
- [adb35ac](https://github.com/orbistry/alder/commit/adb35acf5e70ddd26ca44e1c21771af88e3f70ac) Resolve nested workspace source files against their most specific containing source root, keeping package identity and local imports independent of member discovery order. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Require compatible payload types for alternative match bindings, preventing
  incompatible values escaping through another alternative. — Thanks @rvcas!
- [6b4af4c](https://github.com/orbistry/alder/commit/6b4af4ce186ce4539bcde262161d4b6f67a39f0d) Reject interface-only dependency caches whose package instance index disagrees
  with the implementation headers in their module interfaces, even when each file
  has a valid fingerprint. — Thanks @rvcas!
- [d8c4983](https://github.com/orbistry/alder/commit/d8c4983815b570e62b4c3085e309a460f508ce20) Allow type variables in local let annotations and reuse the enclosing callable's
  generic variables consistently with lambda annotations. Preserve generic
  contract checking and monomorphic local bindings. — Thanks @rvcas!
- [cb89192](https://github.com/orbistry/alder/commit/cb89192e12cf6d2d57b92ba023c8a3302370155d) Reject distinct workspace roots declaring the same package name before module
  discovery, with a deterministic diagnostic identifying both roots. Repeated
  paths to the same physical package remain one member. — Thanks @rvcas!
- [743a09c](https://github.com/orbistry/alder/commit/743a09c81ed6f65f5bddd21f7ebd853cf8350e18) Require alternative match patterns to bind identical names and share canonical
  binding identities, with source-aware diagnostics for missing or extra names. — Thanks @rvcas!
- [6080822](https://github.com/orbistry/alder/commit/608082277afe3994048b5e758defeecf1d19aa13) Invoke array map, filter, flatMap, and applicative callbacks with only their
  declared value argument. Do not leak JavaScript array indexes or source arrays
  to unary extern-returned callbacks. — Thanks @rvcas!
- [8be2b83](https://github.com/orbistry/alder/commit/8be2b83a931c55e68518e2cd1bfcb527c98236bc) Preserve all trait parameters in default-method annotation scopes, including
  parameters omitted from a method signature. Reject specialization through local
  annotations and retain the trait's superclass evidence in nested bodies. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Carry reachability across aggregate elements, record fields, tag payloads,
  template interpolations, indexing, and assignment operands so unreachable break
  payloads do not constrain an enclosing loop's result. Include contextually
  checked arrays and records. — Thanks @rvcas!
- [a0b9a41](https://github.com/orbistry/alder/commit/a0b9a41b148b86bf6f93567a56ed6d7c98227410) Register the documented Option constructors and lower Some/None construction
  and patterns through the existing nesting-preserving Option representation. — Thanks @rvcas!
- [14be079](https://github.com/orbistry/alder/commit/14be07997423e789b7a0f52954a1f709fcfa7afe) Reject overflowing tuple indices with a source diagnostic instead of silently
  changing them to the largest representable index. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Retain and emit inherited default methods when implementing an imported trait,
  including async defaults and constrained dictionary factories. Publish accurate
  default-helper symbols and implementation method metadata, and invalidate stale
  interface caches. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Expose `Fiber.unbounded` as a Number value for explicit unbounded traversal
  concurrency, including typed built-in value lookup and the bundled runtime export. — Thanks @rvcas!
- [743a09c](https://github.com/orbistry/alder/commit/743a09c81ed6f65f5bddd21f7ebd853cf8350e18) Keep match-only pin patterns and constructor lookup confined to actual match
  patterns instead of leaking permission into nested bindings. — Thanks @rvcas!
- [209141d](https://github.com/orbistry/alder/commit/209141d018dd734c5294da337c763128ea362224) Discover transitive source dependencies using each dependency project's own
  manifest and path base, without requiring prebuilt semantic caches.
  Reject distinct dependency roots claiming one package identity, while coalescing
  equivalent paths to the same canonical root.
  Resolve workspace imports against their owning member's dependency declarations,
  without activating unused sibling dependencies. — Thanks @rvcas!
- [5932a9c](https://github.com/orbistry/alder/commit/5932a9c1a420fd95e6257080683523d4a0cec8ca) Include runtime tag prefixes in structural error-row equality metadata so
  errors with the same tag but different payloads no longer compare equal. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Preserve contextual Option field initialization in records containing spreads.
  Resolve written-field wrapping through ordered field relationships so overwritten
  values retain their own types, later Option fields overwrite earlier values, and inherited
  mutable payloads are not converted. Support nested fresh constructors and arrays. — Thanks @rvcas!
- [cc5d41e](https://github.com/orbistry/alder/commit/cc5d41edbddcd232512dc9148f0acf13bf28a79d) Build source-backed dependencies from current source without mixing in saved
  interfaces or instance indexes, so removed trait implementations cannot remain
  available to consumers through stale caches. — Thanks @rvcas!
- [edbcd63](https://github.com/orbistry/alder/commit/edbcd633652e4097acc18656bca8cd1b891a219a) Add Array.iter and independent ArrayIterator cursors, replacing the non-advancing
  Iterator instance on arrays. Preserve shared source values, live iteration, and
  correct Option payloads through exhaustion. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Initialize trait dictionaries in superclass dependency order before top-level
  values, fixing references to later-declared or derived superclass dictionaries. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Attribute Option lifting failures to a source argument in the failing constraint
  component, preserving its actual types instead of blaming an unrelated earlier
  argument. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Preserve fresh record and array initializer context on pipe inputs, including
  Option-valued fields and optional parameters. Check each input once without
  converting existing mutable aliases or changing runtime evaluation order. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Track active nominal comparison pairs in derived equality so recursive values
  can compare cyclic payloads without repeatedly following the same cycle.
  Preserve payload dictionary checks, NaN inequality, and cleanup after failures. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Preserve explicitly universal type parameters during contextual Option lifting.
  Insert Some wrappers when required instead of specializing a generic function
  or trait method to an Option-shaped parameter. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Check generic call arguments from left to right, including piped arguments,
  so mismatches report the expected type and label the incompatible argument. — Thanks @rvcas!
- [a0b9a41](https://github.com/orbistry/alder/commit/a0b9a41b148b86bf6f93567a56ed6d7c98227410) Short-circuit nested pin effects after failed enclosing pattern checks,
  including pins that suspend while awaiting a task. — Thanks @rvcas!
- [83c2be3](https://github.com/orbistry/alder/commit/83c2be3acb3e9216511e9e91168a8365df08e992) Add synchronized shared cells with serialized task-based state transformations. — Thanks @rvcas!
- [36e654e](https://github.com/orbistry/alder/commit/36e654e810d0145995ec464e01894e24d23a9ddf) Preserve interruption and deliver abort exactly once when a Promise extern
  requests cancellation synchronously during registration, including failed or
  malformed registrations. — Thanks @rvcas!
- [92beacb](https://github.com/orbistry/alder/commit/92beacbbffc363d71f0994a4d6b9aea9ae9d8bef) Ignore late Promise rejection mapping after interruption invalidates the waiter,
  while still observing rejection and preserving exactly-once cancellation cleanup. — Thanks @rvcas!
- [a9f1d44](https://github.com/orbistry/alder/commit/a9f1d4450bc4b377bdb1e062790f53192a4994a8) Preserve lexical break and continue targets when while-condition setup is
  lowered inside a generated JavaScript loop, including suspended conditions. — Thanks @rvcas!
- [ca9ab0e](https://github.com/orbistry/alder/commit/ca9ab0eb46454b5407d43eabb79a517cb679aa38) Canonicalize and deduplicate workspace member roots so repeated patterns, parent-path spellings, and symlink aliases cannot compile one source tree under multiple identities. — Thanks @rvcas!
- [eaea2b7](https://github.com/orbistry/alder/commit/eaea2b7e41907f9d0822e7fac9cb774b33008c71) Append variable-length hash payload bytes without spreading them into function
  arguments, preventing large strings from exceeding the JavaScript argument limit
  while preserving the existing hash byte format. — Thanks @rvcas!
- [0ddfea3](https://github.com/orbistry/alder/commit/0ddfea32658c54e74da4aa92e583caae15147876) Stop body compilation after source-package coherence failures, reporting errors
  at their defining modules and marking other modules blocked. Avoid phantom
  source labels for overlapping implementations declared in different modules. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Solve consistent direct-match Option depth constraints without dense closure.
  Report impossible constraint components consistently even when another component
  has ambiguous wrapping preferences.
  Preserve inferred record/error-row kinds while selecting contextual Some wrappers. — Thanks @rvcas!
- Updated dependencies: alder-bundle@0.3.0, alder-driver@0.4.0, alder-fmt@0.2.1, alder-runtime@0.2.2

## 0.2.3 — 2026-09-04

### Patch changes

- Updated dependencies: alder-bundle@0.2.2, alder-driver@0.3.0

## 0.2.2 — 2026-09-04

### Patch changes

- [76d003f](https://github.com/orbistry/alder/commit/76d003f798a88998fed70574fc9a421b77ef3c26) Update the embedded Deno runtime stack to restore Windows CLI builds and add a
  Windows build to regular CI. — Thanks @rvcas!
- Updated dependencies: alder-runtime@0.2.1

## 0.2.1 — 2026-09-03

### Patch changes

- Updated dependencies: alder-bundle@0.2.1, alder-driver@0.2.1

## 0.2.0 — 2026-09-03

### Minor changes

- [7d53578](https://github.com/orbistry/alder/commit/7d53578e5aef1c152bda29fb55181c78fd9af45d) Implement the M2 core-language pipeline through direct Oxc AST generation,
  Rolldown bundling, the embedded standalone runtime, stdlib/kernel foundations,
  formatting, and test execution. — Thanks @rvcas!

### Patch changes

- [2278da9](https://github.com/orbistry/alder/commit/2278da9f8e58a1c12f6bfdf0723af001c669fc41) Produce and persist solved module interfaces and deduplicated package instance indexes after successful builds. — Thanks @rvcas!
- [eb54db5](https://github.com/orbistry/alder/commit/eb54db59dee8a66087010c71ab9a123154e289d2) Execute the complete Traits guide example and broaden end-to-end trait runtime
  coverage across higher-kinded dictionaries and first-class constrained calls. — Thanks @rvcas!
- [b699950](https://github.com/orbistry/alder/commit/b699950207423f198107d2872dabf690d84235b3) Add source-aware miette diagnostics for compiler errors and warnings, render
  trait and parser failures with labeled snippets, and preserve Alder source in
  code generation snapshots. — Thanks @rvcas!
- [9789f23](https://github.com/orbistry/alder/commit/9789f237bd68714c67bffafa7e8836fc9a731a7f) Compile imported path-dependency sources into the same in-memory Oxc/Rolldown
  module graph, allowing dictionary factories selected from unimported sibling
  modules to bundle and execute without serializing generated JavaScript. — Thanks @rvcas!
- [70ab929](https://github.com/orbistry/alder/commit/70ab9292acb31029b43046e525bc9abaaa705e75) Load imported dependency package instance indexes into package-aware builds and deduplicate their implementations by stable identity. — Thanks @rvcas!
- Updated dependencies: alder-bundle@0.2.0, alder-config@0.2.0, alder-driver@0.2.0, alder-fmt@0.2.0, alder-runtime@0.2.0

