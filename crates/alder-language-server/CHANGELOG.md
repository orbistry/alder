# alder-language-server

## 0.2.0 — 2026-09-06

### Minor changes

- [2802ca8](https://github.com/orbistry/alder/commit/2802ca86466b50ca2ad33a300c1b452220a4df7b) Publish real compiler errors and warnings for versioned unsaved documents,
  recheck dependents, and clear stale editor diagnostics. Preserve UTF-16 source
  ranges and secondary requirement locations. Fix incomplete stdio responses by
  using the native Tokio language-server transport. — Thanks @rvcas!

### Patch changes

- Updated dependencies: alder-driver@0.5.0, alder-report@0.3.0

