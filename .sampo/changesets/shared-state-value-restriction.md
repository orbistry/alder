---
cargo/alder-constrain: patch
cargo/alder-solve: patch
cargo/alder-driver: patch
---

Keep shared top-level state monomorphic, preserve safe factory polymorphism, and prevent interfaces from turning unresolved shared types into independent generics. Diagnose incomplete shared export types.
