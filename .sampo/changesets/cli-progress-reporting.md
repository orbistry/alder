---
cargo/alder-cli: minor
cargo/alder-driver: minor
cargo/alder-runtime: minor
cargo/alder-kernel: patch
---

Add consistent Cargo-style CLI statuses, global quiet/verbose/color options,
elapsed summaries, accurate diagnostic counts, and compiler proxy reporting.
Expose optional semantic driver progress and an injected CLI renderer while
preserving source diagnostics, hyperlinks, and program/LSP streams. Add opt-in
structured runtime test results so CLI test summaries use actual executed counts
on stderr without intercepting user output or duplicating failure summaries.
