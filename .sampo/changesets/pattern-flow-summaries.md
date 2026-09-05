---
cargo/alder-ast: patch
---

Preserve pin-expression exits in structural match control-flow summaries.
Distinguish matching from rejection so unreachable sibling patterns, alternatives,
guards, and arm bodies cannot contribute exits.
Apply guards per alternative, preserving retries after a false guard.
