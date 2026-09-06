# alder-kernel

## 0.4.0 — 2026-09-06

### Minor changes

- [2cdb02a](https://github.com/orbistry/alder/commit/2cdb02a9b37599e42a22b373fa4a42878f9869a7) Add lazy fixed-capacity semaphores with FIFO weighted requests, cancellation-safe
  permit ownership, and scoped cleanup before handing permits to the next waiter. — Thanks @rvcas!
- [aca4bc3](https://github.com/orbistry/alder/commit/aca4bc37aba80edd89b5900c2f9c78259404edb5) Add bounded lazy traversal workers with ordered map results, unit-only forEach,
  explicit typed-error traversal, and structured cancellation. Stop active siblings
  as soon as an item defect or selected typed error is observed, before waiting for
  that item's cleanup, and join all cleanup before completing the traversal. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Treat optional record-field shorthand as an ordinary Option type and materialize omitted Option fields as None during contextual record construction.
  
  Remove separate record-field optionality from serialized interfaces and invalidate the previous interface format.
  
  Read Fiber traversal concurrency options using ordinary Option semantics so omitted and explicit None fields both select the sequential default.
  
  Preserve known generic Result error rows during propagation, including explicit Option fallback branches.
  
  Provide conditional Option ordering and Unit ordering so derived record payloads use ordinary field dictionaries, including nested Options. — Thanks @rvcas!
- [f87e540](https://github.com/orbistry/alder/commit/f87e540113feb6c1052b2d782a7ad6e1faa7d204) Add inferred lazy tasks, generator-based async lowering, Promise extern lifting,
  and a structured fiber runtime with interruption, scopes, finalizers, `all`, and
  `race`. — Thanks @rvcas!
- [1e9bb8c](https://github.com/orbistry/alder/commit/1e9bb8c602317a47663e793cd4409b969b90b509) Add lazy synchronized cells with serialized task-based updates, nonblocking
  reads, and cancellation-safe write ownership through scoped cleanup. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Resolve Hash structurally for closed error rows, honoring payload dictionaries
  and preserving hashes across equivalent groups and row widening. Share payload
  evidence with the equality superclass without duplicating nested generated code.
  Remove nominal error-group derives and their declaration-order-dependent behavior;
  error groups use conditional structural Eq, Show, Hash, and Json capabilities.
  Normalize derived enum payloads through the ordinary annotation converter so
  nominal wrappers of named error groups retain their checked structural evidence. — Thanks @rvcas!
- [667d9be](https://github.com/orbistry/alder/commit/667d9be1215cf940c7b92a998456e866254f083c) Add lazy reusable Ref operations with synchronous state transitions, preserved
  payload aliasing, and cancellation-safe commit boundaries. — Thanks @rvcas!

### Patch changes

- [36e654e](https://github.com/orbistry/alder/commit/36e654e810d0145995ec464e01894e24d23a9ddf) Wait for owned child cleanup in scope, all, and race before delivering cancellation
  to the caller, including cancellation during winner/failure cleanup. — Thanks @rvcas!
- [7780a12](https://github.com/orbistry/alder/commit/7780a12a1ee6b91f33ed0dbfe73c393c2dfd0055) Prevent all/race from stranding owned child fibers when a later task factory
  fails, and wait for partial-child cleanup before propagating that failure. — Thanks @rvcas!
- [1efea6c](https://github.com/orbistry/alder/commit/1efea6c0a882752faf882525f5f292e2b20a6360) Require Json evidence for module encode/decode calls and dispatch through the
  selected codec, including custom instances. Remove unchecked JSON entry points
  and fix public export names for imported direct dictionary calls. — Thanks @rvcas!
- [09541ea](https://github.com/orbistry/alder/commit/09541eaceaabc9342c2f7cc90c775bd0def67e98) Preserve scheduler fairness across immediately ready Promise resumptions,
  child fibers, and finalizers using a shared host-yield budget. — Thanks @rvcas!
- [13e97ef](https://github.com/orbistry/alder/commit/13e97ef55e34c6092020aaa8a873940d6de07337) Reject JSON Result envelopes with a missing payload before invoking a custom
  payload decoder, preserving the decoder's String argument contract. — Thanks @rvcas!
- [e41f380](https://github.com/orbistry/alder/commit/e41f380ac60b3e6e7c1311748d3b175cc68e6f8e) Preserve Option layers across equality, showing, hashing, mapping, applicative operations, traversal, and map lookup. Distinguish kernel boxes from user Some variants and round-trip nested nullable JSON options with escaped envelopes. — Thanks @rvcas!
- [27cf3f3](https://github.com/orbistry/alder/commit/27cf3f31e7a86cb1991756b7e16bab3c0d948abd) Unwind task cleanup for malformed runtime operations and synchronous handler
  defects instead of skipping cleanup or escaping the shared scheduler. — Thanks @rvcas!
- [10379a9](https://github.com/orbistry/alder/commit/10379a92b002824f382e96743b9417bfd5dfed8b) Detect active cycles in derived value operations: Show emits `<cycle>`, while
  Hash, Ord, and JSON encoding raise explicit TypeError defects. Clear active
  tracking after success or exceptions and preserve shared acyclic values. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Delegate every declared record field to its ordinary equality, ordering,
  hashing, and JSON dictionary. Preserve nested Options and unit payloads, and
  decode missing Option fields as None. — Thanks @rvcas!
- [12b09ad](https://github.com/orbistry/alder/commit/12b09ad1f41251491b7a548e9d6f1e16ad320bd4) Validate primitive Json trait decoding against the requested type, preserving
  validation inside derived and container codecs. Encode unit as null and BigInt
  as a decimal JSON string; reject non-finite JSON numbers instead of silently
  encoding them as null. — Thanks @rvcas!
- [13e97ef](https://github.com/orbistry/alder/commit/13e97ef55e34c6092020aaa8a873940d6de07337) Reject unknown JSON variant names that coincide with inherited JavaScript
  properties using the normal path-qualified unknown-variant diagnostic. — Thanks @rvcas!
- [677e20f](https://github.com/orbistry/alder/commit/677e20fd158941294eb7688d34180b68cda0e17e) Preserve record-tail identities during inference and publication, check ordinary
  field types at value boundaries, and read stored Option fields directly. — Thanks @rvcas!
- [6080822](https://github.com/orbistry/alder/commit/608082277afe3994048b5e758defeecf1d19aa13) Invoke array map, filter, flatMap, and applicative callbacks with only their
  declared value argument. Do not leak JavaScript array indexes or source arrays
  to unary extern-returned callbacks. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Expose `Fiber.unbounded` as a Number value for explicit unbounded traversal
  concurrency, including typed built-in value lookup and the bundled runtime export. — Thanks @rvcas!
- [edbcd63](https://github.com/orbistry/alder/commit/edbcd633652e4097acc18656bca8cd1b891a219a) Add Array.iter and independent ArrayIterator cursors, replacing the non-advancing
  Iterator instance on arrays. Preserve shared source values, live iteration, and
  correct Option payloads through exhaustion. — Thanks @rvcas!
- [556a21c](https://github.com/orbistry/alder/commit/556a21c7166f51c0b914eabad951440b797ccba4) Track active nominal comparison pairs in derived equality so recursive values
  can compare cyclic payloads without repeatedly following the same cycle.
  Preserve payload dictionary checks, NaN inequality, and cleanup after failures. — Thanks @rvcas!
- [563e003](https://github.com/orbistry/alder/commit/563e003b54f0bf115ccfb9d605b3840b02f28deb) Execute nested task awaits with explicit fiber-local continuation frames,
  preserving lazy reuse and stack-safe success, failure, and interruption cleanup. — Thanks @rvcas!
- [36e654e](https://github.com/orbistry/alder/commit/36e654e810d0145995ec464e01894e24d23a9ddf) Preserve interruption and deliver abort exactly once when a Promise extern
  requests cancellation synchronously during registration, including failed or
  malformed registrations. — Thanks @rvcas!
- [92beacb](https://github.com/orbistry/alder/commit/92beacbbffc363d71f0994a4d6b9aea9ae9d8bef) Ignore late Promise rejection mapping after interruption invalidates the waiter,
  while still observing rejection and preserving exactly-once cancellation cleanup. — Thanks @rvcas!
- [eaea2b7](https://github.com/orbistry/alder/commit/eaea2b7e41907f9d0822e7fac9cb774b33008c71) Append variable-length hash payload bytes without spreading them into function
  arguments, preventing large strings from exceeding the JavaScript argument limit
  while preserving the existing hash byte format. — Thanks @rvcas!

## 0.3.0 — 2026-09-04

### Minor changes

- [ac24445](https://github.com/orbistry/alder/commit/ac24445101bb7a8d5bef6076ff145b94e103c91a) Add row-typed `Result` errors, inferred and propagated error rows, exhaustive
  error matching, diagnostics, direct AST lowering, and typed JSON failures. — Thanks @rvcas!

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

