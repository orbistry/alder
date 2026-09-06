---
cargo/alder-driver: minor
cargo/alder-cli: patch
---

Stop body compilation after source-package coherence failures, reporting errors
at their defining modules and marking other modules blocked. Avoid phantom
source labels for overlapping implementations declared in different modules.
