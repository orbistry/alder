# alder-language-server

## 0.2.3 — 2026-09-15

### Patch changes

- Updated dependencies: alder-driver@0.8.0

## 0.2.2 — 2026-09-11

### Patch changes

- Updated dependencies: alder-driver@0.7.0

## 0.2.1 — 2026-09-07

### Patch changes

- [2cc1d05](https://github.com/orbistry/alder/commit/2cc1d057460100bce03b09d5cda3f8e7975f1a87) Unify bundled, local, and external imports with grouped syntax, lowercase
  standard-library namespaces, explicit utility imports, identity-preserving
  namespace re-exports, and canonical comment-preserving formatting. Share public
  interfaces between prelude and explicit imports, reject conflicting bindings,
  and make module initialization independent of import declaration order.
  
  Interface format 8 replaces earlier contracts without compatibility readers. — Thanks @rvcas!
- Updated dependencies: alder-driver@0.6.0

## 0.2.0 — 2026-09-06

### Minor changes

- [2802ca8](https://github.com/orbistry/alder/commit/2802ca86466b50ca2ad33a300c1b452220a4df7b) Publish real compiler errors and warnings for versioned unsaved documents,
  recheck dependents, and clear stale editor diagnostics. Preserve UTF-16 source
  ranges and secondary requirement locations. Fix incomplete stdio responses by
  using the native Tokio language-server transport. — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-driver@0.5.0, alder-report@0.3.0

