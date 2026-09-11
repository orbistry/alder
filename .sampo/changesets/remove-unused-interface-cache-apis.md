---
cargo/alder-driver: minor
---

Remove unused `ModuleMeta`, `InterfaceCache::start_build`, and
`InterfaceCache::needs_rebuild` APIs that did not participate in source builds.
Remove the unused error-discarding `load_package_index` convenience method;
`load_package_index_checked` remains the validated loading API.
