# Release packaging verification

Current compiler/runtime integration: `556a21c`. The following completed package
checkpoints predate the final integration, including Fiber.unbounded, imported
defaults, pattern captures, ArrayIterator, and Coalesce fixes. They are historical
evidence only; fresh package verification and packaged CLI execution are required.

## Completed integrated checkpoint after 6b4af4c

`cargo package --workspace --exclude stub --offline --allow-dirty --target-dir
/tmp/alder-package-current.4UcAo6` finished with exit 0 in session `99378`. All 17
publishable crates verified, including cache identity namespaces and index/
interface consistency checks. All 1,511 extracted source/stdlib/kernel files
match the integrated worktree. Cargo.lock SHA-256 is unchanged:
`454f3c8f1a6956c18765d79d0ff62fcc5706776b1b0eafaba783491f2e1d9f0e`.

The packaged CLI runs fresh copies in `/tmp/alder-packaged-fixtures.HrnWiB`
from working directory `/tmp`. All four examples and seven integration projects
pass: hash_equality, record_options, traits, explicit_async, control_flow, externs,
and records. Test mode gives 7 passes/exit 0 for the success fixture and
1 pass/3 failures/exit 1 for the intentional failure fixture, including its later
async success. The runner session `92267` is terminal with exit 0.

Previously syntax-updated original review copies execute records, loops, Option,
local externs, 20,000-deep composition, and the formatter payload successfully.
Invalid trait specialization, shared-array use, missing return, and duplicate
modules reject with their specific diagnostics; both duplicate paths are shown.
The formatter reports zero changes and its program runs before and after.
Each CLI subprocess has a 45-second timeout.

Packaged `check` also persists the hello fixture's interface under
`.alder/interfaces/application/main.aldi` and its application index at the
expected path, without producing the obsolete unqualified interface path.
The original kernel probe observes a timer during 10,000 ready Promise awaits
against the extracted kernel. No compiler/runtime sources changed during these
checks. All processes finished; this is local integrated dirty-tree validation,
not final clean-commit or remote-platform CI approval.

## Previous integrated checkpoint at 42c5423

Fresh verification after the cancelled-Promise mapper fix succeeds for all 17
publishable crates:

```sh
cargo package --workspace --exclude stub --offline --allow-dirty --target-dir /tmp/alder-package-current.NlhYrd
```

Cargo exited 0 with verification enabled. All 1,511 extracted source/stdlib/
kernel files match the integrated worktree. The lockfile digest remains
`454f3c8f1a6956c18765d79d0ff62fcc5706776b1b0eafaba783491f2e1d9f0e`.
No production sources changed during verification.

The packaged `/tmp/alder-package-current.NlhYrd/debug/alder`, launched from
`/tmp`, runs fresh cache-free copies under `/tmp/alder-packaged-fixtures.9ZkB8z`:
all four examples and seven integration projects listed below pass, including
the new mutual-recursive overlay and JSON field-name cases. Test mode reports
7 passes/exit 0 for the success fixture and 1 pass/3 failures/exit 1 for the
intentional failure fixture, including the later asynchronous success.

Original review probes with the previously documented syntax updates pass
their expected outcomes: six valid programs execute; trait specialization,
array unsoundness, missing return, and duplicate modules reject with the specific
expected diagnostics. Both duplicate paths are shown. The formatter probe runs
before/after formatting, which reports zero changes. Each run/test/check is
bounded by a 45-second process alarm (formatting is a separate command).

The extracted kernel passes the original 10,000-ready-Promise timer probe.
Its four new Promise regression harnesses were also read from the extracted
Rust test source and executed with the extracted kernel under Node: cancelled
mapping, reentrant registration, throwing mapper, and throwing then getter.
These direct checks supplement, not replace, the V8 kernel suite.

All verification processes finished. This supersedes the archives below but is
still local integrated dirty-tree evidence, not a final clean commit, remote
platform CI, or completion of the hardening goal.

## Previous integrated checkpoint at 209141d

Subsequent runtime change: cancelled Promise waiters now skip late rejection
mapping. The kernel/CLI archives below predate this fix and require refresh.

Fresh verification after the source-dependency discovery fix succeeds for all
17 publishable crates:

```sh
cargo package --workspace --exclude stub --offline --allow-dirty --target-dir /tmp/alder-package-current.9dUFHT
```

Cargo exited 0 with verification enabled in a new target directory. All 1,510
extracted `src`, `stdlib`, and `kernel` files match the worktree byte-for-byte.
Cargo.lock SHA-256 remains
`454f3c8f1a6956c18765d79d0ff62fcc5706776b1b0eafaba783491f2e1d9f0e`.
No compiler/runtime production sources changed during verification.

The packaged `/tmp/alder-package-current.9dUFHT/debug/alder`, invoked from
`/tmp`, passes all four examples and seven integration projects listed in the
previous checkpoint below, using fresh copies without `.alder` or `dist` under
`/tmp/alder-packaged-fixtures.GphmY7`. This includes the new derived JSON field-
name regression. Its initial fixture had an unqualified enum constructor;
after correcting it to `Fields::Fields`, both the dedicated
`option_record_defaults_execute` test and packaged execution pass. The initial
`standalone_e2e_projects_execute` run did not cover that fixture.

Packaged test mode reports 7 passes and exits 0 for `e2e/tests`. The intentional
failure fixture reports 1 pass and 3 failures, executes the later asynchronous
success, and exits 1. Each run/test process has a 45-second alarm.

Fresh copies of the original review probes (with the previously documented
approved syntax updates) execute records, loops, nested Options, local Promise
externs, deep task composition, and formatting successfully. Trait
specialization, mutable-array unsoundness, missing returns, and duplicate
modules exit 1 with their specific expected diagnostics, including both
duplicate paths. Formatting preserves the original template payload and reports
zero changes. The original unmodified runtime probe observes the host timer
during 10,000 fulfilled Promise awaits against the extracted kernel.

All processes finished. This supersedes the previous archive evidence, but
remains local-host integrated dirty-tree validation, not clean-commit or remote
platform CI evidence. Whole-goal completion remains unproven.

## Previous integrated checkpoint at cc5d41e

Subsequent production change: transitive source-dependency discovery now follows
each dependency's manifest. The archives in this checkpoint predate that driver
fix; final package verification must be refreshed.

Fresh verification of all 17 publishable crates succeeds with the ongoing
hardening worktree on `cc5d41e`:

```sh
cargo package --workspace --exclude stub --offline --allow-dirty --target-dir /tmp/alder-package-current.xhI60N
```

Cargo exited 0 with verification enabled and a distinct new target directory.
All 1,510 extracted files in crate `src`, `stdlib`, and `kernel` directories
match the worktree byte-for-byte. Cargo.lock remains SHA-256
`454f3c8f1a6956c18765d79d0ff62fcc5706776b1b0eafaba783491f2e1d9f0e`.
No production sources changed during verification. This includes the current
derived-equality ABI, recursive dictionaries, large-payload hashing, and
source-dependency cache fix, superseding the older archives below.

The packaged `/tmp/alder-package-current.xhI60N/debug/alder`, invoked from
`/tmp`, runs fresh copies under `/tmp/alder-packaged-fixtures.xUARuM` of:

- all four examples: hello, pipes, traits, async;
- seven integration projects: hash_equality, record_options, traits,
  explicit_async, control_flow, externs, records.

Copies exclude `.alder` and `dist`. Each invocation has a 45-second process
alarm. All exit 0. Packaged test mode reports 7 passing tests for `e2e/tests`
and exits 0. The intentional failure fixture reports 1 pass and 3 failures
(sync Result, async Result, async defect), runs the later success, and exits 1.

Fresh copies of the original review probes also pass their intended checks:
records, loops, nested Option equality, local Promise externs, and 20,000-deep
task composition execute. Trait specialization, shared-array unsoundness,
missing returns, and duplicate modules each exit 1 with the expected specific
diagnostic; duplicate diagnostics show both paths. Copies update only the
approved syntax changes (`async fn` and `Some(None)`). The formatter probe
restores the original three-space template line and executes before and after
formatting (which reports zero changed files). The original runtime probe,
unmodified, observes a host timer during 10,000 fulfilled Promise awaits against
the extracted kernel.

This is local-host integrated dirty-tree package evidence, not remote platform
CI, a final clean-commit verification, or whole-goal completion. Later production
changes require refreshing the affected evidence.

## Generated-code/cache ABI audit

The CLI creates a fresh source database for each build, includes discovered
dependency sources in the build graph, and emits their AST artifacts again.
`persist_semantic_artifacts` saves interfaces and package instance indexes, not
generated JavaScript. The bundler consumes that build's artifacts and the
current executable's embedded `alder_kernel::KERNEL_JS`. Consequently, the
new fourth `$equalDerived` argument does not require invalidating a persisted
JavaScript artifact cache in this pipeline: there is no such reader.

The CLI path-dependency regression now saves package interfaces, builds and
executes the application, changes a dependency implementation body without
changing its public signature or refreshing its saved interfaces, and builds
and executes again. The second execution observes the updated body. The focused
test passes. This does not prove arbitrary semantic-interface invalidation or
executable support for interface-only packages, and does not replace fresh
compiler/kernel release packaging verification.

The semantic follow-up exposed a separate defect: removing a dependency's
implementation still allowed its consumer to type-check against the saved
package index. The regression failed before the fix. Source-backed dependencies
now skip saved interfaces/indexes entirely; current source owns both executable
bodies and semantic declarations. Interface-only loading remains separate.
The regression now rejects the removed implementation during compilation.
The current checkpoint above includes this driver fix.

## Historical checkpoint after import and overlay fixes

Subsequent solver change: normalization now accounts for guaranteed fields on
open inputs. The archives below predate that fix and must be refreshed for
final delivery; see `plans/record-overlay-hardening.md`.
They also predate the unary array callback adapter fix; the archived CLI was
used to reproduce that failure against the new externs regression.
The primitive Hash superclass equality fix also postdates these archives.
The variable-length hash byte-append fix likewise requires fresh package checks.
Nested recursive derived-dictionary evidence has changed since these archives too.
Derived Eq now also passes a nominal identity to the kernel's active-pair guard;
the compiler/kernel pair must be packaged and verified together.

At `08c77e5` plus the ongoing hardening worktree, fresh verification succeeds
for all 17 publishable crates:

```sh
cargo package --workspace --exclude stub --offline --allow-dirty --target-dir /tmp/alder-package-current.6ns25p
```

Cargo exited 0 with verification enabled and a newly allocated target directory.
All 1,507 extracted files under crate `src`, `stdlib`, and `kernel` directories
match their worktree counterparts byte-for-byte, across all 17 crates. This
includes the closed-overlay normalization, import namespace fix, ordinary Option
fields, interface format 6, and current runtime. No production sources changed
while verification ran. Cargo.lock retains SHA-256
`454f3c8f1a6956c18765d79d0ff62fcc5706776b1b0eafaba783491f2e1d9f0e`.

The packaged `/tmp/alder-package-current.6ns25p/debug/alder`, launched from
`/tmp`, executes hello, pipes, traits, and async examples and the record_options,
traits, explicit_async, and control_flow integration projects successfully.
Each was also run from a fresh source/config-only copy under
`/tmp/alder-packaged-fixtures.TJxvBw`, without copied caches or build artifacts.
The original host-fairness probe run against the extracted kernel reports timer
progress during 10,000 immediately fulfilled Promise awaits.

The integrated full `cargo test --quiet` run also exited 0, including 182 driver,
452 solver integration, 14 solver unit, 54 kernel, 14 CLI, and 1,310 parser tests
and workspace doctests (two existing ignores). Whitespace checks pass and no
pending snapshots remain. Strict Clippy/formatting passed in the preceding
source-comment cleanup turn; no Rust logic changed in this verification turn.

This is local-host dirty-tree package evidence, not remote platform CI,
cargo-dist execution, final committed-tree acceptance, or goal completion.
Subsequent production changes require another refresh. All sections below are
historical checkpoints and do not supersede this one.

## Post-record-equivalence checkpoint

Subsequent source change: adjacent closed overlay normalization now rejects
contradictory inherited-field contracts hidden by different field grouping.
The verified archives below predate that solver fix; final packaging must be
refreshed. See `plans/record-overlay-hardening.md` for the reproduction/evidence.

Fresh local verification succeeds for all 17 publishable crates:

```sh
cargo package --workspace --exclude stub --offline --allow-dirty --target-dir /tmp/alder-package-records.FkiXbJ
```

The target directory was newly allocated with `mktemp -d`; verification was
enabled and Cargo exited zero. Archived AST, solver inference, codegen backend,
owned-interface, Traits stdlib, and kernel source files match the worktree
byte-for-byte. The archive includes Fiber, Semaphore, and SynchronizedRef stdlib
files. Cargo.lock's SHA-256 remains
`454f3c8f1a6956c18765d79d0ff62fcc5706776b1b0eafaba783491f2e1d9f0e`.

From `/tmp`, `/tmp/alder-package-records.FkiXbJ/debug/alder` successfully executes
all four examples (hello, pipes, traits, async) and the record_options, traits,
explicit_async, and control_flow integration projects. Record-options includes
late-context/open-spread defaults and nested Option dictionary/JSON behavior.
The original fairness probe against the extracted kernel reports timer progress
during 10,000 immediately fulfilled Promise awaits.

This covers the current production source, including the record/Option migration
and interface format 6, unlike the historical checkpoints below. While Cargo
ran, only unreleased changeset descriptions were reconciled to remove obsolete
presence/fallback claims; no packaged production source changed. This is local
host, dirty-worktree verification, not clean-release approval, remote platform
CI, or cargo-dist execution. Joint acceptance review and clean commits remain
required, and later production changes require refreshed package verification.

## Relocation checkpoint before record-equivalence migration

Fresh `cargo package --workspace --exclude stub --offline --allow-dirty
--target-dir /tmp/alder-package-relocation.lInauv` verifies all 17 crates,
including the external workspace-member relocation fix. Archived driver
project.rs and solver inference.rs match the worktree byte-for-byte; Cargo.lock
is unchanged. Its CLI checks the external aliased-member workspace as one
member/two modules and executes examples/async successfully.

While verification ran, the user approved record-field equivalence and a new
regression was added which fails against the pre-migration compiler. These
archives therefore verify the pre-record-migration source checkpoint only;
they do not establish delivery of the newly approved semantics or final release
readiness. No record production change had landed at this checkpoint.

## Current Option-inference and runtime checkpoint

Fresh verification at `0ddfea3` plus the hardening worktree succeeds for all 17
publishable crates, including the traversal defect-cancellation fixes and the
universal, row-kind, sparse-depth, and diagnostic-provenance Option fixes:

```sh
cargo package --workspace --exclude stub --offline --allow-dirty --target-dir /tmp/alder-package-current.VmQGic
```

The target directory was freshly allocated; verification completed with exit
zero. Archived `option_levels.rs`, solver `inference.rs`, kernel `index.ts`, and
the canonicalizer's `stdlib/Fiber.ald` match the working sources byte-for-byte.
Workspace Cargo.lock remains unchanged.

From `/tmp`, the resulting packaged `debug/alder` successfully executes traits,
explicit_async, control_flow, and pattern_bindings (`-- success`), as well as
hello, pipes, traits, and async examples. The traits fixture includes the new
universal-input and inferred-error-row Option-lifting regressions.

This verifies the current dirty-source checkpoint, not a final release commit.
Remaining language decisions, joint implementation review, coherent commits,
and final clean-tree acceptance remain open. This is local host evidence, not
Linux/Windows CI or cargo-dist verification. Older checkpoints below describe
their own source states and are retained as historical evidence.

## Control-flow package checkpoint

At `14be079` plus the current hardening worktree, full workspace tests and
doctests pass, including 426 solver integration tests, 166 driver tests, and
six AST flow tests. Strict all-target/all-feature Clippy passes. The rebuilt
workspace CLI executes the control_flow fixture from `/tmp`, including nested
pin exits and per-alternative guard retries. No pending snapshot files were
found, and workspace Cargo.lock is unchanged.

Fresh verification completed successfully for all 17 publishable crates:

```sh
cargo package --workspace --exclude stub --offline --allow-dirty --target-dir /tmp/alder-package-flow.9dqKdL
```

This directory was newly allocated with `mktemp -d`; Cargo exited zero with
verification enabled. Extracted AST flow, solver inference, and kernel source
files match the worktree byte-for-byte. The archive includes all 17 stdlib files,
including Semaphore and SynchronizedRef, and the kernel TypeScript source.

The packaged `debug/alder` executed control_flow, pattern_bindings (`-- success`),
explicit_async, and traits fixtures, plus all four examples, from `/tmp`.
Original trait-specialization, shared-array, zero-iteration return, and duplicate
module probes reject with the expected source diagnostics. Original record,
valued-loop, and nested-Option probes execute successfully; the loop prints 42.
The current-syntax extern and 20,000-deep await probes also execute successfully,
as does the restored whitespace-template probe. The original Node fairness
probe reports the host timer ran during 10,000 fulfilled Promise awaits against
the extracted kernel source. This last template check executes an already
restored fixture; the actual format/execute transformation is covered by the
workspace CLI regression, not newly claimed for this packaged invocation.

Workspace Cargo.lock is unchanged. This checkpoint does not replace final
clean-commit packaging, remaining language decisions, or remote platform
verification.

## Earlier source-identity checkpoints

Commit `ca9ab0e` canonicalizes and deduplicates workspace
member roots; subsequent uncommitted driver API changes require explicit source
identity metadata and remove URI guessing. Formatting, Clippy, workspace tests,
doctests, and actual CLI checks pass. Fresh package verification now also covers
these changes; the full hardening review and commit gates remain open.

Subsequent source change: duplicate named workspace package roots are now
rejected during loading. The explicit-identity package checkpoint below predates
that fix; final packaging must cover it too.

Commit `a9f1d44` subsequently fixes lexical loop exit targets in codegen. Full
workspace tests/doctests, strict Clippy, actual control-flow execution, and all
four examples pass. Package verification below also predates this codegen fix.

## Explicit-source-identity package checkpoint

`cargo package --workspace --exclude stub --allow-dirty --target-dir
/tmp/alder-package-identities.UpMP5z` completed successfully with verification
enabled for all 17 crates. This newly allocated target avoids the same-version
temporary-registry cache problem described below.

The resulting `debug/alder` binary was invoked from `/tmp`. Both the four-module
nested workspace and the aliased-member workspace pass `check`; the latter
reports one member and two modules. The traits and explicit_async integration
projects and all four examples (hello, pipes, traits, async) execute successfully.
These checks cover the current explicit identity API and canonical workspace
member roots. They do not establish remote Linux/Windows or cargo-dist results,
and do not close the remaining compiler hardening acceptance gates.

## Nested-workspace package checkpoint

Reusing `/tmp/alder-package-current.6yJzBM` after changing driver source produced
a misleading successful package check. The extracted driver archive contained
`source_owner`, but the resulting CLI linked an older registry copy of
alder-driver 0.3.0. Its dep-info points to
`/Users/rvcas/.cargo/registry/src/-87fff15bd17e5479/alder-driver-0.3.0`, whose
`project.rs` still selects the first containing member. An actual nested-workspace
CLI check failed with String/Number mismatch; the rebuilt workspace CLI passed
the same four-source project at `/tmp/alder-nested-workspace.PuPEKd`.

Do not reuse a package target directory as final evidence when repackaging
changed local crates under unchanged versions. The previous fresh-target result
below remains evidence for its original source state only. A fresh verification
in `/tmp/alder-package-nested.NWQCRV` packaged and verified all 17 crates, exiting
zero. Its CLI passes the four-source nested-workspace check from `/tmp` in both
member orders, and executes the traits and explicit_async fixtures successfully.
The new target's temporary registry was unpacked afresh, rather than reusing the
older driver's registry source. No Cargo registry cache was deleted or modified
manually. Workspace Cargo.lock is unchanged. The ownership fix, permanent
regression, module documentation, and Sampo changeset are committed as `adb35ac`.

Formatting, strict all-target/all-feature Clippy, and full `cargo test` including
doctests pass at this checkpoint (160 driver and 414 solver integration tests;
three pre-existing ignored doctests). This is local host verification, not a
remote release, cross-platform build, or full-hardening completion claim.

## Current contextual-inference checkpoint

Started fresh verification after structural error capabilities and contextual
Option/record inference work:

```sh
cargo package --workspace --exclude stub --allow-dirty --target-dir /tmp/alder-package-current.6yJzBM
```

This is the CI package command with `--allow-dirty` for the uncommitted hardening
worktree and a newly allocated target directory. Verification is enabled.
All 17 archives were created and their extracted sources compiled successfully;
the command exited zero. Archive
inspection confirms all 17 canonicalizer stdlib files, including Semaphore and
SynchronizedRef, and the kernel build script plus TypeScript source.

The rebuilt workspace CLI executes all four repository examples: hello, pipes,
traits, and async, each with its expected success message. Two generated binary
interface caches under `examples/traits/.alder` were found tracked in Git; they
are removed from the index but retained locally, and `.alder/` directories are
now ignored at the repository root. No source or user data was deleted.
This repository-hygiene checkpoint is committed as `23dcc41`; the compiler
hardening changes remain separate and uncommitted.

The CLI built by package verification, `/tmp/alder-package-current.6yJzBM/debug/alder`,
was then executed from `/tmp` against all four examples and the traits and
explicit_async end-to-end projects. All six runs exited zero. The traits fixture
includes the new structural error dictionaries and contextual Option/record
cases; explicit_async includes the public traversal/synchronization integration.
Workspace Cargo.lock was unchanged, and no pending snapshots were found.
This is local host package verification, not a Linux/Windows/cargo-dist build
or a claim that remaining hardening acceptance work is complete.

Earlier verification below is historical and does not validate these newer
source changes or the eventual final hardening commit.

## September 5 working-tree verification

After the nested-pin short-circuit fix, all 17 publishable crates packaged and
verified successfully with extracted sources and a fresh target directory:

```sh
cargo package --workspace --exclude stub --offline --allow-dirty --target-dir /tmp/alder-package-hardening.tGBPLq
```

The target directory was allocated with `mktemp -d`; verification was not
disabled. The canonicalizer archive contains all 17 stdlib files, including
Semaphore and SynchronizedRef. The kernel archive contains its build script
and `kernel/src/index.ts`. Cargo.lock in the workspace was unchanged.

The resulting `/tmp/alder-package-hardening.tGBPLq/debug/alder` was run from
`/tmp`, independently of the repository working directory:

| Probe | Observed result |
| --- | --- |
| Original trait generic specialization | Compile-time generic-specialization diagnostic at the method |
| Original shared array | Compile-time Number/String mismatch at the alias annotation |
| Original zero-iteration return | Compile-time missing-return diagnostic |
| Original duplicate module | Compile-time diagnostic naming both physical files |
| Original record projections | Runs successfully |
| Original loop value | Prints 42 |
| Original nested Option equality | Runs successfully |
| 20,000 recursive awaits | Runs successfully with explicit async declarations |
| Source-relative Promise extern | Runs successfully with explicit async main |
| Restored whitespace template | Executes before and after formatting; first format changes one file, second changes none |

Original probes remain at `/tmp/alder-review.D3XIzP`. Current-syntax copies of
the deep/extern probes and the restored three-space template line are at
`/tmp/alder-current-repros.f86n1p`; original source files were not edited.
The original Node fairness probe also reports that the timer ran during
10,000 fulfilled Promise awaits against the current kernel source.

The packaged CLI additionally executes `examples/hello`, `examples/pipes`, and
the traits, records, explicit_async, and pattern_bindings (`-- success`) fixtures.
This verifies current working-tree packaging and local execution only. It does
not close the full hardening goal: the tuple-projection solver test is still
red, public traversal/type decisions remain unresolved, and final committed
release-set/remote-platform verification is still required.

## Earlier committed checkpoint

Checkpoint: compiler-hardening after `32fe7de`, Rust/Cargo 1.92.0.

All 17 publishable workspace crates were packaged and verified with:

```sh
cargo package --workspace --exclude stub --offline --target-dir /tmp/alder-package-hardening.KwkjGq
```

That target directory was freshly allocated with `mktemp -d`. Cargo built the
extracted archives and resolved workspace dependencies through its temporary
registry; this was not `--no-verify` or a workspace-only build. The unpublished
`tasks/stub` package is deliberately excluded.

Archive inspection confirmed all 15 embedded stdlib files in `alder-can` and
`kernel/src/index.ts` plus its build script in `alder-kernel`. Verification
successfully compiled the include/build-script paths outside the workspace.

An initial run against the existing workspace target directory failed with
stale dependency API errors in codegen, despite the current packaged sources
containing those APIs. A fresh target directory passed without source changes.
Repeated pre-release packaging of unchanged version numbers must not rely on
old dependency artifacts. The CI check now verifies packages in a fresh runner
temporary target directory, separately from the cached workspace build.

The CLI produced by package verification executed `examples/hello` and the
`async`, `externs`, `traits`, `records`, and `control_flow` end-to-end fixtures
with `/tmp` as the working directory. This checks both packaged compiler/runtime
integration and physical source-relative extern resolution.

`cargo dist plan --tag alder-cli-v0.2.3` also succeeded with cargo-dist 0.32.0.
It planned the configured macOS/Linux ARM64 and x86-64 binaries, Windows x86-64
binary, checksums, and shell/PowerShell/npm/Homebrew installers. This command
did not create a tag, publish a release, or execute an installer.

Limits: this is local host verification, not proof of Linux/Windows builds,
cross-architecture builds, installer behavior, or Sampo publication. Existing
CI includes Linux checks and a Windows CLI build; cargo-dist builds the release
matrix. Those remote jobs have not been run for this checkpoint. Rerun package
verification against the final hardening commit and the eventual Sampo version
updates before treating the release gate as complete.
