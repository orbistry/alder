---
cargo/alder-driver: minor
cargo/alder-region: patch
---

Replace the legacy export-name cache with versioned semantic interface and
package-instance-index files. Preserve complete trait, associated-type,
dictionary, and public type metadata, validate hydration round trips, and use
canonical SHA-256 fingerprints.
