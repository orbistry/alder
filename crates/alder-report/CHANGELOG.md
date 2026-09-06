# alder-report

## 0.3.0 — 2026-09-06

### Minor changes

- [6590795](https://github.com/orbistry/alder/commit/65907955dca4efc84622b5e72219274763826e95) Generate unused import-binding warnings through resolved source lookups, respecting
  aliases, shadowing, type and trait uses, and public re-exports. Preserve initializer
  effects and source-order diagnostic delivery in check, build, and test commands. — Thanks @rvcas!

### Patch changes

- [166922e](https://github.com/orbistry/alder/commit/166922ed53cd591e6b647fc6d03e168961b8d8dc) Display compiler errors and warnings with project-relative source paths in the
  CLI, including related and bundler diagnostics, without changing editor file
  identities or diagnostic locations. Supported terminals receive explicit absolute
  file hyperlinks behind the short labels so navigation does not depend on the
  shell's working directory. — Thanks @rvcas!

## 0.2.0 — 2026-09-03

### Minor changes

- [b699950](https://github.com/orbistry/alder/commit/b699950207423f198107d2872dabf690d84235b3) Add source-aware miette diagnostics for compiler errors and warnings, render
  trait and parser failures with labeled snippets, and preserve Alder source in
  code generation snapshots. — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-region@0.2.0

