# Goal: Elm-quality parser diagnostics

Continue on `diagnostic-ux`. Bring Alder's active parser diagnostics up to Elm's
standard of specificity, source context, and actionable guidance, while preserving
Alder's own grammar and semantics. Completed and validated on 2026-09-06. The variant inventory, rendered evidence,
Elm comparison, and documented limitations are in `docs/parser-diagnostic-parity.md`.

## Starting point

Read `AGENTS.md`, `SPEC.md`, `docs/compiler-implementation-map.md`,
`plans/compiler-hardening.md`, and `docs/tooling.md`. Use the local
`elm/compiler/src/Reporting/Error/Syntax.hs` and `elm/compiler/src/Parse/` as the
primary reference. Alder's design documents win when its language differs.

The active path is `alder-parse`'s nested errors through
`crates/alder-driver/src/report.rs` to `alder-report` and the CLI/LSP. At the starting point, detailed
errors were discarded by generic `invalid ...` branches, notably for
arrays, tuples, records, calls, conditionals, matches, patterns, and newer language
constructs. Some declarations and type arguments already had useful diagnostics.

## Work

- [x] Inventory every active parser error variant and its rendered handling.
  Record generic fallbacks, discarded nested causes/positions, and missing context.
  Distinguish active Alder paths from inactive Elm-era code.
- [x] Add granular malformed-source tests and rendered diagnostic snapshots before
  implementing each family. Review the output, not merely the error enums.
- [x] Replace generic fallbacks with precise explanations: what was being parsed,
  what went wrong, what was expected, and a concrete correction when justified.
  Cover expressions, patterns, types, declarations, and all currently parsed
  Alder-specific constructs, even those not yet executable.
- [x] Preserve nested error locations and useful enclosing context. Highlight the
  actual failure and, where useful, its opening delimiter or enclosing construct.
  Handle EOF, multiline source, Unicode, and nested failures accurately.
- [x] Add source-aware guidance for common mistakes where evidence supports it,
  including mismatched/missing delimiters, separators, reserved words, and Elm or
  Rust syntax habits. Do not copy Elm-only indentation or trailing-comma rules into
  Alder, invent confident fixes, or change accepted syntax to simplify reporting.
- [x] Keep explanations concise and consistent. Reuse reporting infrastructure;
  extend parser errors only where information is genuinely missing.
- [x] Verify CLI and LSP use the improved diagnostics without losing canonical
  source identities, related labels, readable paths, or terminal hyperlink targets.
- [x] Update relevant design/progress documents and add per-crate changesets.

## Acceptance

- [x] Every active parser error family has reviewed rendered coverage, including
  representative nested failures; generic fallbacks are eliminated or individually
  documented with a concrete justification.
- [x] Missing tokens and malformed subexpressions report their specific cause at
  the correct location rather than only saying that the enclosing construct is
  invalid. Add regressions for `If::ThenKeyword`, `Record::EqualsNotColon`, missing
  array/call separators, malformed match arms, and nested pattern failures.
- [x] Compare representative outputs against the equivalent local Elm reporting
  paths for specificity, contextual snippets, and useful repair advice. Document
  remaining gaps honestly; parity is not established by test counts alone.
- [x] Valid programs retain their parsing/compilation behavior. Parser recovery
  redesign and unrelated type diagnostics are outside scope unless separately
  authorized.
- [x] `cargo fmt --all --check`,
  `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` pass.
  Review and accept new snapshots deliberately; no unexplained ignored tests or
  unreviewed snapshot updates.
- [x] Finish with an evidence-backed summary of coverage, validation, and any
  remaining limitations. Do not mark this goal complete while required gaps remain.
