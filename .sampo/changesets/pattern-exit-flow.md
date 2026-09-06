---
cargo/alder-solve: patch
cargo/alder-cli: patch
---

Track pattern matching, rejection, and pin-expression exits in control-flow
analysis. Keep pin break values in loop result checking and exclude guards,
arm bodies, later alternatives, and sibling pattern operands that an earlier
pin exits before reaching.
