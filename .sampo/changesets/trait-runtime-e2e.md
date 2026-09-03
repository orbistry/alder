---
cargo/alder-can: patch
cargo/alder-solve: patch
cargo/alder-codegen: patch
cargo/alder-kernel: patch
---

Preserve optional enum payload fields through canonicalization and inference,
omit them from derived JSON, and accept them as absent when decoding. Render
derived record-payload constructors in Alder syntax and source field order.
Derived hashes now include canonical type identity, declaration variant index,
and every payload field. Derived JSON decoding rejects unexpected envelope and
payload fields.
