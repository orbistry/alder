---
cargo/alder-solve: patch
cargo/alder-constrain: patch
cargo/alder-driver: patch
---

Reject ordinary types in converted Result error annotations even when the
function only forwards its argument without constructing or matching a Result.
Validate unused alias and enum payload declarations too, and report invalid
error arguments with a source-labeled error-row diagnostic.
Check bodyless trait signatures, associated-type bindings, and error-group
payloads without requiring a use site.
Validate fixed error slots in partial Result constructors and match structural
error rows when resolving higher-kinded trait implementations.
