# Union-find restoration: evidence and measurements

The active program-inference path now uses a shared union-find type graph.
This report records the architecture, measured comparison, and cleanup evidence.

## Active architecture

`alder_driver::build_with_reporter` → `alder_solve::solve` →
`inference::Infer` → `type_graph::TypeGraph` is the checked path. `Infer` owns
one graph per attempt; it has no substitution vector or fallback backend.

- `Ty` is a reference-counted point. Every application, function, tuple, record,
  error row, constructor section, and projection contains shared child handles.
  Cloning a handle never recursively clones its type. Redundant `Box<Ty>` links
  have been removed.
- Roots own descriptors and union weights; other points link to roots.
  Representative lookup compresses paths, and union keeps the larger class's
  physical root. Semantic variable IDs are selected independently, so balancing
  cannot choose diagnostic names or affect generic-variable identity.
- Generalization uses Alder's existing free-environment sets, SCC handling,
  mutation restrictions, and explicit universal contracts. Union weight is not
  an inference level. Elm's rank-pool or rigid-descriptor semantics were not
  reintroduced.
- Binding checks kind compatibility and occurs before changing descriptors.
  Structurally checked equal types can share representatives; compatibility
  through `Any` or differing optional record fields does not falsely equate them.
- Normalization reuses unchanged nodes. Local row-tail elimination and overlay
  normalization explicitly replace local descriptor copies, not shared nodes.
- Recovery retains the existing transactional boundary: discard the entire
  failed attempt, including unions, descriptors, obligations, deferred rows,
  generic contracts, and evidence. A recovered remainder is diagnostic-only
  after any error; it cannot publish a successful interface.

The local Elm `Type/UnionFind.hs` and original Rust union-find were inspected
before migration. Their weighted point/link structure is the structural
reference, not an override of Alder semantics. Pairwise trait-head overlap
matching in `traits.rs` still uses temporary pattern bindings; it is a coherence
predicate, not an alternative program-inference solver or fallback.

## Substitution baseline

Command: `cargo bench -p alder-solve --bench inference`, optimized bench profile,
15 measured samples after two warmups per case. Counting includes requested
allocation/reallocation bytes, not resident memory or peak live memory. Atomic
allocation counters add overhead to timings on both sides of the comparison.

The full `solve` call is measured; source preparation is outside the timer.
Each iteration uses a fresh arena, so no previously solved module is reused.
Host: Apple Silicon/macOS, Rust 1.98.1 (`48a229cea`, 2026-09-01). Baseline was
recorded before modifying the active solver; both runs use the optimized bench
profile and the same eight sources. Development-profile changes do not affect it.

| Case | Median µs | Allocations | Allocated bytes |
| --- | ---: | ---: | ---: |
| variable_chain | 22189.875 | 465447 | 168231571 |
| shared_types | 2819.166 | 73423 | 28020675 |
| polymorphism | 9341.250 | 208044 | 76267129 |
| records | 19600.250 | 388829 | 295600315 |
| traits | 3349.625 | 54574 | 30503204 |
| pipes_fixture | 63.708 | 1496 | 380649 |
| async_traits_fixture | 178.417 | 4256 | 1366789 |
| result_instances_fixture | 102.208 | 2598 | 662644 |

The synthetic cases exercise a 200-link function chain, nine levels of shared
tuple duplication, 100 polymorphic callers, 80 record-overlay callers, and 80
nested trait-heavy callers. The real inputs are the pipes program, async trait
methods, and Result instances from `tests/e2e`. Fixture reads occur before the
timer; benchmark compilation does not require out-of-package `include_str!` files.

## Union-find measurements

| Case | Median µs | Time change | Allocations | Allocated bytes |
| --- | ---: | ---: | ---: | ---: |
| variable_chain | 14967.917 | −32.5% | 243127 | 60809211 |
| shared_types | 475.542 | −83.1% | 10888 | 547947 |
| polymorphism | 5604.375 | −40.0% | 61336 | 15271233 |
| records | 13068.458 | −33.3% | 225373 | 42647979 |
| traits | 2506.083 | −25.2% | 27301 | 14037860 |
| pipes_fixture | 66.083 | +3.7% | 1390 | 232473 |
| async_traits_fixture | 172.750 | −3.2% | 3571 | 601645 |
| result_instances_fixture | 100.291 | −1.9% | 2230 | 336188 |

The first graph version regressed records and small programs because pruning
reallocated unchanged nodes and child links retained redundant boxes. Those
causes were addressed before this final comparison. All cases allocate fewer
objects and bytes. The smallest pipes fixture remains 2.375 µs slower in this
run; do not interpret these single-host microbenchmarks as universal speedups.
Peak live memory/RSS was not measured. No material timing regression remains
in the synthetic stress cases.

## Cleanup inventory

- Deleted 17 unlinked Rust files: five old canonicalizer files, eight old
  constraint/union-find files, and four old solver files. Exact paths are in
  `compiler-implementation-map.md`. Their module/include edges and snapshot
  sources were checked before deletion; they were not active test coverage.
- Deleted the empty unpublished `tasks/stub` manifest and no-op executable;
  removed its workspace glob and CI packaging exclusion. Historical package
  reports remain explicitly historical rather than being rewritten as new runs.
- Removed the unused public driver `ModuleMeta`, `start_build`, `needs_rebuild`,
  and error-discarding `load_package_index` APIs. Checked index loading and
  serialized interface formats are unchanged. This API removal has a minor
  driver changeset.
- Removed unused config dependencies `pubgrub` and `insta`, the solver's unused
  `alder-source` dev dependency, and unused workspace declarations for
  `color-print`, `hex`, `pretty_assertions`, and `pubgrub`. The lockfile drops
  PubGrub's now-unused `priority-queue` and `version-ranges` dependencies; no
  dependency versions were upgraded.
- Kept Rolldown/Oxc family pins: they constrain package resolution even where
  there is no Rust import. Kept workspace `std/` and packaged `stdlib/` copies:
  inclusion and parity tests consume both. Kept runtime bootstrap/kernel assets,
  CI release scripts, fixtures, provisional design plans, and the vendored Elm
  reference.
- Kept optional `.claude/` tooling rather than assuming that absence from the
  repository's hook registrations proves absence of personal/external use.
  Its usage was raised with the user; those configuration files were not changed.
- Preserved the user's pre-existing CLI edits, including removed inline tests.
  Consolidated the remaining compile wrapper/constant persistence branch and
  added a black-box CLI harness over all 18 existing E2E fixture projects. The
  single options-aware `exec` path is exercised rather than bypassed.
- Parsed Cargo target roots and recursive Rust module declarations: all 130
  remaining project Rust files are reachable as modules, tests, benches, or build
  scripts. Package file listings include the new graph and required runtime and
  standard-library assets. No referenced snapshot was changed or deleted.

## Warning fixes

The baseline test run emitted an unused `target_tail` warning; the unnecessary
binding is now `_`. Strict Clippy also found a manual slice fill in
`option_levels.rs`; it now uses `fill`.

The macOS CLI linker's `__eh_frame` section was 17,652,056 bytes, exceeding its
24-bit compact-unwind offset limit. Development dependencies now use opt-level 1,
reducing it to 8,275,996 bytes. Workspace crates remain unoptimized, with debug
information, debug assertions, and panic unwinding retained. This fixes the
underlying size issue; no linker warning or lint is suppressed. A clean dependency
rebuild has an additional optimization cost (the local rebuild took 1m 33s).

## Validation evidence

- Five graph tests cover weighted representatives, path compression, shared
  edges, attempt isolation, and reclamation after dropping an attempt.
- Five focused inference tests cover linked occurs checks, rejected kind merges,
  structural coalescing, generalization independent of union weight, independent
  instantiation with shared outer variables, and retry isolation after partial
  unification and deferred constraints.
- Four CLI E2E tests execute all 18 fixture projects, intentional synchronous and
  asynchronous failures followed by success, all ten pattern-failure modes and
  successful cleanup, and repeated-process bundles/diagnostics. Only the validated
  elapsed-time suffix is excluded from diagnostic comparison; text, ordering,
  source regions, and summary counts must match byte-for-byte.
- `cargo insta test --workspace --all-features --test-runner cargo-test --check
  --unreferenced reject` passes: 2,824 unit/integration tests, one passing doctest,
  and the two pre-existing ignored documentation examples. No unreferenced or
  changed snapshots.
- `cargo check --workspace --all-targets --all-features` passes.
- `cargo fmt --all -- --check`, strict workspace Clippy, the direct
  `cargo test --workspace --all-features` run, and `git diff --check` all pass
  without compiler/linker warnings or suppression. The companion plan records
  the complete commands.

Verification is local macOS/Apple Silicon, not a claim of Windows/Linux CI or a
new release-package build. `cargo package --workspace --allow-dirty --offline
--list` checked package inclusion; no commit, push, or publication was performed.
