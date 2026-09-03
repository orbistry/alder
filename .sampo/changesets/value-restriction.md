---
cargo/alder-solve: patch
cargo/alder-driver: patch
---

Keep mutable and local bindings monomorphic, and subtract type variables held
by the outer environment when generalizing top-level values. Report unresolved
trait obligations over non-generalized variables as type ambiguity rather than
suggesting an impossible generic bound.
