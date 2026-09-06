# Explicit driver module identities

The driver no longer derives semantic identities from URI spelling. Every
source URI requires both an owning package and a source-root-relative module
path in `BuildDependencies`. Filesystem project discovery supplies these maps;
embedded sources supply their own identities. Metadata-free build and graph
entry points have been removed rather than retained as compatibility wrappers.

Graph construction and compilation validate the same maps. Missing metadata
is selected in sorted URI order and rejected before interface construction or
code emission. A build-level diagnostic has no fabricated source labels, and
modules prevented from compiling are reported as blocked. The CLI includes
these diagnostics in its normal error reporting.

Regression coverage checks missing package/path entries independently, reversed
discovery order, and equivalent explicit identities attached to filesystem and
in-memory URIs. The latter checks both stored interface identity and emitted
artifact identity/code. Existing duplicate-module, package-resolution, workspace
ownership, and cross-module tests remain in the driver suite.

Checkpoint validation uses an isolated copy of the Git index at
`/tmp/alder-identity-check.aILgc4`, excluding unrelated uncommitted hardening
changes. All 124 driver tests, formatting checks, strict workspace Clippy, and
the full workspace test/doctest run passed there (two existing doctests remain
ignored). No pending snapshot files were produced. This evidence does not close
the remaining package-coherence or final release-readiness audit.

## Fresh-build discovery-order regression

The nested-workspace regression now constructs twelve fresh databases instead
of reusing one database for two builds. It varies source insertion, module
discovery, and workspace member order. Every run compares graph build order,
emitted module identities and JavaScript, serialized interfaces, and serialized
package instance indexes. Local imports from both nested source roots retain
their separate owning packages. The expanded regression passes without changing
production code.

This is stronger evidence for this concrete nested-root build than the earlier
warm-database comparison. It does not establish arbitrary downstream runtime
initialization ordering. External-member relocation is covered separately below.

Validation on the joint hardening worktree: full workspace tests and doctests,
formatting, and strict all-target/all-feature Clippy pass (170 driver tests,
436 solver integration tests; two existing ignored doctests). No pending
snapshots were produced. This checkpoint changes tests and documentation only.

## External-member relocation

A regression moved a workspace and two external sibling application members
together, preserving relative layout. Both members' identities changed because
`strip_prefix` fell back to their absolute paths. Member keys now hash relative
paths including parent components whenever filesystem prefixes agree. Distinct
same-basename siblings still have distinct identities. The regression failed
before this change and passes afterwards; path tests cover same-root, descendant,
ancestor, sibling, and multiple-parent relationships.

Different filesystem prefixes retain absolute paths; a Windows-only regression
covers separate drives. That platform-specific test is not claimed as executed
on the local macOS host. Moving a member independently so its relationship to
the workspace changes intentionally changes its application-member identity.

Full workspace tests and doctests, formatting, and strict Clippy pass after the
fix: 172 driver tests, 436 solver integration tests, and 13 CLI tests, with the
two existing ignored doctests. Release packaging must be refreshed for this
new driver source before final acceptance.
