# Import unification

Replace the provisional import/prelude system without compatibility aliases.
The target grammar is in SPEC.md; this checklist tracks implementation, not
already available behavior.

- [x] Distinct bundled, local, and external roots; grouped entries with precise regions.
- [x] Lowercase bundled identities and ordinary implicit prelude imports.
- [x] Explicit non-prelude utilities and types; remove provisional web globals.
- [x] Identity-aware duplicate bindings, shadowing, and ambiguity diagnostics.
- [x] Selective, wildcard, and namespace re-exports through serialized interfaces.
- [x] Deterministic initialization independent of import declaration order.
- [x] Canonical grouped formatting with attached and standalone comments.
- [x] Repository-wide source, test, snapshot, documentation, and editor migration.
- [x] Changesets, formatting, strict Clippy, and full test validation.

Initial inspection found source-ordered ESM initialization imports despite a
sorted graph, a parser restriction on public namespace imports, and builtin
values bypassing ordinary interfaces. Those boundaries are now unified.
Source declarations use lowercase files and builtin module IDs; core runtime
type/trait identities remain unchanged. Interface format 8 intentionally rejects
older serialized contracts. The formatter execution regression compares
complete bundles before/after sorting and asserts shared identity and effects.
The language server uses shared driver diagnostics and currently advertises
document synchronization, not completion or formatting. CLI/editor integration
tests cover grouped-entry regions and clearing corrected unsaved diagnostics.

Validation completed: `cargo fmt --all`,
`cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test`.
Snapshots were reviewed and accepted. Deferred HTTP, router, DI, and macro
examples use explicit lowercase imports without adding those implementations.
