---
cargo/alder-driver: patch
cargo/alder-cli: patch
---

Reject interface-only dependency caches whose package instance index disagrees
with the implementation headers in their module interfaces, even when each file
has a valid fingerprint.
