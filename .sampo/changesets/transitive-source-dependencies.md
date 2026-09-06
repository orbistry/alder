---
cargo/alder-driver: patch
cargo/alder-cli: patch
---

Discover transitive source dependencies using each dependency project's own
manifest and path base, without requiring prebuilt semantic caches.
Reject distinct dependency roots claiming one package identity, while coalescing
equivalent paths to the same canonical root.
Resolve workspace imports against their owning member's dependency declarations,
without activating unused sibling dependencies.
