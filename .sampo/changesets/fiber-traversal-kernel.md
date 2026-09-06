---
cargo/alder-kernel: minor
---

Add bounded lazy traversal workers with ordered map results, unit-only forEach,
explicit typed-error traversal, and structured cancellation. Stop active siblings
as soon as an item defect or selected typed error is observed, before waiting for
that item's cleanup, and join all cleanup before completing the traversal.
