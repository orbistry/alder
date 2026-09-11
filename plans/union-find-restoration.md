# Union-find restoration and compiler cleanup

Restore a single union-find-backed shared type graph without changing Alder's
language, diagnostics, or runtime contracts. The task is an implementation
change, not a new grammar or a reason to weaken regression coverage.

## Acceptance gates

- [x] Record substitution-solver correctness and performance baselines.
- [x] Inspect the active solver, original Rust port, and local Elm references.
- [x] Replace substitution storage with weighted union-find and shared types.
- [x] Preserve variable kinds, generalization, occurs checks, and rigid contracts.
- [x] Verify independent-error recovery discards graph mutations and evidence.
- [x] Add graph invariants and targeted inference regression tests.
- [x] Compare real and synthetic inference timing and allocation measurements.
- [x] Audit project-owned files, dependencies, helpers, comments, and boundaries.
- [x] Remove verified obsolete implementations and their orphaned artifacts.
- [x] Verify the options-aware CLI execution path without undoing user edits.
- [x] Update implementation documentation, SPEC tracking, and changesets.
- [x] Pass workspace formatting, check, strict Clippy, all-feature tests, E2E,
      and whitespace checks without warning suppressions.

## Initial state

The active solver is `alder-solve/src/inference.rs`, called from the driver
through `solve`. It uses owned recursive `Ty` trees and a substitution vector.
The original weighted union-find in `alder-constrain/src/union_find.rs` and its
consumers are unlinked. Elm's `Type/UnionFind.hs` supplies the weighted union and
path-compression reference; its generalization rank is a separate concept from
union weight. Alder's current recovery discards each failed inference attempt,
including deferred constraints and evidence.

Pre-existing user edits already remove the CLI `exec_with` wrappers and CLI
inline tests. Preserve those edits; separately verify end-to-end behavior.

The initial `cargo test --workspace --all-features --quiet` passes, but reports
an unused `target_tail` binding and a macOS linker compact-unwind size warning.
Both remain cleanup work, not an acceptable final warning baseline.

## Measurement

`cargo bench -p alder-solve --bench inference` measures only the full `solve`
call after parsing, canonicalization, constraint preparation, and trait database
construction. Each sample gets a fresh arena. Timings and allocation totals
include inference, trait resolution, and checked output construction, not source
preparation. The same inputs and settings must be used before and after.

No commits, pushes, or publishing are authorized by this task.

## Final verification

All required checks pass locally without compiler/linker warnings or lint
suppression:

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
git diff --check
```

The all-feature suite includes 2,824 unit/integration tests and one passing
doctest; two existing documentation examples remain ignored, unchanged.
The added CLI E2E suite covers all 18 fixture projects plus failure-mode and
repeatability probes. The explicit Insta check reports no unreferenced snapshots
and no snapshots to review. Package file listings preserve all required assets.

The full before/after table, cleanup inventory, invariants, and host limitations
are in `docs/union-find-restoration.md`. No runtime semantics or interface format
changed. The unused driver public-API removals are recorded as a minor changeset.
Pre-existing user changes and optional user tooling are preserved.
