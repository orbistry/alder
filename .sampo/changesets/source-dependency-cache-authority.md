---
cargo/alder-driver: patch
cargo/alder-cli: patch
---

Build source-backed dependencies from current source without mixing in saved
interfaces or instance indexes, so removed trait implementations cannot remain
available to consumers through stale caches.
