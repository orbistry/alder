---
cargo/alder-solve: patch
cargo/alder-codegen: patch
---

Preserve contextual field types inside fresh Option-wrapped record and array
payloads. Apply ordinary checked-call rules to bare pipe destinations, including
Option lifting, omitted arguments, and dictionary passing.
